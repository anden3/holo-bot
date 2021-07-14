use std::{collections::HashMap, sync::Arc};

use anyhow::{anyhow, Context};
use chrono::{Duration, Utc};
use futures::{StreamExt, TryStreamExt};
use regex::Regex;
use serenity::{
    builder::CreateMessage,
    http::Http,
    model::{
        channel::{ChannelCategory, Message, MessageReference, MessageType},
        id::{ChannelId, GuildId, RoleId, UserId},
        misc::Mention,
    },
    CacheAndHttp,
};
use tokio::sync::{broadcast, mpsc, oneshot, watch, Mutex};
use tracing::{debug, debug_span, error, info, instrument, Instrument};

use utility::{
    config::{Config, Reminder, ReminderLocation},
    discord::{DataOrder, SegmentDataPosition, SegmentedMessage},
    extensions::MessageExt,
    here, regex,
    streams::{Livestream, StreamState, StreamUpdate},
};

use crate::{
    birthday_reminder::Birthday,
    twitter_api::{HoloTweet, HoloTweetReference, ScheduleUpdate},
};

pub struct DiscordApi;

impl DiscordApi {
    #[instrument(skip(ctx, config))]
    pub async fn start(
        ctx: Arc<CacheAndHttp>,
        config: Config,
        channel: mpsc::Receiver<DiscordMessageData>,
        stream_notifier: broadcast::Receiver<StreamUpdate>,
        index_receiver: watch::Receiver<HashMap<u32, Livestream>>,
        guild_ready: oneshot::Receiver<()>,
        mut exit_receiver: watch::Receiver<bool>,
    ) {
        let cache_copy = Arc::<serenity::CacheAndHttp>::clone(&ctx);
        let cache_copy2 = Arc::<serenity::CacheAndHttp>::clone(&ctx);

        let config_copy = config.clone();
        let config_copy2 = config.clone();

        let mut exit_receiver_clone = exit_receiver.clone();
        let mut exit_receiver_clone2 = exit_receiver.clone();

        let (archive_tx, archive_rx) = mpsc::unbounded_channel();

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
            .instrument(debug_span!("Discord posting thread")),
        );

        tokio::spawn(
            async move {
                tokio::select! {
                    res = Self::stream_update_thread(
                        cache_copy,
                        config_copy,
                        stream_notifier,
                        index_receiver,
                        guild_ready,
                        archive_tx,
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
            .instrument(debug_span!("Discord stream notifier thread")),
        );

        tokio::spawn(
            async move {
                tokio::select! {
                    res = Self::chat_archive_thread(
                        cache_copy2,
                        config_copy2,
                        archive_rx,
                    ) => {
                        if let Err(e) = res {
                            error!("{:#}", e);
                        }
                    },
                    e = exit_receiver_clone2.changed() => {
                        if let Err(e) = e {
                            error!("{:#}", e);
                        }
                    }
                }

                info!(task = "Discord archiver thread", "Shutting down.");
            }
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
        mut index_receiver: watch::Receiver<HashMap<u32, Livestream>>,
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

        let mut claimed_channels: HashMap<u32, ChannelId> = HashMap::with_capacity(32);

        for (ch, topic) in Self::get_old_stream_chats(&ctx, guild_id, chat_category).await? {
            match Self::try_find_stream_for_channel(&topic, &ready_index) {
                Some((stream, StreamState::Live)) => {
                    claimed_channels.insert(stream.id, ch);
                }
                Some((stream, StreamState::Ended)) => stream_archiver.send((ch, Some(stream)))?,
                _ => stream_archiver.send((ch, None))?,
            }
        }

        for stream in ready_index.values() {
            if claimed_channels.contains_key(&stream.id) || stream.state != StreamState::Live {
                continue;
            }

            let claimed_channel = Self::claim_channel(&ctx, &active_category, &stream).await?;
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

                    let claim = Self::claim_channel(&ctx, &active_category, &stream).await?;

                    claimed_channels.insert(stream.id, claim);
                }
                StreamUpdate::Ended(stream) => {
                    info!(loc = here!(), stream = %stream.title, "Stream ended!");

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
        index: &HashMap<u32, Livestream>,
    ) -> Option<(Livestream, StreamState)> {
        let stream_id = topic.strip_prefix("https://youtube.com/watch?v=")?;

        let stream = index.values().find(|s| s.url == stream_id)?;

        match &stream.state {
            StreamState::Scheduled => {
                error!("This should never happen.");
                None
            }
            StreamState::Live | StreamState::Ended => Some((stream.clone(), stream.state)),
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

            let _ = tokio::spawn(async move {
                if let Err(e) = Self::archive_channel(ctx_clone, channel, stream, log_clone).await {
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
                            .url(format!("https://youtube.com/watch?v={}", &stream.url))
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
        let channel_topic = format!("https://youtube.com/watch?v={}", stream.url);

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
