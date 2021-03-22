#[path = "birthday_reminder.rs"]
mod birthday_reminder;
#[path = "config.rs"]
mod config;
#[path = "discord_api.rs"]
mod discord_api;
#[path = "extensions.rs"]
mod extensions;
#[path = "holo_api.rs"]
mod holo_api;
#[path = "serializers.rs"]
mod serializers;
#[path = "twitter_api.rs"]
mod twitter_api;

use tokio::sync::mpsc::{self, Receiver, Sender};

use birthday_reminder::BirthdayReminder;
use config::Config;
use discord_api::{DiscordAPI, DiscordMessageData};
use holo_api::HoloAPI;
use twitter_api::TwitterAPI;

pub struct HoloBot {}

impl HoloBot {
    pub async fn start() {
        let config = Config::load_config("settings.json");
        let discord = DiscordAPI::new(&config.discord_token).await;

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
                eprintln!("{}: {:?}", msg, e);
                None
            }
        }
    }
}
