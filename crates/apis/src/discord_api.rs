use std::{collections::HashMap, sync::Arc};

use crate::{holo_api::StreamState, twitter_api::HoloTweetReference};

use super::{
    birthday_reminder::Birthday,
    holo_api::{Livestream, StreamUpdate},
    twitter_api::{HoloTweet, ScheduleUpdate},
};

use utility::{config::Config, here, regex};

use anyhow::{anyhow, Context};
use futures::StreamExt;
use regex::Regex;
use serenity::{
    builder::CreateMessage,
    http::Http,
    model::{
        channel::{Channel, ChannelCategory, Message, MessageReference},
        id::{ChannelId, RoleId},
        misc::Mention,
    },
    CacheAndHttp,
};
use tokio::sync::{broadcast, mpsc, oneshot, watch};
use tracing::{debug, debug_span, error, info, instrument, Instrument};

pub struct DiscordApi;

impl DiscordApi {
    #[instrument(skip(ctx, config))]
    pub async fn start(
        ctx: Arc<CacheAndHttp>,
        config: Config,
        channel: mpsc::Receiver<DiscordMessageData>,
        stream_notifier: broadcast::Receiver<StreamUpdate>,
        index_receiver: watch::Receiver<HashMap<u32, Livestream>>,
        channel_pool_ready: oneshot::Receiver<Vec<ChannelId>>,
        mut exit_receiver: watch::Receiver<bool>,
    ) {
        let cache_copy = Arc::<serenity::CacheAndHttp>::clone(&ctx);
        let config_copy = config.clone();
        let mut exit_receiver_clone = exit_receiver.clone();

        tokio::spawn(
            async move {
                tokio::select! {
                    _ = Self::posting_thread(ctx, config, channel) => {},
                    e = exit_receiver.changed() => {
                        if let Err(e) = e {
                            error!("{:#}", e);
                        }
                    }
                }

                info!(task = "Discord posting thread", "Shutting down.");
            }
            .instrument(debug_span!(
                "Starting task.",
                task_type = "Discord posting thread"
            )),
        );

        tokio::spawn(
            async move {
                tokio::select! {
                    res = Self::stream_update_thread(
                        cache_copy,
                        config_copy,
                        stream_notifier,
                        index_receiver,
                        channel_pool_ready,
                    ) => {
                        if let Err(e) = res {
                            error!("{:#}", e);
                        }
                    },
                    e = exit_receiver_clone.changed() => {
                        if let Err(e) = e {
                            error!("{:#}", e);
                        }
                    }
                }

                info!(task = "Discord stream notifier thread", "Shutting down.");
            }
            .instrument(debug_span!(
                "Starting task.",
                task_type = "Discord stream update thread"
            )),
        );
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

    #[instrument(skip(ctx))]
    async fn search_for_tweet(
        ctx: &Arc<CacheAndHttp>,
        tweet_ref: &HoloTweetReference,
        channel: ChannelId,
    ) -> Option<MessageReference> {
        let mut message_stream = channel.messages_iter(&ctx.http).take(100).boxed();

        while let Some(found_msg) = message_stream.next().await {
            let msg = match found_msg.context(here!()) {
                Ok(m) => m,
                Err(err) => {
                    error!("{:?}", err);
                    return None;
                }
            };

            let twitter_link: &'static Regex = regex!(r#"https://twitter\.com/\d+/status/(\d+)/?"#);

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
                    return Some(MessageReference::from((channel, msg.id)));
                }
            }
        }

