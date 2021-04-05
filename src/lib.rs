pub mod birthday_reminder;
pub mod commands;
pub mod config;
pub mod discord_bot;

pub mod apis {
    pub mod discord_api;
    pub mod holo_api;
    pub mod meme_api;
    pub mod translation_api;
    pub mod twitter_api;
}

pub mod utility {
    pub mod extensions;
    pub mod logger;
    pub mod macros;
    pub mod serializers;
}

use apis::holo_api::StreamUpdate;
use log::error;
use tokio::sync::mpsc::{self, Receiver, Sender, UnboundedReceiver, UnboundedSender};

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

        let (discord_message_tx, discord_message_rx): (
            Sender<DiscordMessageData>,
            Receiver<DiscordMessageData>,
        ) = mpsc::channel(10);

        let (stream_update_tx, stream_update_rx): (
            UnboundedSender<StreamUpdate>,
            UnboundedReceiver<StreamUpdate>,
        ) = mpsc::unbounded_channel();

        HoloAPI::start(config.clone(), discord_message_tx.clone(), stream_update_tx).await;
        TwitterAPI::start(config.clone(), discord_message_tx.clone()).await;
        BirthdayReminder::start(config.clone(), discord_message_tx.clone()).await;

        let discord_cache = DiscordBot::start(config.clone()).await;

        DiscordAPI::start(
            discord_cache,
            discord_message_rx,
            stream_update_rx,
            config.clone(),
        )
        .await;

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
