use std::{
    collections::{HashMap, HashSet},
    string::ToString,
    sync::Arc,
    time::Duration,
};

use anyhow::{anyhow, Context};
use backoff::ExponentialBackoff;
use chrono::prelude::*;
use futures::Future;
use hyper::header;
use reqwest::Client;
use tokio::{
    sync::{broadcast, mpsc, watch, Mutex},
    time::sleep,
};
use tracing::{debug, debug_span, error, info, instrument, warn, Instrument};

use utility::{
    config::{Config, User},
    here,
    streams::{Livestream, StreamUpdate, VideoStatus},
    validate_response,
};

use crate::{discord_api::DiscordMessageData, types::holo_api::*};

type StreamIndex = Arc<Mutex<HashMap<String, Livestream>>>;
type NotifiedStreams = Arc<Mutex<HashSet<String>>>;

pub struct HoloApi;

impl HoloApi {
    const INITIAL_STREAM_FETCH_COUNT: u32 = 1000;

    #[instrument(skip(config, live_sender, update_sender, exit_receiver))]
    pub async fn start(
        config: Config,
        live_sender: mpsc::Sender<DiscordMessageData>,
        update_sender: broadcast::Sender<StreamUpdate>,
        mut exit_receiver: watch::Receiver<bool>,
    ) -> watch::Receiver<HashMap<String, Livestream>> {
        let stream_index = Arc::new(Mutex::new(HashMap::new()));
        let notified_streams = Arc::new(Mutex::new(HashSet::<String>::new()));

        let (index_sender, index_receiver) = watch::channel(HashMap::new());

        let notifier_lock = StreamIndex::clone(&stream_index);
        let notified_streams_prod = NotifiedStreams::clone(&notified_streams);

        let mut exit_receiver_clone = exit_receiver.clone();
        let notifier_sender = update_sender.clone();

        tokio::spawn(
            async move {
                tokio::select! {
                    res = Self::stream_producer(config, stream_index, notified_streams_prod, index_sender, update_sender) => {
                        if let Err(e) = res {
                            error!("{:?}", e);
                        }
                    }

                    res = exit_receiver_clone.changed() => {
                        if let Err(e) = res {
                            error!("{:#}", e);
                        }
                    }
                }

                info!(task = "Stream indexer", "Shutting down.");
            }
            .instrument(debug_span!("Starting task.", task_type = "Stream indexer")),
        );

        tokio::spawn(
            async move {
                tokio::select! {
                    res = Self::stream_notifier(notifier_lock, notified_streams, live_sender, notifier_sender) => {
                        if let Err(e) = res {
                            error!("{:?}", e);
                        }
                    }

                    res = exit_receiver.changed() => {
                        if let Err(e) = res {
                            error!("{:#}", e);
                        }
                    }
                }

                info!(task = "Stream notifier", "Shutting down.");
            }
            .instrument(debug_span!("Starting task.", task_type = "Stream notifier")),
        );

        index_receiver
    }

    async fn try_run<F, R, Fut>(
        func: F, /*
                 client: &Client,
                 parameters: &ApiLiveOptions,
                 users: &HashMap<String, User>, */
    ) -> anyhow::Result<R>
    where
        F: Fn() -> Fut,
        Fut: Future<Output = anyhow::Result<R>>,
    {
        let backoff_config = ExponentialBackoff {
            initial_interval: Duration::from_secs(4),
            max_interval: Duration::from_secs(64 * 60),
            randomization_factor: 0.0,
            multiplier: 2.0,
            ..ExponentialBackoff::default()
        };

        Ok(backoff::future::retry(backoff_config, || async {
            let streams = func().await.map_err(|e| {
                warn!("{}", e.to_string());
                anyhow!(e).context(here!())
            })?;

            Ok(streams)
        })
        .await
        .context(here!())?)
    }

