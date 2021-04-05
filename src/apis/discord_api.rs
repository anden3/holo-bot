use std::{collections::HashMap, sync::Arc};

use crate::apis::holo_api::Livestream;
use crate::apis::twitter_api::{HoloTweet, ScheduleUpdate};
use crate::birthday_reminder::Birthday;
use crate::config::Config;
use crate::regex;

use futures::StreamExt;
use log::{debug, error, warn};
use regex::Regex;
use serenity::{
    builder::CreateMessage,
    http::Http,
    model::{
        channel::{Message, MessageReference},
        id::{ChannelId, RoleId},
        misc::Mention,
    },
    CacheAndHttp,
};
use tokio::sync::mpsc::{Receiver, UnboundedReceiver};

use super::holo_api::StreamUpdate;

pub struct DiscordAPI {}

impl DiscordAPI {
    pub async fn start(
        ctx: Arc<CacheAndHttp>,
        channel: Receiver<DiscordMessageData>,
        stream_notifier: UnboundedReceiver<StreamUpdate>,
        config: Config,
    ) {
        let cache_copy = ctx.clone();
        let config_copy = config.clone();

        tokio::spawn(async move {
            DiscordAPI::posting_thread(ctx, channel, config.clone()).await;
        });

        tokio::spawn(async move {
            DiscordAPI::stream_update_thread(cache_copy, stream_notifier, config_copy).await;
        });
    }

    pub async fn send_message<'a, F>(http: &Arc<Http>, channel: ChannelId, f: F) -> Option<Message>
    where
        for<'b> F: FnOnce(&'b mut CreateMessage<'a>) -> &'b mut CreateMessage<'a>,
    {
        match channel.send_message(&http, f).await {
            Ok(m) => Some(m),
            Err(e) => {
                error!("{}", e);
                None
            }
        }
    }

    async fn posting_thread(
        ctx: Arc<CacheAndHttp>,
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
                        let mut message_ref: Option<MessageReference> = None;

                        // Try to reply to an existing Discord twitter message.
                        if let Some(tweet_ref) = &tweet.replied_to {
                            // Check if message exists in our cache.
                            if let Some(msg_ref) = tweet_messages.get(&tweet_ref.tweet) {
                                debug!("Found message reference in cache!");
                                message_ref = Some(msg_ref.clone());
                            }
                            // Else, search through the latest 100 tweets in the channel.
                            else if let Some(tweet_user) =
                                config.users.iter().find(|u| u.twitter_id == tweet_ref.user)
                            {
                                let tweet_channel = tweet_user.get_twitter_channel(&config);
                                let mut message_stream =
                                    tweet_channel.messages_iter(&ctx.http).boxed();

                                debug!(
                                    "Starting search for https://twitter.com/{}/status/{} in {}.",
                                    tweet_user.twitter_id,
                                    tweet_ref.tweet,
                                    tweet_channel.name(&ctx.cache).await.unwrap()
                                );

                                while let Some(found_msg) = message_stream.next().await {
                                    if let Err(err) = found_msg {
                                        error!("{}", err);
                                        continue;
                                    }

                                    let twitter_link: &'static Regex =
                                        regex!(r#"https://twitter\.com/\d+/status/(\d+)/?"#);

                                    let msg = found_msg.unwrap();

                                    // Parse tweet ID from the link in the embed.
                                    let tweet_id = msg.embeds.iter().find_map(|e| {
                                        e.url
                                            .as_ref()
                                            .and_then(|u| twitter_link.captures(u))
                                            .and_then(|cap| cap.get(1))
                                            .and_then(|id| id.as_str().parse::<u64>().ok())
                                    });

                                    if let Some(tweet_id) = tweet_id {
                                        debug!("Testing tweet ID: {}", tweet_id);
                                        if tweet_id == tweet_ref.tweet {
                                            debug!("Found message with matching tweet ID!");
                                            message_ref = Some(MessageReference::from((
                                                tweet_channel,
                                                msg.id,
                                            )));
                                            break;
                                        }
                                    }
                                }

                                if message_ref.is_none() {
                                    warn!("Couldn't find message reference in channel.");
                                }
                            }
                        }

                        let message = DiscordAPI::send_message(&ctx.http, twitter_channel, |m| {
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

                            if let Some(msg_ref) = message_ref {
                                m.reference_message(msg_ref);
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
                        if let Some(user) = config.users.iter().find(|u| **u == live.streamer) {
                            let livestream_channel = ChannelId(config.live_notif_channel);
                            let role: RoleId = user.discord_role.into();

                            DiscordAPI::send_message(&ctx.http, livestream_channel, |m| {
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

                            DiscordAPI::send_message(&ctx.http, schedule_channel, |m| {
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

                            DiscordAPI::send_message(&ctx.http, birthday_channel, |m| {
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

    async fn stream_update_thread(
        _ctx: Arc<CacheAndHttp>,
        mut stream_notifier: UnboundedReceiver<StreamUpdate>,
        _config: Config,
    ) {
        loop {
            if let Some(msg) = stream_notifier.recv().await {
                match msg {
                    StreamUpdate::Scheduled(_) => {}

                    StreamUpdate::Started(_) => {}

                    StreamUpdate::Ended(_) => {}
                }
            }
        }
    }
}

#[derive(Debug)]
pub enum DiscordMessageData {
    Tweet(HoloTweet),
    ScheduledLive(Livestream),
    ScheduleUpdate(ScheduleUpdate),
    Birthday(Birthday),
}
