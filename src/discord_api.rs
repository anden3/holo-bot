use std::sync::Arc;

use super::birthday_reminder::Birthday;
use super::config::Config;
use super::holo_api::ScheduledLive;
use super::twitter_api::ScheduleUpdate;

use serenity::{
    builder::CreateMessage,
    model::{
        id::{ChannelId, RoleId},
        misc::Mention,
    },
    prelude::*,
    CacheAndHttp,
};
use tokio::sync::mpsc::Receiver;

pub struct DiscordAPI {
    pub cache_and_http: Arc<CacheAndHttp>,
}

impl DiscordAPI {
    pub async fn new(discord_token: &str) -> DiscordAPI {
        let client = Client::builder(discord_token)
            .await
            .expect("[DISCORD] Client creation failed");

        return DiscordAPI {
            cache_and_http: client.cache_and_http.clone(),
        };
    }

    pub async fn send_message<'a, F>(&self, channel: ChannelId, f: F)
    where
        for<'b> F: FnOnce(&'b mut CreateMessage<'a>) -> &'b mut CreateMessage<'a>,
    {
        let _ = channel
            .send_message(&self.cache_and_http.http, f)
            .await
            .unwrap();
    }

    pub async fn posting_thread(
        discord: DiscordAPI,
        mut channel: Receiver<DiscordMessageData>,
        config: Config,
    ) {
        loop {
            if let Some(msg) = channel.recv().await {
                match msg {
                    DiscordMessageData::ScheduledLive(live) => {
                        if let Some(user) = config.users.iter().find(|u| u.name == live.streamer) {
                            let livestream_channel = ChannelId(config.live_notif_channel);
                            let role: RoleId = user.discord_role.into();

                            discord
                                .send_message(livestream_channel, |m| {
                                    m.content(Mention::from(role));

                                    m.allowed_mentions(|am| {
                                        am.empty_parse();
                                        am.roles(vec![role]);

                                        am
                                    });

                                    m.embed(|e| {
                                        e.title(format!("{} just went live!", user.display_name));
                                        e.description(live.title);
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
                    DiscordMessageData::ScheduleUpdate(update) => {
                        if let Some(user) = config
                            .users
                            .iter()
                            .find(|u| u.twitter_id == update.twitter_id)
                        {
                            let schedule_channel = ChannelId(config.schedule_channel);
                            let role: RoleId = user.discord_role.into();

                            discord
                                .send_message(schedule_channel, |m| {
                                    m.content(Mention::from(role));

                                    m.allowed_mentions(|am| {
                                        am.empty_parse();
                                        am.roles(vec![role]);

                                        am
                                    });

                                    m.embed(|e| {
                                        e.title(format!(
                                            "{} just released a schedule update!",
                                            user.display_name
                                        ));
                                        e.description(update.tweet_text);
                                        e.url(update.tweet_link);
                                        e.timestamp(&update.timestamp);
                                        e.colour(u32::from(user.colour));
                                        e.image(update.schedule_image);
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
                    DiscordMessageData::Birthday(_) => {}
                }
            }
        }
    }
}

#[derive(Debug)]
pub enum DiscordMessageData {
    ScheduledLive(ScheduledLive),
    ScheduleUpdate(ScheduleUpdate),
    Birthday(Birthday),
}
