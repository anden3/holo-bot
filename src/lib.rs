#[path = "config.rs"]
mod config;
#[path = "discord_api.rs"]
mod discord_api;
#[path = "holo_api.rs"]
mod holo_api;
#[path = "twitter_api.rs"]
mod twitter_api;

use config::Config;
// use futures::StreamExt;
use holo_api::ScheduledLive;
use reqwest::Error;
use serenity::model::{
    id::{ChannelId, RoleId},
    misc::Mention,
};
use tokio::sync::mpsc::{self, Receiver, Sender};

pub struct HoloBot {}

impl HoloBot {
    pub async fn start() -> Result<(), Error> {
        let config = Config::load_config("settings.json");
        let discord = discord_api::DiscordAPI::new(&config.discord_token).await;

        let (tx, rx): (Sender<DiscordMessageData>, Receiver<DiscordMessageData>) =
            mpsc::channel(10);

        holo_api::HoloAPI::start(tx.clone()).await;

        tokio::spawn(async move {
            HoloBot::discord_poster(discord, rx, config.clone()).await;
        });

        loop {}

        /*
        let twitter = twitter_api::TwitterAPI::new(&config.bearer_token);

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

    async fn discord_poster(
        discord: discord_api::DiscordAPI,
        mut channel: Receiver<DiscordMessageData>,
        config: Config,
    ) {
        let livestream_channel = ChannelId(config.live_notif_channel);

        loop {
            if let Some(msg) = channel.recv().await {
                match msg {
                    DiscordMessageData::ScheduledLive(live) => {
                        if let Some(user) = config.users.iter().find(|u| u.name == live.streamer) {
                            let role: RoleId = user.discord_role.into();

                            discord
                                .send_message(livestream_channel, |m| {
                                    m.allowed_mentions(|am| {
                                        am.empty_parse();
                                        am.roles(vec![role]);

                                        am
                                    });

                                    m.embed(|e| {
                                        e.title(format!("{} just went live!", user.display_name));
                                        e.description(format!(
                                            "{} {}",
                                            Mention::from(role),
                                            live.title
                                        ));
                                        e.url(format!("https://youtube.com/watch?v={}", live.url));
                                        e.timestamp(&live.start_at);
                                        e.colour(u32::from(user.colour));
                                        e.image(format!(
                                            "https://img.youtube.com/vi/{}/hqdefault.jpg",
                                            live.url
                                        ));
                                        e.author(|a| {
                                            a.name(&user.display_name);
                                            a.url(format!(
                                                "https://www.youtube.com/channel/{}",
                                                user.channel
                                            ));
                                            a.icon_url(&user.icon);

                                            a
                                        });
                                        e.footer(|f| {
                                            f.text("Provided by HoloBot (created by anden3)");

                                            f
                                        });

                                        e
                                    });

                                    m
                                })
                                .await;
                        }
                    }
                }
            }
        }
    }
}

#[derive(Debug)]
pub enum DiscordMessageData {
    ScheduledLive(ScheduledLive),
}
