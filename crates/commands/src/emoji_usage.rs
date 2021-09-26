use std::collections::HashMap;

use serenity::model::{guild::Emoji, id::EmojiId};
use utility::config::EmojiStats;

use super::prelude::*;

interaction_setup! {
    name = "emoji_usage",
    group = "utility",
    description = "Shows the most used emotes in this server",
    options = [
        //! What order to display the emotes in.
        req order: String = [
            "Ascending",
            "Descending",
        ],
        //! If only text or reaction usages should be shown.
        usage: String = [
            "In Messages": "Text",
            "Reactions": "Reactions",
        ],
        //! If only normal or animated emotes should be shown.
        emoji_type: String = [
            "Normal",
            "Animated",
        ],
        //! Filter emotes by name.
        search: String,
        //! Number of emotes to fetch.
        count: Integer,
    ],
    restrictions = [
        allowed_roles = [
            "Admin",
            "Moderator",
            "Moderator (JP)"
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
    interaction.data, [
        order: req String,
        usage: String,
        emoji_type: String,
        search: String,
        count: usize,
    ]);
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

        let data = ctx.data.read().await;
        let emoji_map = data.get::<EmojiUsage>().unwrap().0.clone();
        std::mem::drop(data);

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

    emotes = match emoji_type.as_deref() {
        Some("Normal") => emotes.into_iter().filter(|(e, _)| !e.animated).collect(),
        Some("Animated") => emotes.into_iter().filter(|(e, _)| e.animated).collect(),
        Some(_) | None => emotes,
    };

    emotes = match usage.as_deref() {
        Some("Text") => emotes
            .into_iter()
            .filter(|(_, c)| c.text_count > 0)
            .collect(),
        Some("Reactions") => emotes
            .into_iter()
            .filter(|(_, c)| c.reaction_count > 0)
            .collect(),
        Some(_) | None => emotes,
    };

    emotes = match search {
        Some(ref search) => emotes
            .into_iter()
            .filter(|(e, _)| e.name.to_lowercase().contains(search))
            .collect(),
        None => emotes,
    };

    match order.as_str() {
        "Ascending" => emotes.sort_unstable_by(|(_, a), (_, b)| match usage.as_deref() {
            Some("Text") => a.text_count.cmp(&b.text_count),
            Some("Reactions") => a.reaction_count.cmp(&b.reaction_count),
            Some(_) | None => a.cmp(b),
        }),
        "Descending" => emotes.sort_unstable_by(|(_, a), (_, b)| match usage.as_deref() {
            Some("Text") => b.text_count.cmp(&a.text_count),
            Some("Reactions") => b.reaction_count.cmp(&a.reaction_count),
            Some(_) | None => b.cmp(a),
        }),
        _ => return Err(anyhow!("Invalid ordering.").context(here!())),
    }

    let top_emotes = emotes
        .into_iter()
        .take(count.unwrap_or(100))
        .collect::<Vec<_>>();

    let title = format!(
        "{} used {}emotes{}{}",
        match order.as_str() {
            "Ascending" => "Least",
            "Descending" => "Most",
            _ => "",
        },
        match emoji_type.as_deref() {
            Some("Normal") => "static ",
            Some("Animated") => "animated ",
            Some(_) | None => "",
        },
        match &search {
            Some(search) => format!(" matching \"*{}*\"", search),
            None => String::new(),
        },
        match usage.as_deref() {
            Some("Text") => " (Not counting reactions)",
            Some("Reactions") => " (Only counting reactions)",
            Some(_) | None => "",
        }
    );

    PaginatedList::new()
        .title(&title)
        .data(&top_emotes)
        .layout(PageLayout::Chunked {
            chunk_size: 10,
            chunks_per_page: 3,
        })
        .params(&[&usage.unwrap_or_default()])
        .format(Box::new(|(e, c), params| {
            if !params[0].is_empty() {
                match params[0].as_str() {
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
        }))
        .display(ctx, interaction)
        .await?;

    Ok(())
}
