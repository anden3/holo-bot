use std::collections::HashMap;

use serenity::model::{id::StickerId, prelude::Sticker};
use tokio::sync::oneshot;

use super::prelude::*;

#[derive(Debug, Clone, Copy, ChoiceParameter)]
pub(crate) enum StickerSortingCriteria {
    #[name = "Usage"]
    Usage,
    #[name = "Created at"]
    CreatedAt,
}

impl Default for StickerSortingCriteria {
    fn default() -> Self {
        Self::Usage
    }
}

#[derive(Debug, Clone, Copy, ChoiceParameter)]
pub(crate) enum StickerOrder {
    #[name = "Ascending"]
    Ascending,
    #[name = "Descending"]
    Descending,
}

impl Default for StickerOrder {
    fn default() -> Self {
        Self::Descending
    }
}

#[poise::command(
    slash_command,
    prefix_command,
    track_edits,
    check = "sticker_tracking_enabled",
    required_permissions = "VIEW_AUDIT_LOG"
)]
/// Shows the most used stickers in this server.
pub(crate) async fn sticker_usage(
    ctx: Context<'_>,

    #[description = "How the stickers should be sorted."] sort_by: StickerSortingCriteria,

    #[description = "What order to display the stickers in."] order: Option<StickerOrder>,
    #[description = "Filter stickers by name."] search: Option<String>,
    #[description = "Number of stickers to fetch."] count: Option<usize>,
) -> anyhow::Result<()> {
    ctx.defer().await?;

    let order = order.unwrap_or_default();

    let guild_id = match ctx.guild_id() {
        Some(guild_id) => guild_id,
        None => return Err(anyhow!("This command can only be used in a guild.")),
    };

    let mut stickers = {
        let guild_stickers = guild_id
            .stickers(&ctx)
            .await?
            .into_iter()
            .map(|e| (e.id, e))
            .collect::<HashMap<StickerId, Sticker>>();

        let sticker_response = {
            let (sticker_request, sticker_response) = oneshot::channel();

            let data = ctx.data();
            let read_lock = data.data.read().await;

            read_lock
                .sticker_usage_counter
                .as_ref()
                .ok_or_else(|| anyhow!("Failed to reach sticker usage tracker!"))?
                .send(StickerUsageEvent::GetUsage(sticker_request))
                .await?;

            sticker_response
        };

        let sticker_map = sticker_response.await?;

        guild_stickers
            .into_iter()
            .map(|(i, e)| {
                (
                    e,
                    match sticker_map.get(&i) {
                        Some(s) => *s,
                        None => 0,
                    },
                )
            })
            .collect::<Vec<_>>()
    };

    stickers = match search {
        Some(ref search) => stickers
            .into_iter()
            .filter(|(e, _)| e.name.to_lowercase().contains(search))
            .collect(),
        None => stickers,
    };

    match sort_by {
        StickerSortingCriteria::Usage => match order {
            StickerOrder::Ascending => stickers.sort_unstable_by(|(_, a), (_, b)| a.cmp(b)),
            StickerOrder::Descending => stickers.sort_unstable_by(|(_, a), (_, b)| b.cmp(a)),
        },
        StickerSortingCriteria::CreatedAt => match order {
            StickerOrder::Ascending => stickers
                .sort_unstable_by(|(a, _), (b, _)| a.id.created_at().cmp(&b.id.created_at())),
            StickerOrder::Descending => stickers
                .sort_unstable_by(|(a, _), (b, _)| b.id.created_at().cmp(&a.id.created_at())),
        },
    }

    let top_stickers = stickers
        .into_iter()
        .take(count.unwrap_or(100))
        .collect::<Vec<_>>();

    let title = format!(
        "{} stickers{}",
        match (sort_by, order) {
            (StickerSortingCriteria::Usage, StickerOrder::Ascending) => "Least used",
            (StickerSortingCriteria::Usage, StickerOrder::Descending) => "Most used",
            (StickerSortingCriteria::CreatedAt, StickerOrder::Ascending) => "Oldest",
            (StickerSortingCriteria::CreatedAt, StickerOrder::Descending) => "Newest",
        },
        match &search {
            Some(search) => format!(" matching \"*{search}*\""),
            None => String::new(),
        },
    );

    PaginatedList::new()
        .title(&title)
        .data(&top_stickers)
        .layout(PageLayout::Chunked {
            chunk_size: 10,
            chunks_per_page: 3,
        })
        .params(&[&sort_by.to_string()])
        .format(Box::new(|(e, c), params| match params[0].as_str() {
            "Usage" => {
                format!(
                    "[{}]({}) {c}\r\n",
                    e.name,
                    e.image_url().unwrap_or_default(),
                )
            }
            "Created at" => format!(
                "[{}]({}) <t:{}:f>\r\n",
                e.name,
                e.image_url().unwrap_or_default(),
                e.id.created_at().timestamp()
            ),
            s => {
                error!("Invalid sort qualifier: '{s}'!");
                "Invalid sort qualifier.".to_string()
            }
        }))
        .display(ctx)
        .await?;

    Ok(())
}

async fn sticker_tracking_enabled(ctx: Context<'_>) -> anyhow::Result<bool> {
    Ok(ctx.data().config.emoji_tracking.enabled)
}
