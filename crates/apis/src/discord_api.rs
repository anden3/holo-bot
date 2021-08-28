use std::{collections::HashMap, sync::Arc, time::Duration as StdDuration};

use anyhow::{anyhow, Context};
use chrono::{Duration, Utc};
use futures::{StreamExt, TryStreamExt};
use lru::LruCache;
use regex::Regex;
use serenity::{
    builder::{CreateEmbed, CreateMessage},
    http::Http,
    model::{
        channel::{Channel, ChannelCategory, Message, MessageReference, MessageType},
        id::{ChannelId, GuildId, MessageId, RoleId, UserId},
        misc::Mention,
    },
    CacheAndHttp,
};
use tokio::{
    sync::{broadcast, mpsc, oneshot, watch, Mutex},
    time::{sleep, Instant},
};
use tracing::{debug, debug_span, error, info, instrument, Instrument};

use holo_bot_macros::clone_variables;
use utility::{
    config::{Config, Reminder, ReminderLocation, User},
    discord::{DataOrder, SegmentDataPosition, SegmentedMessage},
    extensions::{EmbedRowAddition, EmbedRowEdit, EmbedRowRemoval, MessageExt},
    here, regex,
    streams::{Livestream, StreamUpdate, VideoStatus},
};

use crate::{
    birthday_reminder::Birthday,
    mchad_api::{Listener, MchadApi, RoomUpdate},
    twitter_api::{HoloTweet, HoloTweetReference, ScheduleUpdate},
    types::mchad_api::{EventData, RoomEvent},
};

pub struct DiscordApi;

impl DiscordApi {
    const ARCHIVAL_WARNING_TIME: StdDuration = StdDuration::from_secs(5 * 60);

