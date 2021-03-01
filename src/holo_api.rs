use std::time::Duration;
use std::{error::Error, sync::Arc};

use chrono::prelude::*;
use graphql_client::{GraphQLQuery, Response};
use tokio::sync::RwLock;
use tokio::{sync::mpsc::Sender, time::sleep};

use super::DiscordMessageData;

type ISO8601DateTime = String;

pub struct HoloAPI {}

impl HoloAPI {
    pub async fn start(notifier_sender: Sender<DiscordMessageData>) {
        let stream_index = Arc::new(RwLock::new(Vec::new()));

        let producer_lock = stream_index.clone();
        let notifier_lock = stream_index.clone();

        tokio::spawn(async move {
            HoloAPI::stream_producer(producer_lock).await;
        });

        tokio::spawn(async move {
            HoloAPI::stream_notifier(notifier_lock, notifier_sender).await;
        });
    }

    async fn stream_producer(producer_lock: Arc<RwLock<Vec<ScheduledLive>>>) {
        let client = reqwest::ClientBuilder::new()
            .user_agent(concat!(
                env!("CARGO_PKG_NAME"),
                "/",
                env!("CARGO_PKG_VERSION"),
            ))
            .build()
            .expect("[HOLO.DEV] Failed to build client.");

        println!("[HOLO.DEV] Client ready!");

        loop {
            let mut scheduled_streams =
                HoloAPI::get_scheduled_streams(&client, get_scheduled_lives::Variables {})
                    .await
                    .unwrap();

            let mut stream_index = producer_lock.write().await;
            let mut i = 0;
            while i != stream_index.len() {
                let live = &mut stream_index[i];

                if let Some(pos) = scheduled_streams.iter().position(|s| s.title == live.title) {
                    let stream = scheduled_streams.remove(pos);

                    if *live != stream {
                        (*live).clone_from(&stream);
                    }

                    i += 1;
                } else {
                    if (live.start_at - Utc::now()).num_minutes() < 5 {
                        eprintln!("Stream not in API despite starting in less than 5 minutes!");
                    }

                    stream_index.remove(i);
                }
            }

            stream_index.append(&mut scheduled_streams);

            sleep(Duration::from_secs(6)).await;
        }
    }

    async fn stream_notifier(
        notifier_lock: Arc<RwLock<Vec<ScheduledLive>>>,
        tx: Sender<DiscordMessageData>,
    ) {
        loop {
            let mut sleep_duration = Duration::from_secs(60);
            let mut stream_index = notifier_lock.write().await;

            loop {
                if let Some(closest_stream) = stream_index.iter().min_by_key(|s| s.start_at) {
                    let remaining_time = closest_stream.start_at - Utc::now();

                    println!(
                        "[HOLO.DEV] Next stream {}.",
                        chrono_humanize::HumanTime::from(remaining_time).to_text_en(
                            chrono_humanize::Accuracy::Precise,
                            chrono_humanize::Tense::Future
                        )
                    );

                    if remaining_time.num_seconds() < 10 {
                        println!(
                            "[HOLO.DEV] Time to watch {} playing {} at https://youtube.com/watch?v={}!",
                            closest_stream.streamer, closest_stream.title, closest_stream.url
                        );

                        tx.send(DiscordMessageData::ScheduledLive(closest_stream.clone()))
                            .await
                            .unwrap();

                        let pos = stream_index
                            .iter()
                            .position(|s| s == closest_stream)
                            .unwrap();

                        stream_index.remove(pos);
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

    async fn get_scheduled_streams(
        client: &reqwest::Client,
        variables: get_scheduled_lives::Variables,
    ) -> Result<Vec<ScheduledLive>, Box<dyn Error>> {
        let request_body = GetScheduledLives::build_query(variables);

        let res = client
            .post("https://holo.dev/graphql")
            .json(&request_body)
            .send()
            .await
            .unwrap();

        let response_body: Response<get_scheduled_lives::ResponseData> = res.json().await.unwrap();

        if let Some(errors) = &response_body.errors {
            for err in errors {
                eprintln!("{}", err);
            }
        }
        let mut scheduled_lives: Vec<ScheduledLive> = Vec::new();

        for live in response_body.data.unwrap().lives.nodes.unwrap() {
            let live_data = live.unwrap();

            if live_data.room.platform.platform != "youtube" {
                continue;
            }

            scheduled_lives.push(ScheduledLive {
                title: live_data.title,
                url: live_data.room.room,
                streamer: live_data.channel.member.unwrap().name,
                start_at: DateTime::from(
                    DateTime::parse_from_rfc3339(&live_data.start_at)
                        .expect("[HOLO.DEV] Couldn't parse start time!"),
                ),
            });
        }

        Ok(scheduled_lives)
    }
}

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "queries/schema.json",
    query_path = "queries/get_scheduled_lives.graphql",
    response_derives = "Debug"
)]
pub struct GetScheduledLives;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledLive {
    pub title: String,
    pub url: String,
    pub streamer: String,
    pub start_at: DateTime<Utc>,
}
