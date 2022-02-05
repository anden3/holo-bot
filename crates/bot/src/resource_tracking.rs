use std::collections::HashMap;

use anyhow::Context;
use serenity::model::id::{EmojiId, StickerId};
use tokio::sync::mpsc;
use tracing::{error, instrument};
use utility::{
    config::{Database, DatabaseOperations, EmojiStats},
    discord::{EmojiUsageEvent, StickerUsageEvent},
    here,
};

#[instrument(skip(database, emojis))]
pub async fn emoji_tracker(
    database: &Database,
    mut emojis: mpsc::Receiver<EmojiUsageEvent>,
) -> anyhow::Result<()> {
    let mut emoji_usage: HashMap<EmojiId, EmojiStats> = {
        let handle = database.get_handle().context(here!())?;

        handle
            .rename_table("emoji_usage", "EmojiUsage")
            .context(here!())?;

        HashMap::<EmojiId, EmojiStats>::create_table(&handle).context(here!())?;
        HashMap::<EmojiId, EmojiStats>::load_from_database(&handle).context(here!())?
    };

    {
        let handle = database.get_handle().context(here!())?;

        handle
            .create_table(
                "EmojiUsageHistory",
                &[
                    ("emoji_id", "INTEGER", Some("PRIMARY KEY")),
                    ("date", "TEXT", Some("NOT NULL")),
                    ("text_count", "INTEGER", Some("NOT NULL")),
                    ("reaction_count", "INTEGER", Some("NOT NULL")),
                ],
            )
            .context(here!())?;
    }

    while let Some(event) = emojis.recv().await {
        match event {
            EmojiUsageEvent::Used { resources, usage } => {
                for id in resources {
                    let mut count = emoji_usage.entry(id).or_insert_with(EmojiStats::default);
                    count += usage;
                }
            }
            EmojiUsageEvent::GetUsage(sender) => {
                if sender.send(emoji_usage.clone()).is_err() {
                    error!("Failed to send emoji usage!");
                    continue;
                }
            }
            EmojiUsageEvent::Terminate => {
                let db_handle = database.get_handle().context(here!())?;
                emoji_usage.save_to_database(&db_handle).context(here!())?;
                break;
            }
        }
    }

    Ok(())
}

#[instrument(skip(database, stickers))]
pub async fn sticker_tracker(
    database: &Database,
    mut stickers: mpsc::Receiver<StickerUsageEvent>,
) -> anyhow::Result<()> {
    let mut sticker_usage: HashMap<StickerId, u64> = {
        let db_handle = database.get_handle()?;

        HashMap::<StickerId, u64>::create_table(&db_handle)?;
        HashMap::<StickerId, u64>::load_from_database(&db_handle)?
    };

    while let Some(event) = stickers.recv().await {
        match event {
            StickerUsageEvent::Used { resources, .. } => {
                for id in resources {
                    let count = sticker_usage.entry(id).or_insert(0);
                    *count += 1;
                }
            }
            StickerUsageEvent::GetUsage(sender) => {
                if sender.send(sticker_usage.clone()).is_err() {
                    error!("Failed to send emoji usage!");
                    continue;
                }
            }
            StickerUsageEvent::Terminate => {
                let db_handle = database.get_handle()?;
                sticker_usage.save_to_database(&db_handle)?;
                break;
            }
        }
    }

    Ok(())
}