    #[instrument(skip(ctx, config))]
    pub async fn start(
        ctx: Arc<CacheAndHttp>,
        config: Config,
        channel: mpsc::Receiver<DiscordMessageData>,
        stream_notifier: broadcast::Sender<StreamUpdate>,
        index_receiver: watch::Receiver<HashMap<String, Livestream>>,
        guild_ready: oneshot::Receiver<()>,
        exit_receiver: watch::Receiver<bool>,
    ) {
        let stream_notifier_rx = stream_notifier.subscribe();
        let stream_notifier_rx2 = stream_notifier.subscribe();

        let (archive_tx, archive_rx) = mpsc::unbounded_channel();

        tokio::spawn(
            clone_variables!(ctx, config, mut exit_receiver; {
                tokio::select! {
                    _ = Self::posting_thread(ctx, config, channel) => {},
                    e = exit_receiver.changed() => {
                        if let Err(e) = e {
                            error!("{:#}", e);
                        }
                    }
                }

                info!(task = "Discord posting thread", "Shutting down.");
            })
            .instrument(debug_span!("Discord posting thread")),
        );

        tokio::spawn(
            clone_variables!(ctx, config, index_receiver, mut exit_receiver; {
                tokio::select! {
                    res = Self::stream_update_thread(
                        ctx,
                        config,
                        stream_notifier_rx,
                        index_receiver,
                        guild_ready,
                        archive_tx,
                    ) => {
                        if let Err(e) = res {
                            error!("{:#}", e);
                        }
                    },
                    e = exit_receiver.changed() => {
                        if let Err(e) = e {
                            error!("{:#}", e);
                        }
                    }
                }

                info!(task = "Discord stream notifier thread", "Shutting down.");
            })
            .instrument(debug_span!("Discord stream notifier thread")),
        );

        tokio::spawn(
            clone_variables!(ctx, config, index_receiver, mut exit_receiver; {
                tokio::select! {
                    res = Self::mchad_watch_thread(ctx,
                        config,
                        index_receiver,
                        stream_notifier_rx2) => {
                        if let Err(e) = res {
                            error!("{:#}", e);
                        }
                    },
                    e = exit_receiver.changed() => {
                        if let Err(e) = e {
                            error!("{:#}", e);
                        }
                    }
                }

                info!(task = "Discord LiveTL watch thread", "Shutting down.");
            })
            .instrument(debug_span!("Discord LiveTL watch thread")),
        );

        tokio::spawn(
            clone_variables!(ctx, config, mut exit_receiver; {
                tokio::select! {
                    res = Self::chat_archive_thread(
                        ctx,
                        config,
                        archive_rx,
                    ) => {
                        if let Err(e) = res {
                            error!("{:#}", e);
                        }
                    },
                    e = exit_receiver.changed() => {
                        if let Err(e) = e {
                            error!("{:#}", e);
                        }
                    }
                }

                info!(task = "Discord archiver thread", "Shutting down.");
            })
            .instrument(debug_span!("Discord archiver thread")),
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

    async fn check_if_reply(
        ctx: &Arc<CacheAndHttp>,
        config: &Config,
        tweet: &HoloTweet,
        twitter_channel: ChannelId,
        tweet_cache: &mut LruCache<u64, (MessageReference, String)>,
    ) -> TweetReply {
        // Try to reply to an existing Discord twitter message.
        if let Some(tweet_ref) = &tweet.replied_to {
            // Check if message exists in our cache.
            if let Some((msg_ref, user_name)) = tweet_cache.get(&tweet_ref.tweet) {
                if msg_ref.channel_id == twitter_channel {
                    return TweetReply::SameChannel(user_name.clone(), msg_ref.clone());
                } else if let Some(msg_id) = msg_ref.message_id {
                    return TweetReply::OtherChannel(
                        user_name.clone(),
                        msg_id
                            .link_ensured(&ctx.http, msg_ref.channel_id, msg_ref.guild_id)
                            .await,
                    );
                }
            }
            // Else, search through the latest 100 tweets in the channel.
            else if let Some(tweet_user) =
                config.users.iter().find(|u| u.twitter_id == tweet_ref.user)
            {
                let tweet_channel = tweet_user.get_twitter_channel(config);

                if let Some(msg_ref) = Self::search_for_tweet(ctx, tweet_ref, tweet_channel).await {
                    if tweet_channel == twitter_channel {
                        return TweetReply::SameChannel(tweet_user.display_name.clone(), msg_ref);
                    } else if let Some(msg_id) = msg_ref.message_id {
                        return TweetReply::OtherChannel(
                            tweet_user.display_name.clone(),
                            msg_id
                                .link_ensured(&ctx.http, msg_ref.channel_id, msg_ref.guild_id)
                                .await,
                        );
                    }
                }
            }
        }

        TweetReply::None
    }

    #[allow(clippy::too_many_lines)]
    #[instrument(skip(ctx, config))]
    async fn posting_thread(
        ctx: Arc<CacheAndHttp>,
        config: Config,
        mut channel: mpsc::Receiver<DiscordMessageData>,
    ) {
        let mut tweet_messages = LruCache::new(1024);

        loop {
            if let Some(msg) = channel
                .recv()
                .instrument(debug_span!("Waiting for Discord message request."))
                .await
            {
                match msg {
                    DiscordMessageData::Tweet(tweet) => {
                        let role: RoleId = tweet.user.discord_role.into();
                        let tweet_id = tweet.id;
                        let name = tweet.user.display_name.clone();

                        let twitter_channel = tweet.user.get_twitter_channel(&config);

                        let reply = Self::check_if_reply(
                            &ctx,
                            &config,
                            &tweet,
                            twitter_channel,
                            &mut tweet_messages,
                        )
                        .await;

                        let message = Self::send_message(&ctx.http, twitter_channel, |m| {
                            m.allowed_mentions(|am| am.empty_parse().roles(vec![role]))
                                .embed(|e| {
                                    e.colour(tweet.user.colour).author(|a| {
                                        a.name(&tweet.user.display_name);
                                        a.url(&tweet.link);
                                        a.icon_url(&tweet.user.icon);

                                        a
                                    });

                                    if let TweetReply::OtherChannel(user, link) = &reply {
                                        e.field(
                                            format!("Replying to {}", user),
                                            format!("[Link to tweet]({})", link),
                                            false,
                                        );
                                        e.field("Tweet".to_string(), tweet.text, false);
                                    } else {
                                        e.description(&tweet.text);
                                    }

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

                            if let TweetReply::SameChannel(_, msg_ref) = reply {
                                m.reference_message(msg_ref);
                            }

                            m
                        })
                        .await
                        .context(here!());

                        match message {
                            Ok(m) => {
                                tweet_messages.put(
                                    tweet_id,
                                    (MessageReference::from((twitter_channel, m.id)), name),
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
                                            .url(&live.url)
                                            .timestamp(&live.start_at)
                                            .colour(user.colour)
                                            .image(&live.thumbnail)
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
                    DiscordMessageData::Reminder(ref reminder) => {
                        let mut channel_map: HashMap<ChannelId, (Vec<UserId>, bool)> =
                            HashMap::new();

                        for subscriber in &reminder.subscribers {
                            let (channel_id, public) = match subscriber.location {
                                ReminderLocation::DM => {
                                    match subscriber.user.create_dm_channel(&ctx).await {
                                        Ok(ch) => (ch.id, false),
                                        Err(e) => {
                                            error!("{:?}", e);
                                            continue;
                                        }
                                    }
                                }
                                ReminderLocation::Channel(id) => (id, true),
                            };

                            channel_map
                                .entry(channel_id)
                                .and_modify(|(v, _)| v.push(subscriber.user))
                                .or_insert((Vec::new(), public));
                        }

                        for (channel, (users, public)) in channel_map {
                            let result = channel
                                .send_message(&ctx.http, |m| {
                                    if public {
                                        m.content(
                                            users
                                                .into_iter()
                                                .fold(String::new(), |acc, u| {
                                                    acc + &format!("{} ", Mention::from(u))
                                                })
                                                .trim(),
                                        );
                                    }

                                    m.embed(|e| {
                                        e.title("Reminder!")
                                            .description(&reminder.message)
                                            .timestamp(&reminder.time)
                                    })
                                })
                                .await;

                            if let Err(e) = result {
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
    #[instrument(skip(
        ctx,
        config,
        stream_notifier,
        index_receiver,
        guild_ready,
        stream_archiver
    ))]
    async fn stream_update_thread(
        ctx: Arc<CacheAndHttp>,
        config: Config,
        mut stream_notifier: broadcast::Receiver<StreamUpdate>,
        mut index_receiver: watch::Receiver<HashMap<String, Livestream>>,
        guild_ready: oneshot::Receiver<()>,
        stream_archiver: mpsc::UnboundedSender<(ChannelId, Option<Livestream>)>,
    ) -> anyhow::Result<()> {
        let _ = guild_ready.await.context(here!())?;

        let chat_category = ChannelId(config.stream_chat_category);
        let active_category = chat_category
            .to_channel(&ctx.http)
            .await
            .context(here!())?
            .category()
            .unwrap();

        let guild_id = active_category.guild_id;

        let ready_index = loop {
            index_receiver.changed().await.context(here!())?;
            let index = index_receiver.borrow();

            if !index.is_empty() {
                break index.clone();
            }
        };

        let mut claimed_channels: HashMap<String, ChannelId> = HashMap::with_capacity(32);

        for (ch, topic) in Self::get_old_stream_chats(&ctx, guild_id, chat_category).await? {
            match Self::try_find_stream_for_channel(&topic, &ready_index) {
                Some((stream, VideoStatus::Live)) => {
                    claimed_channels.insert(stream.id, ch);
                }
                Some((stream, VideoStatus::Past)) => stream_archiver.send((ch, Some(stream)))?,
                _ => stream_archiver.send((ch, None))?,
            }
        }

        for stream in ready_index.values() {
            if claimed_channels.contains_key(&stream.id) || stream.state != VideoStatus::Live {
                continue;
            }

            let claimed_channel = Self::claim_channel(&ctx, &active_category, stream).await?;
            claimed_channels.insert(stream.id.clone(), claimed_channel);
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
                    info!(stream = %stream.title, "Stream started!");
                    if claimed_channels.contains_key(&stream.id) {
                        continue;
                    }

                    let claim = Self::claim_channel(&ctx, &active_category, &stream).await?;

                    claimed_channels.insert(stream.id, claim);
                }
                StreamUpdate::Ended(stream) => {
                    info!(stream = %stream.title, "Stream ended!");

                    let claimed_channel = match claimed_channels.remove(&stream.id) {
                        Some(s) => s,
                        None => continue,
                    };

                    stream_archiver.send((claimed_channel, Some(stream)))?;
                }
                _ => (),
            }
        }
    }

    #[instrument(skip(ctx, config, index_receiver, stream_notifier))]
    async fn mchad_watch_thread(
        ctx: Arc<CacheAndHttp>,
        config: Config,
        mut index_receiver: watch::Receiver<HashMap<String, Livestream>>,
        mut stream_notifier: broadcast::Receiver<StreamUpdate>,
    ) -> anyhow::Result<()> {
        let mut live_streams: HashMap<_, _> = loop {
            index_receiver.changed().await.context(here!())?;
            let index = index_receiver.borrow();

            if !index.is_empty() {
                break index
                    .iter()
                    .filter(|(_, s)| s.state == VideoStatus::Live)
                    .map(|(id, l)| (id.clone(), l.streamer.twitter_id))
                    .collect();
            }
        };

        let guild_id = ChannelId(config.stream_chat_category)
            .to_channel(&ctx.http)
            .await
            .context(here!())?
            .category()
            .unwrap()
            .guild_id;

        let mut mchad = MchadApi::connect();

        loop {
            tokio::select! {
                res = stream_notifier.recv() => {
                    let update = match res {
                        Ok(u) => u,
                        Err(e) => {
                            error!(loc = here!(), "{:?}", e);
                            continue;
                        }
                    };

                    match update {
                        StreamUpdate::Started(stream) => {
                            live_streams.insert(stream.id.clone(), stream.streamer.twitter_id);
                        }
                        StreamUpdate::Ended(stream) => {
                            live_streams.remove(&stream.id);
                        }
                        _ => (),
                    }
                }

                res = mchad.room_updates.recv() => {
                    let update = match res {
                        Ok(u) => u,
                        Err(e) => {
                            error!(loc = here!(), "{:?}", e);
                            continue;
                        }
                    };

                    match update {
                        RoomUpdate::Added(stream) | RoomUpdate::Changed(_, stream) => {
                            if live_streams.contains_key(&stream) {
                                let talent_twitter_id = live_streams.get(&stream).unwrap();
                                let talent = match config.users.iter().find(|u| u.twitter_id == *talent_twitter_id) {
                                    Some(u) => u.clone(),
                                    None => continue,
                                };

                                if let Some(listener) = mchad.get_listener(&stream).await {
                                    let ctx = Arc::clone(&ctx);

                                    tokio::spawn(async move {
                                        Self::bounce_mchad_messages(ctx, guild_id, stream.clone(), talent, listener).await
                                    });
                                }
                            }
                        }

                        _ => (),
                    }
                }
            }
        }
    }

    #[instrument(skip(ctx, talent))]
    async fn bounce_mchad_messages(
        ctx: Arc<CacheAndHttp>,
        guild_id: GuildId,
        stream: String,
        talent: User,
        listener: Listener,
    ) -> anyhow::Result<()> {
        let (channel, _) = guild_id.channels(&ctx.http).await?.into_iter().find(|(_, ch)| {
            matches!(&ch.topic, Some(url) if *url == format!("https://youtube.com/watch?v={}", &stream))
        }).ok_or_else(|| anyhow!("Failed to find stream!"))?;

        let mut posted_messages: Vec<Message> = Vec::with_capacity(1024);
        let mut message_indices: HashMap<String, (usize, usize)> = HashMap::with_capacity(1024);
        let mut message_stats: HashMap<MessageId, (usize, usize)> = HashMap::with_capacity(1024);

        let mut last_tl_message = MessageId(0);

        let room_name = listener.room.borrow().name.clone();

        info!(%stream, "Starting to listen!");

        let mut event_stream = Box::pin(listener);

        let notification_embed = CreateEmbed::default()
            .author(|a| {
                a.name("MChad Discord Integration")
                    .url("https://mchatx.org/")
            })
            .colour(talent.colour)
            .footer(|f| f.text(format!("Room: {}", room_name)))
            .to_owned();

        let default_embed = CreateEmbed::default()
            .author(|a| a.name(&talent.display_name).icon_url(&talent.icon))
            .colour(talent.colour)
            .to_owned();

        while let Some(event) = event_stream.next().await {
            use EventData::*;

            debug!(?event);

            match event {
                Connect(_msg) => {
                    debug!(message = %_msg, "Connected to MChad.");

                    let _ = channel
                        .send_message(&ctx.http, |m| {
                            m.set_embed(
                                notification_embed
                                    .clone()
                                    .description("Connected to MChad!")
                                    .to_owned(),
                            )
                        })
                        .await?;
                }

                Update(RoomEvent { id, text, time: _ })
                | Insert(RoomEvent { id, text, time: _ })
                    if message_indices.contains_key(&id) =>
                {
                    debug!(%id, %text, "Updating message.");

                    let (msg_idx, row_idx) = message_indices.get(&id).unwrap();
                    let msg = posted_messages.get_mut(*msg_idx).unwrap();

                    let EmbedRowEdit { size } = msg
                        .edit_embed_row(&ctx, &default_embed, *row_idx, text)
                        .await?;

                    let (_, bytes) = message_stats.get_mut(&msg.id).unwrap();
                    *bytes = size;
                }

                Insert(RoomEvent { id, text, time: _ })
                | Update(RoomEvent { id, text, time: _ })
                    if !message_indices.contains_key(&id) =>
                {
                    debug!(%id, %text, "New message.");

                    let should_update_msg = Self::get_last_message_id_in_channel(&ctx, &channel)
                        .await
                        .map(|id| {
                            if id != last_tl_message {
                                return false;
                            }

                            let (_, msg_size) = message_stats.get(&id).unwrap();

                            if msg_size + text.len() > 4096 {
                                return false;
                            }

                            true
                        })
                        .unwrap_or(false);

                    if should_update_msg {
                        let msg_idx = posted_messages
                            .iter()
                            .position(|m| m.id == last_tl_message)
                            .unwrap();
                        let message = posted_messages.get_mut(msg_idx).unwrap();

                        let EmbedRowAddition { size } =
                            message.add_embed_row(&ctx, &default_embed, text).await?;

                        let (last_row, bytes) = message_stats.get_mut(&last_tl_message).unwrap();

                        *last_row += 1;
                        *bytes = size;

                        message_indices.insert(id, (msg_idx, *last_row));
                    } else {
                        let text_length = text.len();

                        let message = channel
                            .send_message(&ctx.http, |m| {
                                m.set_embed(default_embed.clone().description(&text).to_owned())
                            })
                            .await?;

                        last_tl_message = message.id;

                        message_indices.insert(id, (posted_messages.len(), 0));
                        message_stats.insert(message.id, (0, text_length));
                        posted_messages.push(message);
                    }
                }

                Delete(id) => {
                    debug!(%id, "Deleting message.");

                    if let Some((msg_idx, row_idx)) = message_indices.remove(&id) {
                        let mut remove_msg = false;

                        if let Some(message) = posted_messages.get_mut(msg_idx) {
                            let EmbedRowRemoval {
                                msg_deleted,
                                last_row,
                                size,
                            } = message
                                .remove_embed_row(&ctx, &default_embed, row_idx)
                                .await?;

                            if msg_deleted {
                                remove_msg = true;
                                message_stats.remove(&message.id);

                                if last_tl_message == message.id {
                                    last_tl_message = MessageId(0);
                                }
                            } else {
                                message_stats.insert(message.id, (last_row, size));

                                // Decrement indices larger than the deleted row.
                                message_indices
                                    .iter_mut()
                                    .filter(|(_, (m_idx, r_idx))| {
                                        *m_idx == msg_idx && *r_idx > row_idx
                                    })
                                    .for_each(|(_, (_, r_idx))| *r_idx -= 1);
                            }
                        }

                        if remove_msg {
                            posted_messages.remove(msg_idx);
                        }
                    }
                }

                _ => (),
            }
        }

        info!(%stream, "Listener dropped.");

        let _ = channel
            .send_message(&ctx.http, |m| {
                m.set_embed(
                    notification_embed
                        .clone()
                        .description("MChad listener timed out...")
                        .to_owned(),
                )
            })
            .await?;

        Ok(())
    }

    async fn get_old_stream_chats(
        ctx: &Arc<CacheAndHttp>,
        guild: GuildId,
        chat_category: ChannelId,
    ) -> anyhow::Result<impl Iterator<Item = (ChannelId, String)>> {
        let guild_channels = guild.channels(&ctx.http).await?;

        Ok(guild_channels.into_iter().filter_map(move |(_, ch)| {
            ch.category_id
                .map(|category| {
                    (category == chat_category).then(|| (ch.id, ch.topic.unwrap_or_default()))
                })
                .flatten()
        }))
    }

    fn try_find_stream_for_channel(
        topic: &str,
        index: &HashMap<String, Livestream>,
    ) -> Option<(Livestream, VideoStatus)> {
        let stream = index.values().find(|s| s.url == topic)?;

        match &stream.state {
            VideoStatus::Upcoming => {
                error!("This should never happen.");
                None
            }
            VideoStatus::Live | VideoStatus::Past => Some((stream.clone(), stream.state)),
            VideoStatus::New => todo!(),
            VideoStatus::Missing => todo!(),
            _ => todo!(),
        }
    }

    async fn get_last_message_id_in_channel(
        ctx: &Arc<CacheAndHttp>,
        channel: &ChannelId,
    ) -> Option<MessageId> {
        match channel.to_channel(&ctx.http).await {
            Ok(Channel::Guild(ch)) => ch.last_message_id,
            Ok(Channel::Private(ch)) => ch.last_message_id,
            Ok(_) => None,
            Err(e) => {
                error!("{:?}", e);
                None
            }
        }
    }

    async fn chat_archive_thread(
        ctx: Arc<CacheAndHttp>,
        config: Config,
        mut archive_notifier: mpsc::UnboundedReceiver<(ChannelId, Option<Livestream>)>,
    ) -> anyhow::Result<()> {
        let log_ch = ChannelId(config.stream_chat_logs);
        let log_ch = Arc::new(Mutex::new(log_ch));

        while let Some((channel, stream)) = archive_notifier.recv().await {
            let log_clone = Arc::clone(&log_ch);
            let ctx_clone = Arc::clone(&ctx);
            let discussion_ch = stream
                .as_ref()
                .map(|s| {
                    config
                        .branch_channels
                        .get(&s.streamer.branch)
                        .map(|c| ChannelId(*c))
                })
                .flatten();

            let _ = tokio::spawn(async move {
                if let Err(e) =
                    Self::archive_channel(ctx_clone, channel, stream, log_clone, discussion_ch)
                        .await
                {
                    error!("{:?}", e);
                }
            });
        }

        Ok(())
    }

    async fn archive_channel(
        ctx: Arc<CacheAndHttp>,
        channel: ChannelId,
        stream: Option<Livestream>,
        log_channel: Arc<Mutex<ChannelId>>,
        discussion_ch: Option<ChannelId>,
    ) -> anyhow::Result<()> {
        let cache = &ctx.cache;

        let message_stream = channel.messages_iter(&ctx.http);
        let stream_start = match stream.as_ref() {
            Some(s) => s.start_at,
            None => channel.created_at(),
        };
        let stream_id = stream.as_ref().map(|s| &s.url);

        let messages = message_stream
            .try_filter_map(|msg| async move {
                if !Self::should_message_be_archived(&msg) {
                    return Ok(None);
                }

                Ok(Some(ArchivedMessage {
                    author: Mention::from(msg.author.id),
                    content: msg.content_safe(&cache).await,
                    video_id: stream_id,
                    timestamp: msg.timestamp - stream_start,
                    attachment_urls: msg.attachments.iter().map(|a| a.url.clone()).collect(),
                }))
            })
            .map_ok(|msg| msg.to_string())
            .try_collect::<Vec<String>>()
            .await?;

        if messages.is_empty() {
            channel.delete(&ctx.http).await?;
            return Ok(());
        }

        let start_time = Instant::now();

        channel
            .send_message(&ctx.http, |m| {
                m.embed(|e| {
                    e.title("Stream has ended!");

                    let formatted_archival_time = match (
                        Self::ARCHIVAL_WARNING_TIME.as_secs() / 60,
                        Self::ARCHIVAL_WARNING_TIME.as_secs() % 60
                    ) {
                        (0, 0..=30) => "now".to_string(),
                        (m, 50..=59) => format!("in {} minutes", m + 1),
                        (m, 0..=10) => format!("in {} minutes", m),
                        (0, s) => format!("in {} seconds", s),
                        (m, s) => format!("in {} minutes and {} seconds", m, s),
                    };

                    e.description(
                        if let Some(discussion_ch) = &discussion_ch {
                        format!(
                            "Feel free to continue talking in {}!\nThis stream will be archived {}.",
                            Mention::from(*discussion_ch), formatted_archival_time
                        )
                    } else {
                        format!("This stream will be archived {}.", formatted_archival_time)
                    });

                    e.colour(
                        stream
                            .as_ref()
                            .map(|s| s.streamer.colour)
                            .unwrap_or(6_282_735),
                    )
                })
            })
            .await?;

        let mut seg_msg = SegmentedMessage::<String, Livestream>::new();
        let seg_msg = seg_msg
            .data(messages)
            .order(DataOrder::Reverse)
            .position(SegmentDataPosition::Fields)
            .segment_format(Box::new(|e, i, _| {
                e.title(format!("Log {}", i + 1));
            }))
            .link_format(Box::new(|i, m, _| {
                format!("[Log {}]({})\n", i + 1, m.link())
            }));

        let seg_msg = match stream {
            Some(stream) => seg_msg
                .colour(stream.streamer.colour)
                .index_format(Box::new(move |e, i, _| {
                    if i == 0 {
                        e.title(format!("Logs from {}", &stream.title))
                            .url(&stream.url)
                            .thumbnail(&stream.thumbnail)
                            .timestamp(&stream.duration.map_or_else(Utc::now, |d| {
                                stream.start_at + chrono::Duration::seconds(d as i64)
                            }))
                            .author(|a| {
                                a.name(&stream.streamer.display_name)
                                    .url(format!(
                                        "https://www.youtube.com/channel/{}",
                                        &stream.streamer.channel
                                    ))
                                    .icon_url(&stream.streamer.icon)
                            });
                    }
                })),
            None => seg_msg.index_format(Box::new(|e, i, _| {
                if i == 0 {
                    e.title("Logs from unknown stream").timestamp(&Utc::now());
                }
            })),
        };

        seg_msg.create(&ctx, log_channel).await?;

        let archival_time = Instant::now() - start_time;
        let time_to_wait = Self::ARCHIVAL_WARNING_TIME - archival_time;

        sleep(time_to_wait).await;

        channel.delete(&ctx.http).await?;

        Ok(())
    }

    fn should_message_be_archived(msg: &Message) -> bool {
        if msg.author.bot {
            return false;
        }

        if msg.content.is_empty() && msg.attachments.is_empty() {
            return false;
        }

        if msg.content.len() > 1000 {
            return false;
        }

        match msg.kind {
            MessageType::Regular | MessageType::InlineReply => (),
            _ => return false,
        }

        if msg.attachments.is_empty() && msg.is_only_emojis() {
            return false;
        }

        true
    }

    async fn claim_channel(
        ctx: &Arc<CacheAndHttp>,
        category: &ChannelCategory,
        stream: &Livestream,
    ) -> anyhow::Result<ChannelId> {
        let channel_name = format!(
            "{}-{}-stream",
            stream.streamer.emoji,
            stream
                .streamer
                .display_name
                .to_ascii_lowercase()
                .replace(' ', "-")
        );
        let channel_topic = &stream.url;

        let channel = category
            .guild_id
            .create_channel(&ctx.http, |c| {
                c.name(channel_name)
                    .category(category.id)
                    .position(1)
                    .topic(channel_topic)
                    .permissions(category.permission_overwrites.clone())
            })
            .await
            .context(here!())?;

        channel
            .send_message(&ctx.http, |m| {
                m.embed(|e| {
                    e.title("Now watching")
                        .description(&stream.title)
                        .url(&stream.url)
                        .timestamp(&stream.start_at)
                        .colour(stream.streamer.colour)
                        .image(&stream.thumbnail)
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

        Ok(channel.id)
    }
}

#[derive(Debug)]
pub enum DiscordMessageData {
    Tweet(HoloTweet),
    ScheduledLive(Livestream),
    ScheduleUpdate(ScheduleUpdate),
    Birthday(Birthday),
    Reminder(Reminder),
}

struct ArchivedMessage<'a> {
    pub author: Mention,
    pub content: String,
    pub timestamp: Duration,
    pub attachment_urls: Vec<String>,
    pub video_id: Option<&'a String>,
}

impl ArchivedMessage<'_> {
    pub fn format_timestamp(&self) -> String {
        let hours = (self.timestamp.num_hours() != 0)
            .then(|| format!("{:02}:", self.timestamp.num_hours().abs()))
            .unwrap_or_default();

        let minutes = self.timestamp.num_minutes() % 60;
        let seconds = self.timestamp.num_seconds() % 60;

        // Check if message was sent before the stream started.
        if self.timestamp.num_seconds() < 0 {
            format!("-{}{:02}:{:02}", hours, minutes.abs(), seconds.abs())
        } else {
            let timestamp = format!("{}{:02}:{:02}", hours, minutes, seconds);

            if let Some(id) = &self.video_id {
                let url = format!(
                    "https://youtu.be/{id}?t={secs}",
                    id = id,
                    secs = self.timestamp.num_seconds()
                );
                format!("[{time}]({url})", time = timestamp, url = url)
            } else {
                timestamp
            }
        }
    }
}

impl std::fmt::Display for ArchivedMessage<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        writeln!(
            f,
            "{} {}: {}",
            self.format_timestamp(),
            self.author,
            self.content
        )?;

        if !self.attachment_urls.is_empty() {
            writeln!(f, "{}", self.attachment_urls.join(" "))
        } else {
            Ok(())
        }
    }
}

enum TweetReply {
    None,
    SameChannel(String, MessageReference),
    OtherChannel(String, String),
}
