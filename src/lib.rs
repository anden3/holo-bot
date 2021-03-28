#[path = "birthday_reminder.rs"]
mod birthday_reminder;
#[path = "config.rs"]
mod config;
#[path = "apis/discord_api.rs"]
mod discord_api;
#[path = "discord_bot.rs"]
mod discord_bot;
#[path = "utility/extensions.rs"]
mod extensions;
#[path = "apis/holo_api.rs"]
mod holo_api;
#[path = "utility/logger.rs"]
mod logger;
#[path = "utility/serializers.rs"]
mod serializers;
#[path = "apis/translation_api.rs"]
mod translation_api;
#[path = "apis/twitter_api.rs"]
mod twitter_api;

use tokio::sync::mpsc::{self, Receiver, Sender};

use birthday_reminder::BirthdayReminder;
use config::Config;
use discord_api::{DiscordAPI, DiscordMessageData};
use discord_bot::DiscordBot;
use holo_api::HoloAPI;
use log::error;
use logger::Logger;
use twitter_api::TwitterAPI;

pub struct HoloBot {}

impl HoloBot {
    pub async fn start() {
        Logger::initialize().expect("Setting up logging failed!");

        let config = Config::load_config("settings.json");
        let discord_cache = DiscordBot::start(config.clone()).await;

        let discord = DiscordAPI {
            cache_and_http: discord_cache,
        };

        let (tx, rx): (Sender<DiscordMessageData>, Receiver<DiscordMessageData>) =
            mpsc::channel(10);

        HoloAPI::start(tx.clone()).await;
        TwitterAPI::start(config.clone(), tx.clone()).await;
        BirthdayReminder::start(config.clone(), tx.clone()).await;

        tokio::spawn(async move {
            DiscordAPI::posting_thread(discord, rx, config.clone()).await;
        });

        loop {}
    }
}

pub trait ResultOkPrintErrExt<T> {
    fn ok_or_print_err(self, msg: &str) -> Option<T>;
}

impl<T, E> ResultOkPrintErrExt<T> for Result<T, E>
where
    E: ::std::fmt::Debug,
{
    fn ok_or_print_err(self, msg: &str) -> Option<T> {
        match self {
            Ok(v) => Some(v),
            Err(e) => {
                error!("{}: {:?}", msg, e);
                None
            }
        }
    }
}