    #[instrument(skip(config, producer_lock, notified_streams, index_sender, stream_updates))]
    async fn stream_producer(
        config: Config,
        producer_lock: StreamIndex,
        notified_streams: NotifiedStreams,
        index_sender: watch::Sender<HashMap<String, Livestream>>,
        stream_updates: broadcast::Sender<StreamUpdate>,
    ) -> anyhow::Result<()> {
        let mut headers = header::HeaderMap::new();

        let mut auth_value = header::HeaderValue::from_str(&config.holodex_key)?;
        auth_value.set_sensitive(true);
        headers.insert(header::HeaderName::from_static("x-apikey"), auth_value);

        let client = reqwest::ClientBuilder::new()
            .default_headers(headers)
            .user_agent(concat!(
                env!("CARGO_PKG_NAME"),
                "/",
                env!("CARGO_PKG_VERSION"),
            ))
            .build()
            .context(here!())?;

        let user_map = config
            .users
            .iter()
            .map(|u| (u.channel.clone(), u.clone()))
            .collect::<HashMap<_, _>>();

        let parameters = ApiLiveOptions {
            limit: 100,
            ..Default::default()
        };

        // Start by fetching the latest N streams.
        {
            let mut stream_index = producer_lock.lock().await;

            *stream_index = Self::try_run(|| async {
                HoloApi::get_streams(
                    &client,
                    &ApiLiveOptions {
                        limit: Self::INITIAL_STREAM_FETCH_COUNT,
                        status: vec![
                            VideoStatus::New,
                            VideoStatus::Upcoming,
                            VideoStatus::Live,
                            VideoStatus::Past,
                        ],
                        ..parameters.clone()
                    },
                    &user_map,
                )
                .await
            })
            .await?;

            debug!("Starting stream index update!");
            index_sender.send(stream_index.clone()).context(here!())?;
            debug!(size = %stream_index.len(), "Stream index updated!");
        }

        loop {
            let mut stream_index = producer_lock.lock().await;

            let channels_to_update = stream_index
                .iter()
                .filter_map(|(channel, stream)| {
                    matches!(stream.state, VideoStatus::Upcoming | VideoStatus::Live)
                        .then(|| channel.clone())
                })
                .collect::<Vec<_>>();

            let mut index_dirty = false;

            // Fetch updates for the streams that are currently live or scheduled.
            if !channels_to_update.is_empty() {
                let updated_streams = Self::try_run(|| async {
                    Self::check_stream_updates(&client, &channels_to_update, &stream_index).await
                })
                .await?;

                if !updated_streams.is_empty() {
                    index_dirty = true;
                }

                let mut notified = notified_streams.lock().await;

                for update in updated_streams {
                    match update {
                        VideoUpdate::Scheduled(s) => {
                            let entry = stream_index.get_mut(&s).unwrap();
                            (*entry).state = VideoStatus::Upcoming;

                            stream_updates
                                .send(StreamUpdate::Scheduled(entry.clone()))
                                .context(here!())?;
                        }
                        VideoUpdate::Started(s) => {
                            let entry = stream_index.get_mut(&s).unwrap();
                            (*entry).state = VideoStatus::Live;

                            stream_updates
                                .send(StreamUpdate::Started(entry.clone()))
                                .context(here!())?;
                        }
                        VideoUpdate::Ended(s) => {
                            let entry = stream_index.get_mut(&s).unwrap();
                            (*entry).state = VideoStatus::Past;

                            // Remove ended stream from set of notified streams.
                            if !notified.remove(&entry.url) {
                                warn!(stream = %entry.title, "Stream ended which was not in the notified streams cache.");
                            }

                            info!("Stream has ended!");

                            stream_updates
                                .send(StreamUpdate::Ended(entry.clone()))
                                .context(here!())?;
                        }
                    }
                }
            }

            let mut offset = 0;

            // Fetch 100 streams at a time until reaching already cached streams.
            loop {
                let mut stream_index = producer_lock.lock().await;

                let stream_batch = Self::try_run(|| async {
                    Self::get_streams(
                        &client,
                        &ApiLiveOptions {
                            offset,
                            ..parameters.clone()
                        },
                        &user_map,
                    )
                    .await
                })
                .await?;

                let mut reached_indexed_data = false;

                for (id, stream) in stream_batch {
                    if stream_index.contains_key(&id) {
                        reached_indexed_data = true;
                        continue;
                    }

                    if !index_dirty {
                        index_dirty = true;
                    }

                    stream_updates
                        .send(StreamUpdate::Scheduled(stream.clone()))
                        .context(here!())?;

                    stream_index.insert(id, stream);
                }

                if reached_indexed_data {
                    break;
                } else {
                    offset += parameters.limit as i32;
                }
            }

            if index_dirty {
                debug!("Starting stream index update!");
                index_sender.send(stream_index.clone()).context(here!())?;
                debug!(size = %stream_index.len(), "Stream index updated!");
            }

            std::mem::drop(stream_index);

            /* let streams = Self::try_get_streams(&client, &parameters, &user_map).await?;

            let mut stream_index = producer_lock.lock().await;
            let mut new_index = HashMap::with_capacity(stream_index.capacity());

            if !stream_index.is_empty() {
                // Check for newly scheduled streams.
                for (id, scheduled_stream) in &scheduled_streams {
                    if !stream_index.contains_key(id) {
                        stream_updates
                            .send(StreamUpdate::Scheduled(scheduled_stream.clone()))
                            .context(here!())?;
                    }
                }
            }

            // Update new index.
            new_index.extend(scheduled_streams.into_iter());
            new_index.extend(live_streams.into_iter());
            new_index.retain(|i, _| !ended_streams.contains_key(i));

            // Check for ended streams.
            if !ended_streams.is_empty() {
                let mut notified = notified_streams.lock().await;

                for (id, ended_stream) in ended_streams {
                    if stream_index.contains_key(&id) {
                        // Remove ended stream from set of notified streams.
                        if !notified.remove(&ended_stream.url) {
                            warn!(stream = %ended_stream.title, "Stream ended which was not in the notified streams cache.");
                        }

                        info!("Stream has ended!");
                        stream_updates
                            .send(StreamUpdate::Ended(ended_stream))
                            .context(here!())?;
                    }
                }
            }

            for stream_id in stream_index.keys() {
                let indexed = stream_index.get(stream_id).unwrap();

                if !new_index.contains_key(stream_id)
                    && stream_index.get(stream_id).unwrap().state != StreamState::Live
                    && (indexed.start_at - Utc::now()).num_minutes() < 5
                {
                    error!(
                        "Stream not in API despite starting in less than 5 minutes!\n{} from {}.",
                        indexed.title, indexed.streamer.display_name
                    );
                }
            }

            debug!("Starting stream index update!");
            index_sender.send(new_index.clone()).context(here!())?;
            debug!(size = %new_index.len(), "Stream index updated!");

            *stream_index = new_index;
            std::mem::drop(stream_index);
            debug!("Stream index update finished!"); */

            sleep(Duration::from_secs(60)).await;
        }
    }

