use std::{collections::HashMap, sync::Arc, time::Duration};

use anyhow::{anyhow, Context};
use chrono::prelude::*;
use hyper::header;
use reqwest::Client;
use tokio::{
    sync::{broadcast, mpsc, watch, Mutex},
    time::sleep,
};
use tracing::{debug, debug_span, error, info, instrument, trace, warn, Instrument};

use utility::{
    config::{Config, LoadFromDatabase, SaveToDatabase, User},
    discord::NotifiedStreamsCache,
    functions::{try_run, validate_response},
    here,
    streams::{Livestream, StreamUpdate, VideoStatus},
};

use crate::{discord_api::DiscordMessageData, types::holo_api::*};

type StreamIndex = Arc<Mutex<HashMap<String, Livestream>>>;

pub struct HoloApi;

impl HoloApi {
    const INITIAL_STREAM_FETCH_COUNT: u32 = 100;

    const USER_AGENT: &'static str =
        concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"),);
    const UPDATE_INTERVAL: Duration = Duration::from_secs(60);

    #[instrument(skip(config, live_sender, update_sender, exit_receiver))]
    pub async fn start(
        config: Config,
        live_sender: mpsc::Sender<DiscordMessageData>,
        update_sender: broadcast::Sender<StreamUpdate>,
        mut exit_receiver: watch::Receiver<bool>,
    ) -> watch::Receiver<HashMap<String, Livestream>> {
        let stream_index = Arc::new(Mutex::new(HashMap::new()));

        let (index_sender, index_receiver) = watch::channel(HashMap::new());

        let config_clone = config.clone();
        let index_clone = StreamIndex::clone(&stream_index);

        let mut exit_receiver_clone = exit_receiver.clone();
        let notifier_sender = update_sender.clone();

        let database_path = config.database_path.clone();

        tokio::spawn(
            async move {
                tokio::select! {
                    res = Self::stream_producer(config, stream_index, index_sender, update_sender) => {
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
                let mut notified_streams = NotifiedStreamsCache::new(128);

                tokio::select! {
                    res = Self::stream_notifier(config_clone, index_clone, &mut notified_streams, live_sender, notifier_sender) => {
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

                // Save notified streams cache to database.
                if let Ok(handle) = Config::open_database(&database_path) {
                    if let Err(e) = notified_streams.save_to_database(&handle) {
                        error!("{:#}", e);
                    }
                }
            }
            .instrument(debug_span!("Starting task.", task_type = "Stream notifier")),
        );

        index_receiver
    }

    #[instrument(skip(config, producer_lock, index_sender, stream_updates))]
    async fn stream_producer(
        config: Config,
        producer_lock: StreamIndex,
        index_sender: watch::Sender<HashMap<String, Livestream>>,
        stream_updates: broadcast::Sender<StreamUpdate>,
    ) -> anyhow::Result<()> {
        let mut headers = header::HeaderMap::new();

        let mut auth_value = header::HeaderValue::from_str(&config.holodex_key)?;
        auth_value.set_sensitive(true);
        headers.insert(header::HeaderName::from_static("x-apikey"), auth_value);

        let client = reqwest::ClientBuilder::new()
            .default_headers(headers)
            .user_agent(Self::USER_AGENT)
            .build()
            .context(here!())?;

        let user_map = config
            .users
            .iter()
            .map(|u| (u.channel.clone(), u.clone()))
            .collect::<HashMap<_, _>>();

        let parameters = ApiLiveOptions {
            limit: 100,
            status: vec![VideoStatus::Upcoming],
            ..Default::default()
        };

        // Start by fetching the latest N streams.
        {
            let mut stream_index = producer_lock.lock().await;

            *stream_index = try_run(|| async {
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

            trace!("Starting stream index update!");
            index_sender.send(stream_index.clone()).context(here!())?;
            debug!(size = %stream_index.len(), "Stream index updated!");
        }

        loop {
            let mut index_dirty = false;

            // Fetch updates for the streams that are currently live or scheduled.
            {
                let updates =
                    Self::get_stream_updates(client.clone(), Arc::clone(&producer_lock)).await?;

                for update in updates {
                    index_dirty = true;

                    trace!(?update, "Stream update received!");

                    stream_updates.send(update).context(here!())?;
                }
            }

            // Fetch 100 streams at a time until reaching already cached streams.
            {
                for (id, stream) in Self::fetch_new_streams(
                    &client,
                    Arc::clone(&producer_lock),
                    &parameters,
                    &user_map,
                )
                .await?
                {
                    trace!(name = %stream.title, "New stream added to index!");
                    index_dirty = true;

                    stream_updates
                        .send(StreamUpdate::Scheduled(stream.clone()))
                        .context(here!())?;

                    let mut stream_index = producer_lock.lock().await;
                    stream_index.insert(id, stream);
                }
            }

            if index_dirty {
                let stream_index = producer_lock.lock().await;

                trace!("Starting stream index update!");
                index_sender.send(stream_index.clone()).context(here!())?;
                debug!(size = %stream_index.len(), "Stream index updated!");
            }

            sleep(Self::UPDATE_INTERVAL).await;
        }
    }

    #[instrument(skip(config, stream_index, notified_streams, discord_sender, live_sender))]
    async fn stream_notifier(
        config: Config,
        stream_index: StreamIndex,
        notified_streams: &mut NotifiedStreamsCache,
        discord_sender: mpsc::Sender<DiscordMessageData>,
        live_sender: broadcast::Sender<StreamUpdate>,
    ) -> anyhow::Result<()> {
        // See if there's any cached notified streams in the database, to prevent duplicate alerts.
        if let Ok(handle) = config.get_database_handle() {
            debug!("Fetching notified streams from database...");

            match NotifiedStreamsCache::load_from_database(&handle) {
                Ok(cached_data) => {
                    debug!(
                        "{} notified streams found in database cache!",
                        cached_data.len()
                    );

                    for stream_id in cached_data {
                        notified_streams.put(stream_id, ());
                    }
                }
                Err(e) => {
                    error!("Failed to load notified stream cache!\n{:#}", e);
                }
            }
        }

        let mut next_stream_start = Utc::now();

        loop {
            let mut sleep_duration = Self::UPDATE_INTERVAL;
            let mut index = stream_index.lock().await;

            let mut sorted_streams = index
                .iter()
                .filter(|(_, s)| {
                    !notified_streams.contains(&s.url)
                        && (s.state == VideoStatus::Upcoming
                            || (s.state == VideoStatus::Live
                                && ((Utc::now() - s.start_at) <= chrono::Duration::minutes(1))))
                })
                .collect::<Vec<_>>();

            if sorted_streams.is_empty() {
                std::mem::drop(index);
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
                std::mem::drop(index);

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
                assert!(notified_streams.put(stream.url.clone(), ()).is_none());
                trace!(?stream, "Stream going live!");

                live_sender
                    .send(StreamUpdate::Started((*stream).clone()))
                    .context(here!())?;

                discord_sender
                    .send(DiscordMessageData::ScheduledLive((*stream).clone()))
                    .await
                    .context(here!())?;
            }

            // Update the live status with a new write lock afterwards.
            let live_ids = next_streams
                .into_iter()
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>();

            for id in live_ids {
                if let Some(s) = index.get_mut(&id) {
                    s.state = VideoStatus::Live;
                }
            }

            std::mem::drop(index);

            sleep(sleep_duration).await;
        }
    }

    #[instrument(skip(client, users))]
    async fn get_streams(
        client: &Client,
        parameters: &ApiLiveOptions,
        users: &HashMap<String, User>,
    ) -> anyhow::Result<HashMap<String, Livestream>> {
        let res = client
            .get("https://holodex.net/api/v2/videos")
            .query(&parameters)
            .send()
            .await
            .context(here!())?;

        let res: ApiLiveResponse = validate_response(res).await?;

        let videos = match res {
            ApiLiveResponse::Videos(v) => v,
            ApiLiveResponse::Page { total: _, items } => items,
        };

        if videos.len() > parameters.limit as usize {
            error!(
                "Holodex returned {} streams, when only {} were requested!",
                videos.len(),
                parameters.limit
            );
        }

        Ok(videos
            .into_iter()
            .filter_map(|v| {
                match &v.channel {
                    VideoChannel::Data(ChannelMin { org, .. }) if *org != parameters.org => {
                        return None
                    }
                    _ => (),
                }

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
                        start_at: v.live_info.start_scheduled.unwrap_or(v.available_at),
                        duration: (v.duration > 0).then(|| v.duration),
                        streamer,
                        state: v.status,
                        url,
                    },
                ))
            })
            .collect::<HashMap<_, _>>())
    }

    #[instrument(skip(client, stream_index))]
    async fn get_stream_updates(
        client: Client,
        stream_index: StreamIndex,
    ) -> anyhow::Result<Vec<StreamUpdate>> {
        let streams_to_update = {
            let index = stream_index.lock().await;

            index
                .iter()
                .filter_map(|(_, stream)| {
                    matches!(stream.state, VideoStatus::Upcoming | VideoStatus::Live)
                        .then(|| stream.id.clone())
                })
                .collect::<Vec<_>>()
        };

        debug!(count = streams_to_update.len(), "Streams to update!");

        if streams_to_update.is_empty() {
            return Ok(Vec::new());
        }

        let mut index = stream_index.lock().await;

        let updated_streams = try_run(|| async {
            Self::check_stream_updates(&client, &streams_to_update, &index).await
        })
        .await?;

        let mut updates = Vec::new();

        for update in updated_streams {
            match update {
                VideoUpdate::Scheduled(id) => {
                    let entry = index
                        .get_mut(&id)
                        .ok_or_else(|| anyhow!("Entry wasn't in index!"))?;
                    (*entry).state = VideoStatus::Upcoming;

                    updates.push(StreamUpdate::Scheduled(entry.clone()));
                }
                VideoUpdate::Started(id) => {
                    let entry = index
                        .get_mut(&id)
                        .ok_or_else(|| anyhow!("Entry wasn't in index!"))?;

                    (*entry).state = VideoStatus::Live;

                    updates.push(StreamUpdate::Started(entry.clone()));
                }
                VideoUpdate::Ended(id) => {
                    let entry = index
                        .get_mut(&id)
                        .ok_or_else(|| anyhow!("Entry wasn't in index!"))?;

                    (*entry).state = VideoStatus::Past;

                    updates.push(StreamUpdate::Ended(entry.clone()));
                }
                VideoUpdate::Unscheduled(id) => {
                    if let Some(entry) = index.remove(&id) {
                        updates.push(StreamUpdate::Unscheduled(entry));
                    }
                }
                VideoUpdate::Renamed { id, new_name } => {
                    let entry = index
                        .get_mut(&id)
                        .ok_or_else(|| anyhow!("Entry wasn't in index!"))?;

                    (*entry).title = new_name;
                }
                VideoUpdate::Rescheduled { id, new_start } => {
                    let entry = index
                        .get_mut(&id)
                        .ok_or_else(|| anyhow!("Entry wasn't in index!"))?;

                    (*entry).start_at = new_start;
                }
            }
        }

        Ok(updates)
    }

    #[instrument(skip(client, streams, index))]
    async fn check_stream_updates(
        client: &Client,
        streams: &[String],
        index: &HashMap<String, Livestream>,
    ) -> anyhow::Result<Vec<VideoUpdate>> {
        let parameter = streams.join(",");

        let res = client
            .get("https://holodex.net/api/v2/videos")
            .query(&[
                ("id", parameter.as_str()),
                ("status", "upcoming,live,past,missing,new"),
            ])
            .send()
            .await
            .context(here!())?;

        let now = Utc::now();

        let streams: Vec<Video> = validate_response(res).await?;

        debug!(
            count = streams.len(),
            "Fetched updated live and pending stream data."
        );

        let mut updates = Vec::with_capacity(streams.len());

        for stream in streams {
            let entry = match index.get(&stream.id) {
                Some(l) => l,
                None => {
                    if (stream.available_at - now).num_hours() < 48 {
                        warn!(?stream, "Couldn't find stream in index.");
                    }

                    continue;
                }
            };

            if entry.title != stream.title && !stream.title.is_empty() {
                info!(before = %entry.title, after = %stream.title, "Video renamed!");
                updates.push(VideoUpdate::Renamed {
                    id: entry.id.clone(),
                    new_name: stream.title.clone(),
                });
            }

            if entry.state == VideoStatus::Upcoming
                && entry.start_at
                    != stream
                        .live_info
                        .start_scheduled
                        .unwrap_or(stream.available_at)
            {
                info!(
                    before = ?entry.start_at,
                    after = ?stream.live_info.start_scheduled.unwrap_or(stream.available_at),
                    video = %stream.title,
                    "Video rescheduled!"
                );

                updates.push(VideoUpdate::Rescheduled {
                    id: entry.id.clone(),
                    new_start: stream
                        .live_info
                        .start_scheduled
                        .unwrap_or(stream.available_at),
                });
            }

            updates.push(match (entry.state, stream.status) {
                (VideoStatus::Missing | VideoStatus::New, VideoStatus::Upcoming) => {
                    debug!(video = %stream.title, "Video scheduled!");
                    VideoUpdate::Scheduled(entry.id.clone())
                }
                (VideoStatus::Upcoming | VideoStatus::Missing, VideoStatus::Live) => {
                    debug!(video = %stream.title, "Video started!");
                    VideoUpdate::Started(entry.id.clone())
                }
                (VideoStatus::Live, VideoStatus::Past | VideoStatus::Missing) => {
                    info!(video = %stream.title, "Video ended!");
                    VideoUpdate::Ended(entry.id.clone())
                }
                (VideoStatus::Upcoming, VideoStatus::Missing) => {
                    info!(video = %stream.title, "Video unscheduled!");
                    VideoUpdate::Unscheduled(entry.id.clone())
                }
                // Compensate for cache delay in Holodex.
                (VideoStatus::Live, VideoStatus::Upcoming)
                    if (now - stream.available_at) < chrono::Duration::minutes(5) =>
                {
                    continue
                }
                _ if entry.state != stream.status => {
                    warn!(before = ?entry.state, after = ?stream.status,
                        video = %stream.title, "Unknown status transition!");
                    continue;
                }
                _ => continue,
            });
        }

        Ok(updates)
    }

    #[instrument(skip(client, index_lock, user_map))]
    async fn fetch_new_streams(
        client: &Client,
        index_lock: StreamIndex,
        parameters: &ApiLiveOptions,
        user_map: &HashMap<String, User>,
    ) -> anyhow::Result<Vec<(String, Livestream)>> {
        let mut offset = 0;
        let mut new_streams = Vec::with_capacity(100);

        // Fetch 100 streams at a time until reaching already cached streams.
        loop {
            let stream_index = index_lock.lock().await;

            let stream_batch = try_run(|| async {
                Self::get_streams(
                    client,
                    &ApiLiveOptions {
                        offset,
                        ..parameters.clone()
                    },
                    user_map,
                )
                .await
            })
            .await?;

            trace!("Received stream batch.");

            let mut reached_indexed_data = false;

            for (id, stream) in stream_batch {
                if stream_index.contains_key(&id) {
                    reached_indexed_data = true;
                    continue;
                }

                new_streams.push((id, stream));
            }

            if reached_indexed_data {
                break;
            } else {
                offset += parameters.limit as i32;
            }
        }

        Ok(new_streams)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_test::traced_test;

    const MOCK_SERVER: &str = "https://stoplight.io/mocks/holodex/holodex:main/11620234";
    const USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"),);

    #[tokio::test]
    #[traced_test]
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
    #[traced_test]
    async fn check_channel_update() {
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

    #[allow(unused_attributes)]
    #[tokio::test]
    #[traced_test]
    #[ignore]
    async fn check_stream_updates() {
        let client = reqwest::ClientBuilder::new()
            .user_agent(USER_AGENT)
            .build()
            .context(here!())
            .unwrap();

        let res = client
            .get(format!("{}/videos", MOCK_SERVER))
            .query(&[("id", "wSmNK842gs8,tftbi551s8Q"), ("status", "live,past")])
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
