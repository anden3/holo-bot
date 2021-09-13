use std::{
    collections::HashMap,
    ops::{Deref, DerefMut},
};

use anyhow::{anyhow, Context};
use lru::LruCache;
use rusqlite::Connection;
use serenity::{
    model::{
        channel::Message,
        id::{ChannelId, CommandId, EmojiId, GuildId, MessageId},
    },
    prelude::TypeMapKey,
};
use tokio::sync::{broadcast, mpsc, watch, Mutex};

use crate::{
    client_data_types,
    config::{EmojiStats, EntryEvent, LoadFromDatabase, Quote, Reminder, SaveToDatabase},
    discord::RegisteredInteraction,
    here,
    streams::{Livestream, StreamUpdate},
    wrap_type_aliases,
};

#[derive(Debug, Clone)]
pub enum MessageUpdate {
    Sent(Message),
    Edited(Message),
    Deleted(MessageId),
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
    EmojiUsage,
    StreamIndex,
    StreamUpdateTx,
    ReminderSender,
    MessageSender,
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
