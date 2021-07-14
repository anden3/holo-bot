use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use anyhow::{anyhow, Context};
use backoff::ExponentialBackoff;
use chrono::prelude::*;
use reqwest::Client;
use serde::{self, Deserialize};
use tokio::{
    sync::{broadcast, mpsc, watch, Mutex},
    time::sleep,
};
use tracing::{debug, debug_span, error, info, instrument, warn, Instrument};

use utility::{
    config::Config,
    here,
    streams::{Livestream, StreamState, StreamUpdate},
};

use crate::discord_api::DiscordMessageData;

type StreamIndex = Arc<Mutex<HashMap<u32, Livestream>>>;
type NotifiedStreams = Arc<Mutex<HashSet<String>>>;

pub struct HoloApi;

impl HoloApi {
    #[instrument(skip(config, live_sender, update_sender, exit_receiver))]
    pub async fn start(
        config: Config,
        live_sender: mpsc::Sender<DiscordMessageData>,
        update_sender: broadcast::Sender<StreamUpdate>,
        mut exit_receiver: watch::Receiver<bool>,
    ) -> watch::Receiver<HashMap<u32, Livestream>> {
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

    async fn try_get_streams(
        client: &Client,
        config: &Config,
    ) -> anyhow::Result<[HashMap<u32, Livestream>; 3]> {
        let backoff_config = ExponentialBackoff {
            initial_interval: Duration::from_secs(4),
            max_interval: Duration::from_secs(64 * 60),
            randomization_factor: 0.0,
            multiplier: 2.0,
            ..ExponentialBackoff::default()
        };

        Ok(backoff::future::retry(backoff_config, || async {
            let mut result: [HashMap<u32, Livestream>; 3] = Default::default();

            for i in 0..3 {
                let state = match i {
                    0 => StreamState::Scheduled,
                    1 => StreamState::Live,
                    2 => StreamState::Ended,
                    _ => unreachable!(),
                };

                result[i] = HoloApi::get_streams(state, &client, &config)
                    .await
                    .map_err(|e| {
                        warn!("{}", e.to_string());
                        anyhow!(e).context(here!())
                    })?;
            }

            Ok(result)
        })
        .await
        .context(here!())?)
    }

    #[instrument(skip(config, producer_lock, notified_streams, index_sender, stream_updates))]
    async fn stream_producer(
        config: Config,
        producer_lock: StreamIndex,
        notified_streams: NotifiedStreams,
        index_sender: watch::Sender<HashMap<u32, Livestream>>,
        stream_updates: broadcast::Sender<StreamUpdate>,
    ) -> anyhow::Result<()> {
        let client = reqwest::ClientBuilder::new()
            .user_agent(concat!(
                env!("CARGO_PKG_NAME"),
                "/",
                env!("CARGO_PKG_VERSION"),
            ))
            .build()
            .context(here!())?;

        loop {
            let [scheduled_streams, live_streams, ended_streams] =
                Self::try_get_streams(&client, &config).await?;

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
            debug!("Stream index update finished!");

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
                        && (s.state == StreamState::Scheduled
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
                .map(|(id, _)| *id)
                .collect::<Vec<_>>();

            for id in live_ids {
                if let Some(s) = stream_index.get_mut(&id) {
                    s.state = StreamState::Live;
                }
            }

            std::mem::drop(stream_index);

            sleep(sleep_duration).await;
        }
    }

    #[instrument(skip(client, config))]
    async fn get_streams(
        state: StreamState,
        client: &reqwest::Client,
        config: &Config,
    ) -> anyhow::Result<HashMap<u32, Livestream>> {
        let endpoint = match state {
            StreamState::Scheduled => "https://holo.dev/api/v1/lives/scheduled",
            StreamState::Live => "https://holo.dev/api/v1/lives/current",
            StreamState::Ended => "https://holo.dev/api/v1/lives/ended",
        };

        let res = client.get(endpoint).send().await.context(here!())?;
        let res: ApiResponse = res.json().await.context(here!())?;

        Ok(res
            .lives
            .into_iter()
            .filter_map(|s| {
                let channel = s.channel.clone();
                let streamer = config.users.iter().find(|u| u.channel == channel)?.clone();

                Some((
                    s.id,
                    Livestream {
                        id: s.id,
                        title: s.title,
                        thumbnail: s.thumbnail,
                        created_at: s.created_at,
                        start_at: s.start_at,
                        duration: s.duration,
                        url: s.url,
                        streamer,
                        state,
                    },
                ))
            })
            .collect::<HashMap<_, _>>())
    }
}

#[derive(Deserialize, Debug)]
struct ApiResponse {
    #[serde(default = "Vec::new")]
    lives: Vec<LivestreamResponse>,
    total: u32,
}

#[derive(Deserialize, Debug)]
struct LivestreamResponse {
    id: u32,
    title: String,
    #[serde(rename = "cover")]
    thumbnail: String,
    #[serde(rename = "room")]
    url: String,

    channel_id: u32,
    platform: String,
    channel: String,

    #[serde(with = "utility::serializers::utc_datetime")]
    created_at: DateTime<Utc>,
    #[serde(with = "utility::serializers::utc_datetime")]
    start_at: DateTime<Utc>,

    duration: Option<u32>,

    #[serde(skip)]
    video: String,
}
