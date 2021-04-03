use std::{collections::HashMap, sync::Arc};

use super::birthday_reminder::Birthday;
use super::config::Config;
use super::holo_api::ScheduledLive;
use super::twitter_api::{HoloTweet, ScheduleUpdate};

use log::error;
use serenity::{
    builder::CreateMessage,
    model::{
        channel::{Message, MessageReference},
        id::{ChannelId, RoleId},
        misc::Mention,
    },
    CacheAndHttp,
};
use tokio::sync::mpsc::Receiver;

pub struct DiscordAPI {
    pub cache_and_http: Arc<CacheAndHttp>,
}

impl DiscordAPI {
    pub async fn send_message<'a, F>(&self, channel: ChannelId, f: F) -> Option<Message>
    where
        for<'b> F: FnOnce(&'b mut CreateMessage<'a>) -> &'b mut CreateMessage<'a>,
    {
        match channel.send_message(&self.cache_and_http.http, f).await {
            Ok(m) => Some(m),
            Err(e) => {
                error!("{}", e);
                None
            }
        }
    }

    pub async fn posting_thread(
        discord: DiscordAPI,
        mut channel: Receiver<DiscordMessageData>,
        config: Config,
    ) {
        let mut tweet_messages: HashMap<u64, MessageReference> = HashMap::new();

        loop {
            if let Some(msg) = channel.recv().await {
                match msg {
                    DiscordMessageData::Tweet(tweet) => {
                        let user = &tweet.user;
                        let role: RoleId = user.discord_role.into();

                        let twitter_channel = user.get_twitter_channel(&config);

                        let message = discord
                            .send_message(twitter_channel, |m| {
                                m.allowed_mentions(|am| {
                                    am.empty_parse();
                                    am.roles(vec![role]);

                                    am
                                });

                                m.embed(|e| {
                                    e.description(&tweet.text);
                                    e.timestamp(&tweet.timestamp);
                                    e.colour(u32::from(user.colour));
                                    e.author(|a| {
                                        a.name(&user.display_name);
                                        a.url(&tweet.link);
                                        a.icon_url(&user.icon);

                                        a
                                    });
                                    e.footer(|f| {
                                        f.text("Provided by HoloBot (created by anden3)");

                                        f
                                    });

                                    if !tweet.media.is_empty() {
                                        e.image(&tweet.media[0]);
                                    }

                                    if let Some(translation) = &tweet.translation {
                                        e.field("Machine Translation", translation, false);
                                    }

                                    e
                                });

                                // Try to reply to an existing Discord twitter message.
                                if let Some(tweet_ref) = &tweet.replied_to {
                                    if let Some(msg_ref) = tweet_messages.get(&tweet_ref.tweet) {
                                        m.reference_message(msg_ref.clone());
                                    }
                                }

                                m
                            })
                            .await;

                        if let Some(m) = message {
                            tweet_messages
                                .insert(tweet.id, MessageReference::from((twitter_channel, m.id)));
                        }
                    }

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
                    DiscordMessageData::Birthday(birthday) => {
                        if let Some(user) = config
                            .users
                            .iter()
                            .find(|u| u.display_name == birthday.user)
                        {
                            let birthday_channel = ChannelId(config.birthday_notif_channel);
                            let role: RoleId = user.discord_role.into();

                            discord
                                .send_message(birthday_channel, |m| {
                                    m.content(Mention::from(role));

                                    m.allowed_mentions(|am| {
                                        am.empty_parse();
                                        am.roles(vec![role]);

                                        am
                                    });

                                    m.embed(|e| {
                                        e.title(format!(
                                            "It is {}'s birthday today!!!",
                                            user.display_name
                                        ));
                                        e.timestamp(&birthday.birthday);
                                        e.colour(u32::from(user.colour));
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
    Tweet(HoloTweet),
    ScheduledLive(ScheduledLive),
    ScheduleUpdate(ScheduleUpdate),
    Birthday(Birthday),
}
