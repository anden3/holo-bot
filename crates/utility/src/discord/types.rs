use std::{
    collections::{HashMap, HashSet},
    ops::{Deref, DerefMut},
};

use anyhow::Context;
use holodex::model::id::VideoId;
use rusqlite::ToSql;
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
        DatabaseHandle, DatabaseOperations, EmojiStats, EmojiUsageSource, EntryEvent, Quote,
        Reminder,
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

pub type NotifiedStreamsCache = lru::LruCache<VideoId, ()>;

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

#[allow(clippy::derivable_impls)]
impl Default for RegisteredInteractions {
    fn default() -> Self {
        Self(HashMap::new())
    }
}

impl DatabaseOperations<'_, Quote> for Vec<Quote> {
    type LoadItemContainer = Self;

    const TABLE_NAME: &'static str = "Quotes";
    const COLUMNS: &'static [(&'static str, &'static str, Option<&'static str>)] =
        &[("quote", "BLOB", Some("NOT NULL"))];
    const TRUNCATE_TABLE: bool = true;

    fn into_row(quote: Quote) -> Vec<Box<dyn ToSql>> {
        vec![Box::new(quote)]
    }

    fn from_row(row: &rusqlite::Row) -> anyhow::Result<Quote> {
        row.get("quote").context(here!())
    }
}

impl DatabaseOperations<'_, (EmojiId, EmojiStats)> for HashMap<EmojiId, EmojiStats> {
    type LoadItemContainer = Self;

    const TABLE_NAME: &'static str = "EmojiUsage";
    const COLUMNS: &'static [(&'static str, &'static str, Option<&'static str>)] = &[
        ("emoji_id", "INTEGER", Some("PRIMARY KEY")),
        ("text_count", "INTEGER", Some("NOT NULL")),
        ("reaction_count", "INTEGER", Some("NOT NULL")),
    ];

    fn into_row((emoji, stats): (EmojiId, EmojiStats)) -> Vec<Box<dyn ToSql>> {
        vec![
            Box::new(*emoji.as_u64()),
            Box::new(stats.text_count),
            Box::new(stats.reaction_count),
        ]
    }

    fn from_row(row: &rusqlite::Row) -> anyhow::Result<(EmojiId, EmojiStats)> {
        Ok((
            EmojiId(row.get("emoji_id").context(here!())?),
            EmojiStats {
                text_count: row.get("text_count").context(here!())?,
                reaction_count: row.get("reaction_count").context(here!())?,
            },
        ))
    }
}

impl DatabaseOperations<'_, (StickerId, u64)> for HashMap<StickerId, u64> {
    type LoadItemContainer = Self;

    const TABLE_NAME: &'static str = "StickerUsage";
    const COLUMNS: &'static [(&'static str, &'static str, Option<&'static str>)] = &[
        ("sticker_id", "INTEGER", Some("PRIMARY KEY")),
        ("count", "INTEGER", Some("NOT NULL")),
    ];

    fn into_row((sticker, count): (StickerId, u64)) -> Vec<Box<dyn ToSql>> {
        vec![Box::new(*sticker.as_u64()), Box::new(count)]
    }

    fn from_row(row: &rusqlite::Row) -> anyhow::Result<(StickerId, u64)> {
        Ok((
            StickerId(row.get("sticker_id").context(here!())?),
            row.get("count").context(here!())?,
        ))
    }
}

impl DatabaseOperations<'_, VideoId> for HashSet<VideoId> {
    type LoadItemContainer = Vec<VideoId>;

    const TRUNCATE_TABLE: bool = true;
    const TABLE_NAME: &'static str = "NotifiedCache";
    const COLUMNS: &'static [(&'static str, &'static str, Option<&'static str>)] =
        &[("stream_id", "TEXT", Some("NOT NULL"))];

    fn into_row(item: VideoId) -> Vec<Box<dyn ToSql>> {
        vec![Box::new(item.to_string())]
    }

    fn from_row(row: &rusqlite::Row) -> anyhow::Result<VideoId> {
        row.get::<_, String>("stream_id")
            .map(|s| s.parse().context(here!()))?
    }
}
