use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serenity::model::{guild::Emoji, id::EmojiId};
use strum::{Display, EnumIter};
use tokio::sync::oneshot;
use utility::config::EmojiStats;

use super::prelude::*;

#[derive(Debug, Serialize, Deserialize, EnumIter, Display, PartialEq, Eq, Hash, Clone, Copy)]
enum EmojiSortingCriteria {
    Usage,
    CreatedAt,
}

impl Default for EmojiSortingCriteria {
    fn default() -> Self {
        EmojiSortingCriteria::Usage
    }
}

#[derive(Debug, Serialize, Deserialize, EnumIter, PartialEq, Eq, Hash, Clone, Copy)]
enum EmojiOrder {
    Ascending,
    Descending,
}

#[derive(Debug, Serialize, Deserialize, EnumIter, Display, PartialEq, Eq, Hash, Clone, Copy)]
enum EmojiUsage {
    InMessages,
    AsReactions,
}

#[derive(Debug, Serialize, Deserialize, EnumIter, PartialEq, Eq, Hash, Clone, Copy)]
enum EmojiType {
    Normal,
    Animated,
}

interaction_setup! {
    name = "emoji_usage",
    group = "utility",
    description = "Shows the most used emotes in this server",
    enabled_if = |config| config.emoji_tracking.enabled,
    options = {
        //! How the emotes should be sorted.
        sort_by: EmojiSortingCriteria,

        //! What order to display the emotes in.
        order: Option<EmojiOrder>,
        //! If only text or reaction usages should be shown.
        usage: Option<EmojiUsage>,
        //! If only normal or animated emotes should be shown.
        emoji_type: Option<EmojiType>,
        //! Filter emotes by name.
        search: Option<String>,
        //! Number of emotes to fetch.
        count: Option<Integer>,
    },
    restrictions = [
        allowed_roles = [
            "Admin",
            "Moderator",
            "Moderator (JP)",
            "Community Staff"
        ]
    ]
}

#[interaction_cmd]
pub async fn emoji_usage(
    ctx: &Ctx,
    interaction: &ApplicationCommandInteraction,
    config: &Config,
) -> anyhow::Result<()> {
    parse_interaction_options!(
        interaction.data,
        [
            sort_by: EmojiSortingCriteria,
            order: EmojiOrder = EmojiOrder::Descending,
            usage: Option<EmojiUsage>,
            emoji_type: Option<EmojiType>,
            search: Option<String>,
            count: Option<usize>,
        ]
    );
    show_deferred_response(interaction, ctx, false).await?;

    let mut emotes = {
        let guild_emotes = interaction
            .guild_id
            .unwrap()
            .emojis(&ctx.http)
            .await?
            .into_iter()
            .map(|e| (e.id, e))
            .collect::<HashMap<EmojiId, Emoji>>();

        let emoji_response = {
            let data = ctx.data.read().await;

            let (emoji_request, emoji_response) = oneshot::channel();

            data.get::<EmojiUsageSender>()
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
            Some(search) => format!(" matching \"*{}*\"", search),
            None => String::new(),
        },
        if sort_by == EmojiSortingCriteria::Usage {
            match usage {
                Some(EmojiUsage::InMessages) => " (Not counting reactions)",
                Some(EmojiUsage::AsReactions) => " (Only counting reactions)",
                None => "",
            }
        } else {
            ""
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
                        "Text" => format!("{} {}\r\n", Mention::from(e), c.text_count),
                        "Reactions" => format!("{} {}\r\n", Mention::from(e), c.reaction_count),
                        _ => "Invalid usage.".to_string(),
                    }
                } else {
                    format!(
                        "{} {} ({}T, {}R)\r\n",
                        Mention::from(e),
                        c.total(),
                        c.text_count,
                        c.reaction_count
                    )
                }
            }
            "Created at" => format!(
                "{} <t:{}:f>\r\n",
                Mention::from(e),
                e.id.created_at().timestamp()
            ),
            s => {
                error!("Invalid sort qualifier: '{}'!", s);
                "Invalid sort qualifier.".to_string()
            }
        }))
        .display(ctx, interaction)
        .await?;

    Ok(())
}
