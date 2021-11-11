use std::{
    collections::HashMap,
    ops::{Deref, DerefMut},
};

use anyhow::{anyhow, Context};
use holodex::model::id::VideoId;
use lru::LruCache;
use serenity::{
    model::{
        channel::{Message, Reaction},
        id::{CommandId, EmojiId, GuildId, MessageId, StickerId},
    },
    prelude::TypeMapKey,
};
use tokio::sync::{broadcast, mpsc, oneshot, watch, Mutex};

use crate::{
    client_data_types,
    config::{
        DatabaseHandle, EmojiStats, EmojiUsageSource, EntryEvent, LoadFromDatabase, Quote,
        Reminder, SaveToDatabase,
    },
    here,
    streams::{Livestream, StreamUpdate},
    wrap_type_aliases,
};

use super::RegisteredInteraction;

#[derive(Debug, Clone)]
pub enum MessageUpdate {
    Sent(Message),
    Edited(Message),
    Deleted(MessageId),
}

#[derive(Debug, Clone)]
pub enum ReactionUpdate {
    Added(Reaction),
    Removed(Reaction),
}

pub use tokio_util::sync::CancellationToken;

wrap_type_aliases!(
    DbHandle = Mutex<DatabaseHandle>;
    StreamIndex = watch::Receiver<HashMap<VideoId, Livestream>>;
    StreamUpdateTx = broadcast::Sender<StreamUpdate>;
    ReminderSender =  mpsc::Sender<EntryEvent<u32, Reminder>>;
    MessageSender = broadcast::Sender<MessageUpdate>;
    ReactionSender = broadcast::Sender<ReactionUpdate>;
    EmojiUsageSender = mpsc::Sender<EmojiUsageEvent>;
    StickerUsageSender = mpsc::Sender<StickerUsageEvent>;

    mut Quotes = Vec<Quote>;
    mut EmojiUsage = HashMap<EmojiId, EmojiStats>;
    mut StickerUsage = HashMap<StickerId, u64>;
    mut RegisteredInteractions = HashMap<GuildId, HashMap<CommandId, RegisteredInteraction>>;
);

pub type NotifiedStreamsCache = LruCache<VideoId, ()>;

client_data_types!(
    Quotes,
    DbHandle,
    StreamIndex,
    StreamUpdateTx,
    ReminderSender,
    MessageSender,
    ReactionSender,
    EmojiUsageSender,
    StickerUsageSender,
    RegisteredInteractions
);

#[derive(Debug)]
pub enum ResourceUsageEvent<K, S, V> {
    Used { resources: Vec<K>, usage: S },
    GetUsage(oneshot::Sender<HashMap<K, V>>),
    Terminate,
}

pub type EmojiUsageEvent = ResourceUsageEvent<EmojiId, EmojiUsageSource, EmojiStats>;
pub type StickerUsageEvent = ResourceUsageEvent<StickerId, (), u64>;

impl Default for RegisteredInteractions {
    fn default() -> Self {
        Self(HashMap::new())
    }
}

impl SaveToDatabase for Quotes {
    fn save_to_database(self, handle: &DatabaseHandle) -> anyhow::Result<()> {
        match handle {
            DatabaseHandle::SQLite(h) => {
                let mut stmt =
                    h.prepare_cached("INSERT OR REPLACE INTO Quotes (quote) VALUES (?)")?;

                let tx = h.unchecked_transaction()?;

                for quote in &self.0 {
                    stmt.execute([quote])?;
                }

                tx.commit()?;
            }
        }

        Ok(())
    }
}

impl LoadFromDatabase for Quotes {
    type Item = Quote;
    type ItemContainer = Vec<Quote>;

    fn load_from_database(handle: &DatabaseHandle) -> anyhow::Result<Self::ItemContainer> {
        match handle {
            DatabaseHandle::SQLite(h) => {
                let mut stmt = h.prepare("SELECT quote FROM Quotes").context(here!())?;

                let results = stmt.query_and_then([], |row| -> anyhow::Result<Self::Item> {
                    row.get(0).map_err(|e| anyhow!(e))
                })?;

                results.collect()
            }
        }
    }
}

