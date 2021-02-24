use chrono::prelude::*;
use graphql_client::{GraphQLQuery, Response};
use std::error::Error;
use std::sync::mpsc::Sender;
use std::sync::{Arc, RwLock};
use std::thread;
use std::thread::sleep;
use std::time::Duration;

type ISO8601DateTime = String;

pub struct HoloAPI {}

impl HoloAPI {
    pub fn start(notifier_sender: Sender<ScheduledLive>) {
        let stream_index = Arc::new(RwLock::new(Vec::new()));

        let producer_lock = stream_index.clone();
        let notifier_lock = stream_index.clone();

        let producer_thread = thread::Builder::new()
            .name("Producer thread".to_string())
            .spawn(move || HoloAPI::stream_producer(producer_lock))
            .unwrap();

        let notifier_thread = thread::Builder::new()
            .name("Notifier thread".to_string())
            .spawn(move || HoloAPI::stream_notifier(notifier_lock, notifier_sender))
            .unwrap();

        // producer_thread.join().unwrap();
        // notifier_thread.join().unwrap();
    }

    fn stream_producer(producer_lock: Arc<RwLock<Vec<ScheduledLive>>>) {
        let client = reqwest::blocking::ClientBuilder::new()
            .user_agent(concat!(
                env!("CARGO_PKG_NAME"),
                "/",
                env!("CARGO_PKG_VERSION"),
            ))
            .build()
            .expect("Failed to build client.");

        println!("Client ready!");

        loop {
            let mut scheduled_streams =
                HoloAPI::get_scheduled_streams(&client, get_scheduled_lives::Variables {}).unwrap();
            println!("Getting streams...");

            if let Ok(mut stream_index) = producer_lock.write() {
                println!("Updating stream index...");

                for i in 0..stream_index.len() {
                    let live = &mut stream_index[i];

                    if let Some(pos) = scheduled_streams.iter().position(|s| s.title == live.title)
                    {
                        let stream = scheduled_streams.remove(pos);

                        if *live != stream {
                            (*live).clone_from(&stream);
                        }
                    } else {
                        stream_index.remove(i);
                    }
                }

                stream_index.append(&mut scheduled_streams);
            }

            sleep(Duration::from_secs(6));
        }
    }

    fn stream_notifier(notifier_lock: Arc<RwLock<Vec<ScheduledLive>>>, tx: Sender<ScheduledLive>) {
        loop {
            let mut sleep_duration = Duration::from_secs(60);
            let mut remove_streams = false;

            if let Ok(stream_index) = notifier_lock.read() {
                loop {
                    if let Some(closest_stream) = stream_index.iter().min_by_key(|s| s.start_at) {
                        let now = Utc::now();
                        let remaining_time = closest_stream.start_at - now;

                        println!(
                            "Closest stream is {} playing {} at {} {}.",
                            closest_stream.streamer,
                            closest_stream.title,
                            closest_stream.url,
                            chrono_humanize::HumanTime::from(remaining_time).to_text_en(
                                chrono_humanize::Accuracy::Precise,
                                chrono_humanize::Tense::Future
                            )
                        );

                        if remaining_time.num_seconds() < 10 {
                            println!(
                                "Time to watch {} playing {} at {}.",
                                closest_stream.streamer, closest_stream.title, closest_stream.url
                            );

                            tx.send(closest_stream.clone()).unwrap();
                            remove_streams = true;
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
            }

            // Remove started streams.
            if remove_streams {
                if let Ok(mut stream_index) = notifier_lock.write() {
                    let now = Utc::now();

                    for i in 0..stream_index.len() {
                        if stream_index[i].start_at < now {
                            stream_index.remove(i);
                        }
                    }
                }
            }

            sleep(sleep_duration);
        }
    }

    fn get_scheduled_streams(
        client: &reqwest::blocking::Client,
        variables: get_scheduled_lives::Variables,
    ) -> Result<Vec<ScheduledLive>, Box<dyn Error>> {
        let request_body = GetScheduledLives::build_query(variables);

        let res = client
            .post("https://holo.dev/graphql")
            .json(&request_body)
            .send()
            .unwrap();

        let response_body: Response<get_scheduled_lives::ResponseData> = res.json().unwrap();

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
                url: format!("https://youtube.com/watch?v={}", live_data.room.room),
                streamer: live_data.channel.member.unwrap().name,
                start_at: DateTime::from(
                    DateTime::parse_from_rfc3339(&live_data.start_at)
                        .expect("Couldn't parse start time!"),
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
    title: String,
    url: String,
    streamer: String,
    start_at: DateTime<Utc>,
}
