use std::{collections::HashMap, fmt::Display};

use serenity::model::{guild::Emoji, id::EmojiId};
use tokio::sync::oneshot;
use utility::config::EmojiStats;

use crate::paginated_list::PageLayout;

use super::prelude::*;

#[derive(Debug, Clone, Copy, ChoiceParameter)]
pub(crate) enum EmojiSortingCriteria {
    #[name = "Usage"]
    Usage,
    #[name = "Created at"]
    CreatedAt,
}

impl Default for EmojiSortingCriteria {
    fn default() -> Self {
        Self::Usage
    }
}

impl Display for EmojiSortingCriteria {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Usage => write!(f, "Usage"),
            Self::CreatedAt => write!(f, "Created at"),
        }
    }
}

#[derive(Debug, Clone, Copy, ChoiceParameter)]
pub(crate) enum EmojiOrder {
    #[name = "Ascending"]
    Ascending,
    #[name = "Descending"]
    Descending,
}

impl Default for EmojiOrder {
    fn default() -> Self {
        Self::Descending
    }
}

#[derive(Debug, Clone, Copy, ChoiceParameter)]
pub(crate) enum EmojiUsage {
    #[name = "In messages"]
    InMessages,
    #[name = "As reactions"]
    AsReactions,
}

impl Display for EmojiUsage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InMessages => write!(f, "In messages"),
            Self::AsReactions => write!(f, "As reactions"),
        }
    }
}

#[derive(Debug, Clone, Copy, ChoiceParameter)]
pub(crate) enum EmojiType {
    #[name = "Normal"]
    Normal,
    #[name = "Animated"]
    Animated,
}