        None
    }

    #[allow(clippy::too_many_lines)]
    #[instrument(skip(ctx, config))]
    async fn posting_thread(
        ctx: Arc<CacheAndHttp>,
        config: Config,
        mut channel: mpsc::Receiver<DiscordMessageData>,
    ) {
        let mut tweet_messages: HashMap<u64, MessageReference> = HashMap::new();

        loop {
            if let Some(msg) = channel
                .recv()
                .instrument(debug_span!("Waiting for Discord message request."))
                .await
            {
                match msg {
                    DiscordMessageData::Tweet(tweet) => {
                        let user = &tweet.user;
                        let role: RoleId = user.discord_role.into();

                        let twitter_channel = user.get_twitter_channel(&config);
                        let mut cross_channel_reply = false;
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
                                cross_channel_reply = tweet_channel == twitter_channel;
                                message_ref =
                                    Self::search_for_tweet(&ctx, tweet_ref, tweet_channel).await;
                            }
                        }

                        let message = Self::send_message(&ctx.http, twitter_channel, |m| {
                            m.allowed_mentions(|am| am.empty_parse().roles(vec![role]))
                                .embed(|e| {
                                    e.description(&tweet.text)
                                        .timestamp(&tweet.timestamp)
                                        .colour(user.colour)
                                        .author(|a| {
                                            a.name(&user.display_name);
                                            a.url(&tweet.link);
                                            a.icon_url(&user.icon);

                                            a
                                        })
                                        .footer(|f| {
                                            f.text("Provided by HoloBot (created by anden3)")
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
                                if !cross_channel_reply {
                                    m.reference_message(msg_ref);
                                }
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
                                m.content(Mention::from(role))
                                    .allowed_mentions(|am| am.empty_parse().roles(vec![role]))
                                    .embed(|e| {
                                        e.title(format!("{} just went live!", user.display_name))
                                            .description(live.title)
                                            .url(format!(
                                                "https://youtube.com/watch?v={}",
                                                live.url
                                            ))
                                            .timestamp(&live.start_at)
                                            .colour(user.colour)
                                            .image(format!(
                                                "https://i3.ytimg.com/vi/{}/maxresdefault.jpg",
                                                live.url
                                            ))
                                            .author(|a| {
                                                a.name(&user.display_name)
                                                    .url(format!(
                                                        "https://www.youtube.com/channel/{}",
                                                        user.channel
                                                    ))
                                                    .icon_url(&user.icon)
                                            })
                                            .footer(|f| {
                                                f.text("Provided by HoloBot (created by anden3)")
                                            })
                                    })
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
                                m.content(Mention::from(role))
                                    .allowed_mentions(|am| {
                                        am.empty_parse();
                                        am.roles(vec![role])
                                    })
                                    .embed(|e| {
                                        e.title(format!(
                                            "{} just released a schedule update!",
                                            user.display_name
                                        ))
                                        .description(update.tweet_text)
                                        .url(update.tweet_link)
                                        .timestamp(&update.timestamp)
                                        .colour(user.colour)
                                        .image(update.schedule_image)
                                        .author(|a| {
                                            a.name(&user.display_name)
                                                .url(format!(
                                                    "https://www.youtube.com/channel/{}",
                                                    user.channel
                                                ))
                                                .icon_url(&user.icon)
                                        })
                                    })
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
                                m.content(Mention::from(role))
                                    .allowed_mentions(|am| am.empty_parse().roles(vec![role]))
                                    .embed(|e| {
                                        e.title(format!(
                                            "It is {}'s birthday today!!!",
                                            user.display_name
                                        ))
                                        .timestamp(&birthday.birthday)
                                        .colour(user.colour)
                                        .author(|a| {
                                            a.name(&user.display_name)
                                                .url(format!(
                                                    "https://www.youtube.com/channel/{}",
                                                    user.channel
                                                ))
                                                .icon_url(&user.icon)
                                        })
                                    })
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
    #[instrument(skip(ctx, config, stream_notifier, index_receiver, channel_pool_ready))]
    async fn stream_update_thread(
        ctx: Arc<CacheAndHttp>,
        config: Config,
        mut stream_notifier: broadcast::Receiver<StreamUpdate>,
        mut index_receiver: watch::Receiver<HashMap<u32, Livestream>>,
        channel_pool_ready: oneshot::Receiver<Vec<ChannelId>>,
    ) -> anyhow::Result<()> {
        debug!("Waiting for pool!");
        let mut channel_pool = channel_pool_ready.await.context(here!())?;
        debug!("Pool received!");

        let mut claimed_channels: HashMap<u32, ChannelId> =
            HashMap::with_capacity(channel_pool.len());

        let ready_index = loop {
            index_receiver.changed().await.context(here!())?;
            let index = index_receiver.borrow();

            if !index.is_empty() {
                break index.clone();
            }
        };

        let active_category = ChannelId(config.holochat_category)
            .to_channel(&ctx.http)
            .await
            .context(here!())?
            .category()
            .unwrap();

        let pool_category = ChannelId(config.stream_chat_pool)
            .to_channel(&ctx.http)
            .await
            .context(here!())?
            .category()
            .unwrap();

        for stream in ready_index.values() {
            if claimed_channels.contains_key(&stream.id) || stream.state != StreamState::Live {
                continue;
            }

            let picked_channel = match channel_pool.pop() {
                Some(c) => c,
                None => {
                    error!(loc = here!(), stream = %stream.title, "No available channel for stream!");
                    continue;
                }
            };

            let claimed_channel =
                Self::claim_channel(&ctx, &picked_channel, &stream, &active_category, false)
                    .await?;

            claimed_channels.insert(stream.id, claimed_channel);
        }

        loop {
            let update = match stream_notifier.recv().await {
                Ok(u) => u,
                Err(e) => {
                    error!(loc = here!(), "{:?}", e);
                    continue;
                }
            };

            match update {
                StreamUpdate::Started(stream) => {
                    info!(loc = here!(), stream = %stream.title, "Stream started!");
                    if claimed_channels.contains_key(&stream.id) {
                        continue;
                    }

                    let picked_channel = match channel_pool.pop() {
                        Some(c) => c,
                        None => {
                            error!(loc = here!(), stream = %stream.title, "No available channel for stream!");
                            continue;
                        }
                    };

                    let claim =
                        Self::claim_channel(&ctx, &picked_channel, &stream, &active_category, true)
                            .await?;

                    claimed_channels.insert(stream.id, claim);
                }
                StreamUpdate::Ended(stream) => {
                    info!(loc = here!(), stream = %stream.title, "Stream ended!");

                    let claimed_channel = match claimed_channels.remove(&stream.id) {
                        Some(s) => s,
                        None => continue,
                    };

                    channel_pool.push(claimed_channel);
                    Self::unclaim_channel(&ctx, &claimed_channel, &pool_category).await?;
                }
                _ => (),
            }
        }
    }

    async fn claim_channel(
        ctx: &Arc<CacheAndHttp>,
        ch: &ChannelId,
        stream: &Livestream,
        category: &ChannelCategory,
        send_message: bool,
    ) -> anyhow::Result<ChannelId> {
        let mut channel = match ch.to_channel(&ctx.http).await.context(here!())? {
            Channel::Guild(c) => c,
            _ => anyhow::bail!("Wrong channel type!"),
        };

        let new_name = format!(
            "{}-{}-stream",
            stream.streamer.emoji,
            stream
                .streamer
                .display_name
                .to_ascii_lowercase()
                .replace(' ', "-")
        );
        let new_topic = format!("https://youtube.com/watch?v={}", stream.url);

        channel
            .edit(&ctx.http, |c| {
                c.name(new_name)
                    .category(category.id)
                    .position(1)
                    .topic(new_topic)
                    .permissions(category.permission_overwrites.clone())
            })
            .await
            .context(here!())?;

        if send_message {
            channel
                .send_message(&ctx.http, |m| {
                    m.embed(|e| {
                        e.title("Now watching")
                            .description(&stream.title)
                            .url(format!("https://youtube.com/watch?v={}", stream.url))
                            .timestamp(&stream.start_at)
                            .colour(stream.streamer.colour)
                            .image(format!(
                                "https://i3.ytimg.com/vi/{}/maxresdefault.jpg",
                                stream.url
                            ))
                            .author(|a| {
                                a.name(&stream.streamer.display_name)
                                    .url(format!(
                                        "https://www.youtube.com/channel/{}",
                                        stream.streamer.channel
                                    ))
                                    .icon_url(&stream.streamer.icon)
                            })
                    })
                })
                .await
                .context(here!())?;
        }

        Ok(channel.id)
    }

    async fn unclaim_channel(
        ctx: &Arc<CacheAndHttp>,
        ch: &ChannelId,
        pool: &ChannelCategory,
    ) -> anyhow::Result<()> {
        ch.edit(&ctx.http, |c| {
            c.name("pooled-stream-chat")
                .category(pool.id)
                .permissions(pool.permission_overwrites.clone())
                .topic("")
        })
        .await
        .context(here!())?;

        Ok(())
    }
}

#[derive(Debug)]
pub enum DiscordMessageData {
    Tweet(HoloTweet),
    ScheduledLive(Livestream),
    ScheduleUpdate(ScheduleUpdate),
    Birthday(Birthday),
}
