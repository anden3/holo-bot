use std::{collections::HashMap, sync::Arc};

use super::birthday_reminder::Birthday;
use super::holo_api::Livestream;
use super::twitter_api::{HoloTweet, ScheduleUpdate};

use utility::{config::Config, here, regex};

use anyhow::{anyhow, Context};
use futures::StreamExt;
use log::{debug, error, info};
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
use tokio::sync::{
    mpsc::{Receiver, UnboundedReceiver},
    watch,
};

use super::holo_api::StreamUpdate;

pub struct DiscordApi {}

impl DiscordApi {
    pub async fn start(
        ctx: Arc<CacheAndHttp>,
        channel: Receiver<DiscordMessageData>,
        stream_notifier: UnboundedReceiver<StreamUpdate>,
        config: Config,
        mut exit_receiver: watch::Receiver<bool>,
    ) {
        let cache_copy = Arc::<serenity::CacheAndHttp>::clone(&ctx);
        let config_copy = config.clone();
        let mut exit_receiver_clone = exit_receiver.clone();

        tokio::spawn(async move {
            tokio::select! {
                _ = Self::posting_thread(ctx, channel, config) => {},
                e = exit_receiver.changed() => {
                    if let Err(e) = e {
                        error!("{:#}", e);
                    }
                }
            }

            info!("Shutting down posting thread...");
        });

        tokio::spawn(async move {
            tokio::select! {
                _ = Self::stream_update_thread(cache_copy,
                    stream_notifier,
                    config_copy,) => {},

                e = exit_receiver_clone.changed() => {
                    if let Err(e) = e {
                        error!("{:#}", e);
                    }
                }
            }

            info!("Shutting down stream update thread...");
        });
    }

    pub async fn send_message<'a, F: Sync + Send>(
        http: &Arc<Http>,
        channel: ChannelId,
        f: F,
    ) -> anyhow::Result<Message>
    where
        for<'b> F: FnOnce(&'b mut CreateMessage<'a>) -> &'b mut CreateMessage<'a>,
    {
        match channel.send_message(&http, f).await {
            Ok(m) => Ok(m),
            Err(e) => {
                error!("{:?}", e);
                Err(anyhow!(e))
            }
        }
    }

    #[allow(clippy::too_many_lines)]
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
                                // Only allow if in the same channel until Discord allows for cross-channel replies.
                                if msg_ref.channel_id == twitter_channel {
                                    message_ref = Some(msg_ref.clone());
                                }
                            }
                            // Else, search through the latest 100 tweets in the channel.
                            else if let Some(tweet_user) =
                                config.users.iter().find(|u| u.twitter_id == tweet_ref.user)
                            {
                                let tweet_channel = tweet_user.get_twitter_channel(&config);

                                // Only allow if in the same channel until Discord allows for cross-channel replies.
                                if tweet_channel == twitter_channel {
                                    let mut message_stream =
                                        tweet_channel.messages_iter(&ctx.http).take(100).boxed();

                                    while let Some(found_msg) = message_stream.next().await {
                                        let msg = match found_msg.context(here!()) {
                                            Ok(m) => m,
                                            Err(err) => {
                                                error!("{:?}", err);
                                                break;
                                            }
                                        };

                                        let twitter_link: &'static Regex =
                                            regex!(r#"https://twitter\.com/\d+/status/(\d+)/?"#);

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
                                }
                            }
                        }

                        let message = Self::send_message(&ctx.http, twitter_channel, |m| {
                            m.allowed_mentions(|am| {
                                am.empty_parse();
                                am.roles(vec![role]);

                                am
                            });

                            m.embed(|e| {
                                e.description(&tweet.text);
                                e.timestamp(&tweet.timestamp);
                                e.colour(user.colour);
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

                                match &tweet.media[..] {
                                    [] => (),
                                    [a, ..] => {
                                        e.image(a);
                                    }
                                };

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
                        .await
                        .context(here!());

                        match message {
                            Ok(m) => {
                                tweet_messages.insert(
                                    tweet.id,
                                    MessageReference::from((twitter_channel, m.id)),
                                );
                            }
                            Err(e) => {
                                error!("{:?}", e);
                                continue;
                            }
                        }
                    }

                    DiscordMessageData::ScheduledLive(live) => {
                        if let Some(user) = config.users.iter().find(|u| **u == live.streamer) {
                            let livestream_channel = ChannelId(config.live_notif_channel);
                            let role: RoleId = user.discord_role.into();

                            let message = Self::send_message(&ctx.http, livestream_channel, |m| {
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
                                    e.colour(user.colour);
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
                            .await
                            .context(here!());

                            if let Err(e) = message {
                                error!("{:?}", e);
                                continue;
                            }
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

                            let message = Self::send_message(&ctx.http, schedule_channel, |m| {
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
                                    e.colour(user.colour);
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
                            .await
                            .context(here!());

                            if let Err(e) = message {
                                error!("{:?}", e);
                                continue;
                            }
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

                            let message = Self::send_message(&ctx.http, birthday_channel, |m| {
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
                                    e.colour(user.colour);
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
                            .await
                            .context(here!());

                            if let Err(e) = message {
                                error!("{:?}", e);
                                continue;
                            }
                        }
                    }
                }
            }
        }
    }

    #[allow(clippy::no_effect)]
    async fn stream_update_thread(
        _ctx: Arc<CacheAndHttp>,
        mut stream_notifier: UnboundedReceiver<StreamUpdate>,
        _config: Config,
    ) {
        loop {
            if let Some(_msg) = stream_notifier.recv().await {
                ();
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