#[poise::command(
    slash_command,
    prefix_command,
    track_edits,
    check = "emoji_tracking_enabled",
    required_permissions = "VIEW_AUDIT_LOG"
)]
/// Shows the most used custom emotes in this server.
pub(crate) async fn emoji_usage(
    ctx: Context<'_>,

    #[description = "How the emotes should be sorted."] sort_by: EmojiSortingCriteria,

    #[description = "What order to display the emotes in."] order: Option<EmojiOrder>,
    #[description = "If only text or reaction usages should be shown."] usage: Option<EmojiUsage>,
    #[description = "If only normal or animated emotes should be shown."] emoji_type: Option<
        EmojiType,
    >,
    #[description = "Filter emotes by name."] search: Option<String>,
    #[description = "Number of emotes to fetch."] count: Option<usize>,
) -> anyhow::Result<()> {
    ctx.defer().await?;

    let guild_id = match ctx.guild_id() {
        Some(guild_id) => guild_id,
        None => return Err(anyhow!("This command can only be used in a guild.")),
    };

    let order = order.unwrap_or_default();

    let mut emotes = {
        let guild_emotes = guild_id
            .emojis(&ctx.discord().http)
            .await?
            .into_iter()
            .map(|e| (e.id, e))
            .collect::<HashMap<EmojiId, Emoji>>();

        let emoji_response = {
            let (emoji_request, emoji_response) = oneshot::channel();

            let data = ctx.data();
            let read_lock = data.data.read().await;

            read_lock
                .emoji_usage_counter
                .as_ref()
                .ok_or_else(|| anyhow!("Failed to reach emoji usage tracker!"))?
                .send(EmojiUsageEvent::GetUsage(emoji_request))
                .await?;

            emoji_response
        };

        let emoji_map = emoji_response.await?;

        guild_emotes
            .into_iter()
            .map(|(i, e)| {
                (
                    e,
                    match emoji_map.get(&i) {
                        Some(s) => *s,
                        None => EmojiStats::default(),
                    },
                )
            })
            .collect::<Vec<_>>()
    };

    emotes = match emoji_type {
        Some(EmojiType::Normal) => emotes.into_iter().filter(|(e, _)| !e.animated).collect(),
        Some(EmojiType::Animated) => emotes.into_iter().filter(|(e, _)| e.animated).collect(),
        None => emotes,
    };

    emotes = match usage {
        Some(EmojiUsage::InMessages) => emotes
            .into_iter()
            .filter(|(_, c)| c.text_count > 0)
            .collect(),
        Some(EmojiUsage::AsReactions) => emotes
            .into_iter()
            .filter(|(_, c)| c.reaction_count > 0)
            .collect(),
        None => emotes,
    };

    emotes = match search {
        Some(ref search) => emotes
            .into_iter()
            .filter(|(e, _)| e.name.to_lowercase().contains(search))
            .collect(),
        None => emotes,
    };

    match sort_by {
        EmojiSortingCriteria::Usage => match order {
            EmojiOrder::Ascending => emotes.sort_unstable_by(|(_, a), (_, b)| match usage {
                Some(EmojiUsage::InMessages) => a.text_count.cmp(&b.text_count),
                Some(EmojiUsage::AsReactions) => a.reaction_count.cmp(&b.reaction_count),
                None => a.cmp(b),
            }),
            EmojiOrder::Descending => emotes.sort_unstable_by(|(_, a), (_, b)| match usage {
                Some(EmojiUsage::InMessages) => b.text_count.cmp(&a.text_count),
                Some(EmojiUsage::AsReactions) => b.reaction_count.cmp(&a.reaction_count),
                None => b.cmp(a),
            }),
        },
        EmojiSortingCriteria::CreatedAt => {
            match order {
                EmojiOrder::Ascending => emotes
                    .sort_unstable_by(|(a, _), (b, _)| a.id.created_at().cmp(&b.id.created_at())),
                EmojiOrder::Descending => emotes
                    .sort_unstable_by(|(a, _), (b, _)| b.id.created_at().cmp(&a.id.created_at())),
            }
        }
    }

    let top_emotes = emotes
        .into_iter()
        .take(count.unwrap_or(100))
        .collect::<Vec<_>>();

    let title = format!(
        "{} {}emotes{}{}",
        match (sort_by, order) {
            (EmojiSortingCriteria::Usage, EmojiOrder::Ascending) => "Least used",
            (EmojiSortingCriteria::Usage, EmojiOrder::Descending) => "Most used",
            (EmojiSortingCriteria::CreatedAt, EmojiOrder::Ascending) => "Oldest",
            (EmojiSortingCriteria::CreatedAt, EmojiOrder::Descending) => "Newest",
        },
        match emoji_type {
            Some(EmojiType::Normal) => "static ",
            Some(EmojiType::Animated) => "animated ",
            None => "",
        },
        match &search {
            Some(search) => format!(" matching \"*{search}*\""),
            None => String::new(),
        },
        match (sort_by, usage) {
            (EmojiSortingCriteria::Usage, Some(EmojiUsage::InMessages)) =>
                " (Not counting reactions)",
            (EmojiSortingCriteria::Usage, Some(EmojiUsage::AsReactions)) =>
                " (Only counting reactions)",
            _ => "",
        }
    );

    PaginatedList::new()
        .title(&title)
        .data(&top_emotes)
        .layout(PageLayout::Chunked {
            chunk_size: 10,
            chunks_per_page: 3,
        })
        .params(&[
            &sort_by.to_string(),
            &usage.map(|u| u.to_string()).unwrap_or_default(),
        ])
        .format(Box::new(|(e, c), params| match params[0].as_str() {
            "Usage" => {
                if !params[1].is_empty() {
                    match params[1].as_str() {
                        "Text" => format!("{} {}\r\n", Mention::from(e.id), c.text_count),
                        "Reactions" => format!("{} {}\r\n", Mention::from(e.id), c.reaction_count),
                        _ => "Invalid usage.".to_string(),
                    }
                } else {
                    format!(
                        "{} {} ({}T, {}R)\r\n",
                        Mention::from(e.id),
                        c.total(),
                        c.text_count,
                        c.reaction_count
                    )
                }
            }
            "Created at" => format!(
                "{} <t:{}:f>\r\n",
                Mention::from(e.id),
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

async fn emoji_tracking_enabled(ctx: Context<'_>) -> anyhow::Result<bool> {
    Ok(ctx.data().config.emoji_tracking.enabled)
}
