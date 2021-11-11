use commands::prelude::{EmojiUsage, EmojiUsageEvent, StickerUsage, StickerUsageEvent};
use tokio::sync::mpsc;
use tracing::{error, instrument};
use utility::config::{Database, EmojiStats, LoadFromDatabase, SaveToDatabase};

#[instrument(skip(database, emojis))]
pub async fn emoji_tracker(
    database: &Database,
    mut emojis: mpsc::Receiver<EmojiUsageEvent>,
) -> anyhow::Result<()> {
    let mut emoji_usage: EmojiUsage = {
        let db_handle = database.get_handle()?;
        EmojiUsage::load_from_database(&db_handle)?.into()
    };

    while let Some(event) = emojis.recv().await {
        match event {
            EmojiUsageEvent::Used { resources, usage } => {
                for id in resources {
                    let count = emoji_usage.entry(id).or_insert_with(EmojiStats::default);
                    count.add(usage);
                }
            }
            EmojiUsageEvent::GetUsage(sender) => {
                if sender.send(emoji_usage.clone()).is_err() {
                    error!("Failed to send emoji usage!");
                    continue;
                }
            }
            EmojiUsageEvent::Terminate => {
                let db_handle = database.get_handle()?;
                emoji_usage.save_to_database(&db_handle)?;
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
    let mut sticker_usage: StickerUsage = {
        let db_handle = database.get_handle()?;
        StickerUsage::load_from_database(&db_handle)?.into()
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
