pub mod birthday_reminder;
pub mod config;
pub mod discord_bot;

pub mod apis {
    pub mod discord_api;
    pub mod holo_api;
    pub mod translation_api;
    pub mod twitter_api;
}

pub mod utility {
    pub mod extensions;
    pub mod logger;
    pub mod macros;
    pub mod serializers;
}

use log::error;
use tokio::sync::mpsc::{self, Receiver, Sender};

use crate::apis::{
    discord_api::{DiscordAPI, DiscordMessageData},
    holo_api::HoloAPI,
    twitter_api::TwitterAPI,
};
use crate::birthday_reminder::BirthdayReminder;
use crate::config::Config;
use crate::discord_bot::DiscordBot;
use crate::utility::logger::Logger;

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
