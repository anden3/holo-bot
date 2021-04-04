use std::{collections::HashMap, time::Duration};
use std::{error::Error, sync::Arc};

use chrono::prelude::*;
use log::{debug, error, info};
use once_cell::sync::OnceCell;
use serde::{self, Deserialize};
use tokio::sync::RwLock;
use tokio::{sync::mpsc::Sender, time::sleep};

use crate::{
    apis::discord_api::DiscordMessageData,
    config::{Config, User},
};

static STREAM_INDEX: OnceCell<Arc<RwLock<HashMap<u32, Livestream>>>> = OnceCell::new();

pub struct HoloAPI {}

impl HoloAPI {
    pub async fn start(config: Config, notifier_sender: Sender<DiscordMessageData>) {
        let stream_index = Arc::new(RwLock::new(HashMap::new()));

        let producer_lock = stream_index.clone();
        let notifier_lock = stream_index.clone();

        tokio::spawn(async move {
            HoloAPI::stream_producer(config, producer_lock).await;
        });

        tokio::spawn(async move {
            HoloAPI::stream_notifier(notifier_lock, notifier_sender).await;
        });

        STREAM_INDEX.get_or_init(|| stream_index);
    }

    pub fn get_stream_index_lock() -> Option<Arc<RwLock<HashMap<u32, Livestream>>>> {
        STREAM_INDEX.get().and_then(|a| Some(a.clone()))
    }

    pub fn read_stream_index() -> Option<&'static Arc<RwLock<HashMap<u32, Livestream>>>> {
        STREAM_INDEX.get()
    }

    pub async fn get_indexed_streams(stream_state: StreamState) -> Vec<Livestream> {
        match STREAM_INDEX.get() {
            Some(index) => {
                let index = index.read().await;
                index
                    .iter()
                    .filter_map(|(_, s)| {
                        if s.state == stream_state {
                            Some(s)
                        } else {
                            None
                        }
                    })
                    .cloned()
                    .collect()
            }
            None => vec![],
        }
    }

    async fn stream_producer(config: Config, producer_lock: Arc<RwLock<HashMap<u32, Livestream>>>) {
        let client = reqwest::ClientBuilder::new()
            .user_agent(concat!(
                env!("CARGO_PKG_NAME"),
                "/",
                env!("CARGO_PKG_VERSION"),
            ))
            .build()
            .expect("Failed to build client.");

        loop {
            let scheduled_streams = HoloAPI::get_streams(StreamState::Scheduled, &client, &config)
                .await
                .unwrap();

            let live_streams = HoloAPI::get_streams(StreamState::Live, &client, &config)
                .await
                .unwrap();

            let ended_streams = HoloAPI::get_streams(StreamState::Ended, &client, &config)
                .await
                .unwrap();

            let mut stream_index = producer_lock.write().await;
            let mut new_index = HashMap::with_capacity(stream_index.capacity());

            new_index.extend(scheduled_streams.into_iter());
            new_index.extend(live_streams.into_iter());

            new_index.retain(|i, _| !ended_streams.contains_key(i));

            for stream_id in stream_index.keys() {
                let indexed = stream_index.get(&stream_id).unwrap();

                if !new_index.contains_key(&stream_id) {
                    if (indexed.start_at - Utc::now()).num_minutes() < 5 {
                        error!("Stream not in API despite starting in less than 5 minutes!");
                    }
                }
            }

            *stream_index = new_index;
            std::mem::drop(stream_index);

            sleep(Duration::from_secs(60)).await;
        }
    }

    async fn stream_notifier(
        notifier_lock: Arc<RwLock<HashMap<u32, Livestream>>>,
        tx: Sender<DiscordMessageData>,
    ) {
        loop {
            let mut sleep_duration = Duration::from_secs(60);
            let mut stream_index = notifier_lock.write().await;

            loop {
                if let Some(closest_stream) = stream_index
                    .values_mut()
                    .filter(|s| s.state == StreamState::Scheduled)
                    .min_by_key(|s| s.start_at)
                {
                    let remaining_time = closest_stream.start_at - Utc::now();

                    debug!(
                        "Next stream {}.",
                        chrono_humanize::HumanTime::from(remaining_time).to_text_en(
                            chrono_humanize::Accuracy::Precise,
                            chrono_humanize::Tense::Future
                        )
                    );

                    if remaining_time.num_seconds() < 10 {
                        info!(
                            "Time to watch {} playing {} at https://youtube.com/watch?v={}!",
                            closest_stream.streamer, closest_stream.title, closest_stream.url
                        );

                        tx.send(DiscordMessageData::ScheduledLive(closest_stream.clone()))
                            .await
                            .unwrap();

                        closest_stream.state = StreamState::Live;
                    } else if sleep_duration >= remaining_time.to_std().unwrap() {
                        sleep_duration = remaining_time.to_std().unwrap();
                        break;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }

            sleep(sleep_duration).await;
        }
    }

    async fn get_streams(
        state: StreamState,
        client: &reqwest::Client,
        config: &Config,
    ) -> Result<HashMap<u32, Livestream>, Box<dyn Error>> {
        let endpoint = match state {
            StreamState::Scheduled => "https://holo.dev/api/v1/lives/scheduled",
            StreamState::Live => "https://holo.dev/api/v1/lives/live",
            StreamState::Ended => "https://holo.dev/api/v1/lives/ended",
        };

        let res = client.post(endpoint).send().await?;
        let res: APIResponse = res.json().await?;

        Ok(res
            .lives
            .into_iter()
            .map(|s| {
                let channel = s.channel.clone();

                (
                    s.id,
                    Livestream {
                        id: s.id,
                        title: s.title,
                        thumbnail: s.thumbnail,
                        created_at: s.created_at,
                        start_at: s.start_at,
                        duration: s.duration,
                        url: s.url,
                        streamer: config
                            .users
                            .iter()
                            .find(|u| u.channel == channel)
                            .unwrap()
                            .clone(),
                        state,
                    },
                )
            })
            .collect::<HashMap<_, _>>())
    }
}

#[derive(Debug, Clone)]
pub struct Livestream {
    pub id: u32,
    pub title: String,
    pub thumbnail: String,
    pub url: String,
    pub streamer: User,

    pub created_at: DateTime<Utc>,
    pub start_at: DateTime<Utc>,

    pub duration: Option<u32>,
    pub state: StreamState,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum StreamState {
    Scheduled,
    Live,
    Ended,
}

#[derive(Deserialize, Debug)]
struct APIResponse {
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

    #[serde(with = "crate::utility::serializers::utc_datetime")]
    created_at: DateTime<Utc>,
    #[serde(with = "crate::utility::serializers::utc_datetime")]
    start_at: DateTime<Utc>,

    duration: Option<u32>,

    #[serde(skip)]
    video: String,
}

impl PartialEq for Livestream {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }

    fn ne(&self, other: &Self) -> bool {
        self.id != other.id
    }
}