    #[instrument(skip(notifier_lock, notified_streams, discord_sender, live_sender))]
    async fn stream_notifier(
        notifier_lock: StreamIndex,
        notified_streams: NotifiedStreams,
        discord_sender: mpsc::Sender<DiscordMessageData>,
        live_sender: broadcast::Sender<StreamUpdate>,
    ) -> anyhow::Result<()> {
        let mut next_stream_start = Utc::now();

        loop {
            let mut sleep_duration = Duration::from_secs(60);
            let mut stream_index = notifier_lock.lock().await;
            let mut notified = notified_streams.lock().await;

            let mut sorted_streams = stream_index
                .iter()
                .filter(|(_, s)| {
                    !notified.contains(&s.url)
                        && (s.state == VideoStatus::Upcoming
                            || (Utc::now() - s.start_at) <= chrono::Duration::minutes(15))
                })
                .collect::<Vec<_>>();

            if sorted_streams.is_empty() {
                std::mem::drop(notified);
                std::mem::drop(stream_index);
                sleep(sleep_duration).await;
                continue;
            }

            sorted_streams.sort_unstable_by_key(|(_, s)| s.start_at);

            let start_at = sorted_streams[0].1.start_at;
            let remaining_time = start_at - Utc::now();

            // Only write to log if the time for the next stream changes.
            if start_at != next_stream_start {
                next_stream_start = start_at;

                info!(
                    "Next streams are {}.",
                    chrono_humanize::HumanTime::from(remaining_time).to_text_en(
                        chrono_humanize::Accuracy::Precise,
                        chrono_humanize::Tense::Future
                    )
                );
            }

            if remaining_time.num_seconds() > 10 {
                std::mem::drop(notified);
                std::mem::drop(stream_index);

                let remaining_time_std = remaining_time.to_std().context(here!())?;

                if remaining_time_std <= sleep_duration {
                    sleep_duration = remaining_time_std;
                }

                sleep(sleep_duration).await;
                continue;
            }

            let next_streams = sorted_streams
                .into_iter()
                .take_while(|(_, s)| s.start_at == start_at)
                .collect::<Vec<_>>();

            info!(
                "{}",
                next_streams
                    .iter()
                    .fold("Time to watch:".to_owned(), |acc, (_, s)| {
                        acc + format!("\n{}", s.streamer.display_name).as_str()
                    })
            );

            for (_, stream) in &next_streams {
                assert!(notified.insert(stream.url.clone()));

                live_sender
                    .send(StreamUpdate::Started((*stream).clone()))
                    .context(here!())?;

                discord_sender
                    .send(DiscordMessageData::ScheduledLive((*stream).clone()))
                    .await
                    .context(here!())?;
            }

            std::mem::drop(notified);

            // Update the live status with a new write lock afterwards.
            let live_ids = next_streams
                .into_iter()
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>();

            for id in live_ids {
                if let Some(s) = stream_index.get_mut(&id) {
                    s.state = VideoStatus::Live;
                }
            }

            std::mem::drop(stream_index);

            sleep(sleep_duration).await;
        }
    }

