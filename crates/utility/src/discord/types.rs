use std::{
    collections::HashMap,
    ops::{Deref, DerefMut},
};

use anyhow::{anyhow, Context};
use chrono::{DateTime, Utc};
use lru::LruCache;
use rusqlite::Connection;
use serenity::{
    model::{
        channel::Message,
        guild::Member,
        id::{ChannelId, CommandId, EmojiId, GuildId, MessageId, UserId},
    },
    prelude::TypeMapKey,
    utils::Colour,
};
use songbird::tracks::{TrackHandle, TrackQueue};
use tokio::sync::{broadcast, mpsc, watch, Mutex};

use crate::{
    client_data_types,
    config::{EmojiStats, EntryEvent, LoadFromDatabase, Quote, Reminder, SaveToDatabase},
    discord::RegisteredInteraction,
    here,
    streams::{Livestream, StreamUpdate},
    wrap_type_aliases,
};

type Ctx = serenity::client::Context;

#[derive(Debug, Clone)]
pub enum MessageUpdate {
    Sent(Message),
    Edited(Message),
    Deleted(MessageId),
}

#[derive(Debug, Clone, Default)]
pub struct MusicData {
    pub queues: HashMap<GuildId, TrackQueue>,
    pub forced_songs: HashMap<GuildId, Option<TrackHandle>>,
    pub misc_data: HashMap<GuildId, MiscData>,
}

#[derive(Debug, Clone)]
pub struct MiscData {
    pub queue_is_looping: bool,
    pub volume: f32,
}

