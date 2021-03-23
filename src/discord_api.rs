use std::sync::Arc;

use super::birthday_reminder::Birthday;
use super::config::Config;
use super::holo_api::ScheduledLive;
use super::twitter_api::{HoloTweet, ScheduleUpdate};

use serenity::{
    builder::CreateMessage,
    model::{
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
    pub async fn send_message<'a, F>(&self, channel: ChannelId, f: F)
    where
        for<'b> F: FnOnce(&'b mut CreateMessage<'a>) -> &'b mut CreateMessage<'a>,
    {
        if let Err(e) = channel.send_message(&self.cache_and_http.http, f).await {
            eprintln!("{}", e);
        }
    }

    pub async fn posting_thread(
        discord: DiscordAPI,
        mut channel: Receiver<DiscordMessageData>,
        config: Config,
    ) {
        /*
        let message = serenity::utils::MessageBuilder::new()
            .push_bold_line("Hello everyone!")
            .push("")
            .build();

        discord
            .send_message(ChannelId(755759901426319400), |m| {
                m.content(message);

                m
            })
            .await;
        */

        loop {
            if let Some(msg) = channel.recv().await {
                match msg {
                    DiscordMessageData::Tweet(tweet) => {
                        let user = &tweet.user;
                        let twitter_channel = ChannelId(config.twitter_channel);
                        let role: RoleId = user.discord_role.into();

                        discord
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

                                m
                            })
                            .await;
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
