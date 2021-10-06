use commands::prelude::{EmojiUsage, EmojiUsageEvent};
use tokio::sync::mpsc;
use tracing::{error, instrument};
use utility::config::{Database, EmojiStats, LoadFromDatabase, SaveToDatabase};

#[instrument(skip(database, emojis))]
pub async fn tracker(
    database: &Database,
    mut emojis: mpsc::Receiver<EmojiUsageEvent>,
) -> anyhow::Result<()> {
    let mut emoji_usage: EmojiUsage = {
        let db_handle = database.get_handle()?;
        EmojiUsage::load_from_database(&db_handle)?.into()
    };

    while let Some(event) = emojis.recv().await {
        match event {
            EmojiUsageEvent::Used { emojis, usage } => {
                for id in emojis {
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