impl SaveToDatabase for EmojiUsage {
    fn save_to_database(self, handle: &DatabaseHandle) -> anyhow::Result<()> {
        match handle {
            DatabaseHandle::SQLite(h) => {
                let mut stmt = h.prepare_cached(
            "INSERT OR REPLACE INTO emoji_usage (emoji_id, text_count, reaction_count) VALUES (?, ?, ?)",
        )?;

                let tx = h.unchecked_transaction()?;

                for (emoji, count) in &self.0 {
                    stmt.execute([emoji.as_u64(), &count.text_count, &count.reaction_count])?;
                }

                tx.commit()?;
            }
        }

        Ok(())
    }
}

impl SaveToDatabase for StickerUsage {
    fn save_to_database(self, handle: &DatabaseHandle) -> anyhow::Result<()> {
        match handle {
            DatabaseHandle::SQLite(h) => {
                let mut stmt = h.prepare_cached(
                    "INSERT OR REPLACE INTO StickerUsage (sticker_id, count) VALUES (?, ?)",
                )?;

                let tx = h.unchecked_transaction()?;

                for (sticker, count) in &self.0 {
                    stmt.execute([sticker.as_u64(), count])?;
                }

                tx.commit()?;
            }
        }

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

    fn load_from_database(handle: &DatabaseHandle) -> anyhow::Result<Self::ItemContainer>
    where
        Self::Item: Sized,
    {
        match handle {
            DatabaseHandle::SQLite(h) => {
                let mut stmt = h
                    .prepare("SELECT emoji_id, text_count, reaction_count FROM emoji_usage")
                    .context(here!())?;

                let results = stmt.query_and_then([], |row| -> anyhow::Result<Self::Item> {
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
    }
}

impl LoadFromDatabase for StickerUsage {
    type Item = (StickerId, u64);
    type ItemContainer = HashMap<StickerId, u64>;

    fn load_from_database(handle: &DatabaseHandle) -> anyhow::Result<Self::ItemContainer>
    where
        Self::Item: Sized,
    {
        match handle {
            DatabaseHandle::SQLite(h) => {
                let mut stmt = h
                    .prepare("SELECT sticker_id, count FROM StickerUsage")
                    .context(here!())?;

                let results = stmt.query_and_then([], |row| -> anyhow::Result<Self::Item> {
                    Ok((
                        StickerId(row.get("sticker_id").context(here!())?),
                        row.get("count").context(here!())?,
                    ))
                })?;

                results.collect()
            }
        }
    }
}

impl LoadFromDatabase for NotifiedStreamsCache {
    type Item = VideoId;
    type ItemContainer = Vec<Self::Item>;

    fn load_from_database(handle: &DatabaseHandle) -> anyhow::Result<Self::ItemContainer>
    where
        Self::Item: Sized,
    {
        match handle {
            DatabaseHandle::SQLite(h) => {
                let mut stmt = h
                    .prepare("SELECT stream_id FROM NotifiedCache")
                    .context(here!())?;

                let results = stmt.query_and_then([], |row| -> anyhow::Result<Self::Item> {
                    row.get::<_, String>("stream_id")
                        .map(|s| s.parse().context(here!()))
                        .map_err(|e| anyhow!(e))?
                })?;

                results.collect()
            }
        }
    }
}

impl SaveToDatabase for NotifiedStreamsCache {
    fn save_to_database(self, handle: &DatabaseHandle) -> anyhow::Result<()> {
        match handle {
            DatabaseHandle::SQLite(h) => {
                let mut stmt = h.prepare_cached(
                    "INSERT OR REPLACE INTO NotifiedCache (stream_id) VALUES (?)",
                )?;

                let tx = h.unchecked_transaction()?;

                for (stream_id, _) in self.into_iter() {
                    stmt.execute([stream_id.to_string()])?;
                }

                tx.commit()?;
            }
        }

        Ok(())
    }
}