impl Default for MiscData {
    fn default() -> Self {
        Self {
            queue_is_looping: false,
            volume: 1.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TrackMetaData {
    pub added_by: UserId,
    pub added_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct TrackMetaDataFull {
    pub added_by: Member,
    pub added_at: DateTime<Utc>,
    pub member_colour: Option<Colour>,
}

impl TrackMetaData {
    pub async fn fetch_data(
        &self,
        ctx: &Ctx,
        guild_id: &GuildId,
    ) -> anyhow::Result<TrackMetaDataFull> {
        let member = guild_id.member(&ctx.http, self.added_by).await?;

        Ok(TrackMetaDataFull {
            member_colour: member.colour(&ctx.cache).await,
            added_by: member,
            added_at: self.added_at,
        })
    }
}

#[derive(Debug)]
pub enum CurrentTrack {
    None,
    InQueue(TrackHandle),
    Forced(TrackHandle),
}

#[derive(Debug)]
pub enum CurrentSource<'a> {
    None,
    Queue(&'a TrackQueue),
    Forced(&'a TrackHandle),
}

#[derive(Debug)]
pub enum QueueRemovalCondition {
    All,
    Duplicates,
    Indices(String),
    FromUser(UserId),
}

impl MusicData {
    pub fn get_current(&self, guild_id: &GuildId) -> CurrentTrack {
        if !self.is_guild_registered(guild_id) {
            return CurrentTrack::None;
        }

        if let Some(forced_track) = self.forced_songs.get(guild_id).unwrap() {
            return CurrentTrack::Forced(forced_track.clone());
        }

        if let Some(queued_track) = self.queues.get(guild_id).unwrap().current() {
            return CurrentTrack::InQueue(queued_track);
        }

        CurrentTrack::None
    }

    pub fn get_forced(&self, guild_id: &GuildId) -> Option<&TrackHandle> {
        if !self.is_guild_registered(guild_id) {
            return None;
        }

        self.forced_songs.get(guild_id).unwrap().as_ref()
    }

    pub fn get_source(&self, guild_id: &GuildId) -> CurrentSource<'_> {
        if !self.is_guild_registered(guild_id) {
            return CurrentSource::None;
        }

        if let Some(forced_track) = self.forced_songs.get(guild_id).unwrap() {
            return CurrentSource::Forced(forced_track);
        }

        let queue = self.queues.get(guild_id).unwrap();

        if !queue.is_empty() {
            return CurrentSource::Queue(queue);
        }

        CurrentSource::None
    }

    pub fn is_guild_registered(&self, guild_id: &GuildId) -> bool {
        self.queues.contains_key(guild_id)
    }

    pub fn register_guild(&mut self, guild_id: GuildId) {
        if self.queues.contains_key(&guild_id) {
            return;
        }

        self.queues.insert(guild_id, TrackQueue::new());
        self.forced_songs.insert(guild_id, None);
        self.misc_data.insert(guild_id, MiscData::default());
    }

    pub fn deregister_guild(&mut self, guild_id: &GuildId) -> anyhow::Result<()> {
        self.stop(guild_id)?;

        self.queues.remove(guild_id);
        self.forced_songs.remove(guild_id);
        self.misc_data.remove(guild_id);

        Ok(())
    }

    pub fn stop(&self, guild_id: &GuildId) -> anyhow::Result<()> {
        if let Some(queue) = self.queues.get(guild_id) {
            queue.stop();
        }

        if let Some(Some(forced_song)) = self.forced_songs.get(guild_id) {
            forced_song.stop()?;
        }

        Ok(())
    }

    pub fn set_volume(&mut self, guild_id: &GuildId, volume: f32) -> anyhow::Result<()> {
        if let Some(misc_data) = self.misc_data.get_mut(guild_id) {
            misc_data.volume = volume;
        }

        if let Some(Some(forced_song)) = self.forced_songs.get(guild_id) {
            forced_song.set_volume(volume)?;
        }

        if let Some(queue) = self.queues.get(guild_id) {
            queue.modify_queue(|q| q.iter_mut().try_for_each(|t| t.set_volume(volume)))?;
        }

        Ok(())
    }
}

pub use tokio_util::sync::CancellationToken;

wrap_type_aliases!(
    Quotes = Vec<Quote>,
    DbHandle = Mutex<rusqlite::Connection>,
    EmojiUsage = HashMap<EmojiId, EmojiStats>,
    StreamIndex = watch::Receiver<HashMap<String, Livestream>>,
    StreamUpdateTx = broadcast::Sender<StreamUpdate>,
    ReminderSender =  mpsc::Receiver<EntryEvent<u64, Reminder>>,
    MessageSender = broadcast::Sender<MessageUpdate>,
    ClaimedChannels = HashMap<ChannelId, (Livestream, CancellationToken)>,
    RegisteredInteractions = HashMap<GuildId, HashMap<CommandId, RegisteredInteraction>>
);

pub type NotifiedStreamsCache = LruCache<String, ()>;

client_data_types!(
    Quotes,
    DbHandle,
    MusicData,
    EmojiUsage,
    StreamIndex,
    StreamUpdateTx,
    ReminderSender,
    MessageSender,
    TrackMetaData,
    ClaimedChannels,
    RegisteredInteractions
);

impl DerefMut for Quotes {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl DerefMut for EmojiUsage {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl DerefMut for ClaimedChannels {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl DerefMut for RegisteredInteractions {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Default for ClaimedChannels {
    fn default() -> Self {
        Self(HashMap::new())
    }
}

impl Default for RegisteredInteractions {
    fn default() -> Self {
        Self(HashMap::new())
    }
}

impl From<Vec<Quote>> for Quotes {
    fn from(vec: Vec<Quote>) -> Self {
        Self(vec)
    }
}

impl SaveToDatabase for Quotes {
    fn save_to_database(&self, handle: &Connection) -> anyhow::Result<()> {
        let mut stmt = handle.prepare_cached("INSERT OR REPLACE INTO Quotes (quote) VALUES (?)")?;

        let tx = handle.unchecked_transaction()?;

        for quote in &self.0 {
            stmt.execute([quote])?;
        }

        tx.commit()?;
        Ok(())
    }
}

impl LoadFromDatabase for Quotes {
    type Item = Quote;
    type ItemContainer = Vec<Quote>;

    fn load_from_database(handle: &Connection) -> anyhow::Result<Self::ItemContainer> {
        let mut stmt = handle
            .prepare("SELECT quote FROM Quotes")
            .context(here!())?;

        let results = stmt.query_and_then([], |row| -> anyhow::Result<Self::Item> {
            row.get(0).map_err(|e| anyhow!(e))
        })?;

        results.collect()
    }
}

impl SaveToDatabase for EmojiUsage {
    fn save_to_database(&self, handle: &Connection) -> anyhow::Result<()> {
        let mut stmt = handle.prepare_cached(
            "INSERT OR REPLACE INTO emoji_usage (emoji_id, text_count, reaction_count) VALUES (?, ?, ?)",
        )?;

        let tx = handle.unchecked_transaction()?;

        for (emoji, count) in &self.0 {
            stmt.execute([emoji.as_u64(), &count.text_count, &count.reaction_count])?;
        }

        tx.commit()?;
        Ok(())
    }
}

impl From<Vec<(EmojiId, EmojiStats)>> for EmojiUsage {
    fn from(vec: Vec<(EmojiId, EmojiStats)>) -> Self {
        Self(vec.into_iter().collect())
    }
}

impl LoadFromDatabase for EmojiUsage {
    type Item = (EmojiId, EmojiStats);
    type ItemContainer = Vec<Self::Item>;

    fn load_from_database(handle: &Connection) -> anyhow::Result<Self::ItemContainer>
    where
        Self: Sized,
    {
        let mut stmt = handle
            .prepare("SELECT emoji_id, text_count, reaction_count FROM emoji_usage")
            .context(here!())?;

        let results = stmt.query_and_then([], |row| -> anyhow::Result<(EmojiId, EmojiStats)> {
            Ok((
                EmojiId(row.get("emoji_id").context(here!())?),
                EmojiStats {
                    text_count: row.get("text_count").context(here!())?,
                    reaction_count: row.get("reaction_count").context(here!())?,
                },
            ))
        })?;

        results.collect()
    }
}

impl LoadFromDatabase for NotifiedStreamsCache {
    type Item = String;
    type ItemContainer = Vec<Self::Item>;

    fn load_from_database(handle: &Connection) -> anyhow::Result<Self::ItemContainer>
    where
        Self::Item: Sized,
    {
        let mut stmt = handle
            .prepare("SELECT stream_id FROM NotifiedCache")
            .context(here!())?;

        let results = stmt.query_and_then([], |row| -> anyhow::Result<Self::Item> {
            row.get("stream_id").map_err(|e| anyhow!(e))
        })?;

        results.collect()
    }
}

impl SaveToDatabase for NotifiedStreamsCache {
    fn save_to_database(&self, handle: &Connection) -> anyhow::Result<()> {
        let mut stmt =
            handle.prepare_cached("INSERT OR REPLACE INTO NotifiedCache (stream_id) VALUES (?)")?;

        let tx = handle.unchecked_transaction()?;

        for (stream_id, _) in self {
            stmt.execute([stream_id])?;
        }

        tx.commit()?;
        Ok(())
    }
}
