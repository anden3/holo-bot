#[path = "config.rs"]
mod config;
#[path = "discord_api.rs"]
mod discord_api;
#[path = "holo_api.rs"]
mod holo_api;
#[path = "twitter_api.rs"]
mod twitter_api;

use config::Config;
use futures::StreamExt;
use holo_api::ScheduledLive;
use reqwest::Error;
use std::sync::mpsc::{self, Receiver, Sender};

pub struct HoloBot {
    config: Config,
}

impl HoloBot {
    pub async fn new() -> Self {
        let config = Config::load_config("settings.json");

        return HoloBot { config };
    }

    pub async fn start(&self) -> Result<(), Error> {
        let twitter = twitter_api::TwitterAPI::new(&self.config.bearer_token);
        let mut discord = discord_api::DiscordAPI::new(&self.config.discord_token).await;

        let (tx, rx): (Sender<ScheduledLive>, Receiver<ScheduledLive>) = mpsc::channel();

        // discord.connect().await;

        holo_api::HoloAPI::start(tx.clone());

        loop {
            if let Ok(msg) = rx.try_recv() {
                println!("{:#?}", msg);
                break;
            }
        }

        /*

        twitter.setup_rules(&self.config.users).await.unwrap();
        let mut stream = twitter.connect().await.unwrap();

        while let Some(item) = stream.next().await {
            let response = item.unwrap();

            if response == "\r\n" {
                continue;
            }

            let response: serde_json::Value =
                serde_json::from_slice(&response).expect("Deserialization of response failed.");

            println!("Response: {:#?}", response);
        }
        */

        Ok(())
    }
}