    #[instrument(skip(client, index))]
    async fn check_stream_updates(
        client: &Client,
        channels: &[String],
        index: &HashMap<String, Livestream>,
    ) -> anyhow::Result<Vec<VideoUpdate>> {
        let parameter = channels.join(",");

        let res = client
            .get("https://holodex.net/api/v2/users/live")
            .query(&[("channels", parameter)])
            .send()
            .await
            .context(here!())?;

        let streams: Vec<Video> = validate_response(res).await?;

        let mut updates = Vec::with_capacity(streams.len());

        for stream in streams {
            let entry = index
                .get(&stream.id)
                .ok_or_else(|| anyhow!("Couldn't find stream in index.").context(here!()))?;

            updates.push(match (entry.state, stream.status) {
                (VideoStatus::Upcoming, VideoStatus::Live) => {
                    VideoUpdate::Started(entry.id.clone())
                }
                (VideoStatus::Live, VideoStatus::Past) => VideoUpdate::Ended(entry.id.clone()),
                (VideoStatus::Missing, VideoStatus::Upcoming) => {
                    VideoUpdate::Scheduled(entry.id.clone())
                }
                (VideoStatus::Missing, VideoStatus::Live) => VideoUpdate::Started(entry.id.clone()),
                _ => continue,
            });
        }

        Ok(updates)
    }

    #[instrument(skip(client, users))]
    async fn get_streams(
        client: &Client,
        parameters: &ApiLiveOptions,
        users: &HashMap<String, User>,
    ) -> anyhow::Result<HashMap<String, Livestream>> {
        let res = client
            .get("https://holodex.net/api/v2/live")
            .query(&parameters)
            .send()
            .await
            .context(here!())?;

        let res: ApiLiveResponse = validate_response(res).await?;

        let videos = match res {
            ApiLiveResponse::Videos(v) => v,
            ApiLiveResponse::Page { total: _, items } => items,
        };

        Ok(videos
            .into_iter()
            .filter_map(|v| {
                let streamer = users.get(v.channel.get_id())?.clone();

                let id = v.id.clone();
                let thumbnail = format!("https://i3.ytimg.com/vi/{}/maxresdefault.jpg", &v.id);
                let url = format!("https://youtube.com/watch?v={}", &v.id);

                Some((
                    v.id.clone(),
                    Livestream {
                        id,
                        title: v.title.clone(),
                        thumbnail,
                        created_at: v.available_at,
                        start_at: v.live_info.start_scheduled.unwrap_or_else(Utc::now),
                        duration: (v.duration > 0).then(|| v.duration),
                        streamer,
                        state: v.status,
                        url,
                    },
                ))
            })
            .collect::<HashMap<_, _>>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOCK_SERVER: &str = "https://stoplight.io/mocks/holodex/holodex:main/11620234";
    const USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"),);

    #[tokio::test]
    async fn get_streams() {
        let client = reqwest::ClientBuilder::new()
            .user_agent(USER_AGENT)
            .build()
            .context(here!())
            .unwrap();

        let parameters = dbg!(ApiLiveOptions {
            limit: 25,
            ..Default::default()
        });

        let res = client
            .get(format!("{}/live", MOCK_SERVER))
            .query(&parameters)
            .header("Accept", "application/json");

        println!("{:#?}", res);

        let res = res.send().await.unwrap();

        let res: ApiLiveResponse = validate_response(res).await.unwrap();
        println!("{:?}", res);
    }

    #[tokio::test]
    async fn check_stream_update() {
        let client = reqwest::ClientBuilder::new()
            .user_agent(USER_AGENT)
            .build()
            .context(here!())
            .unwrap();

        let res = client
            .get(format!("{}/users/live", MOCK_SERVER))
            .query(&[("channels", "UCqm3BQLlJfvkTsX_hvm0UmA")])
            .header("Accept", "application/json");

        println!("{:#?}", res);

        let res = res.send().await.unwrap();

        let res: Vec<Video> = validate_response(res).await.unwrap();
        println!("{:?}", res);
    }

    #[test]
    fn check_empty_live_response() {
        let _res: ApiLiveResponse = serde_json::from_str("{\"total\":\"0\",\"items\":[]}").unwrap();
    }
}
