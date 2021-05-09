use std::collections::HashMap;

use serenity::model::{guild::Emoji, id::EmojiId};

use super::prelude::*;

interaction_setup! {
    name = "emoji_usage",
    description = "Shows the most used emotes in this server",
    options = [
        //! What order to display the emotes in.
        req order: String = [
            "Ascending": "Ascending",
            "Descending": "Descending",
        ],
        //! If only text or reaction usages should be shown.
        usage: String = [
            "In Messages": "Text",
            "Reactions": "Reactions",
        ],
        //! If only normal or animated emotes should be shown.
        emoji_type: String = [
            "Normal": "Normal",
            "Animated": "Animated",
        ],
    ],
}

#[interaction_cmd]
#[allowed_roles("Admin", "Moderator", "Moderator (JP)")]
pub async fn emoji_usage(ctx: &Ctx, interaction: &Interaction) -> anyhow::Result<()> {
    parse_interaction_options!(
    interaction.data.as_ref().unwrap(), [
        order: req String,
        usage: String,
        emoji_type: String,
    ]);
    show_deferred_response(&interaction, &ctx).await?;

    let data = ctx.data.read().await;
    let emoji_map = data.get::<EmojiUsage>().unwrap().0.clone();
    std::mem::drop(data);

    let guild_emotes = interaction
        .guild_id
        .emojis(&ctx.http)
        .await?
        .into_iter()
        .map(|e| (e.id, e))
        .collect::<HashMap<EmojiId, Emoji>>();

    let mut most_used_emotes = emoji_map
        .iter()
        .filter_map(|(i, c)| guild_emotes.get(i).map(|e| (e, c)))
        .collect::<Vec<_>>();

    most_used_emotes = match emoji_type.as_deref() {
        Some("Normal") => most_used_emotes
            .into_iter()
            .filter(|(i, _)| !i.animated)
            .collect(),
        Some("Animated") => most_used_emotes
            .into_iter()
            .filter(|(i, _)| i.animated)
            .collect(),
        Some(_) | None => most_used_emotes,
    };

    most_used_emotes = match usage.as_deref() {
        Some("Text") => most_used_emotes
            .into_iter()
            .filter(|(_, c)| c.text_count > 0)
            .collect(),
        Some("Reactions") => most_used_emotes
            .into_iter()
            .filter(|(_, c)| c.reaction_count > 0)
            .collect(),
        Some(_) | None => most_used_emotes,
    };

    match order.as_str() {
        "Ascending" => most_used_emotes.sort_unstable_by(|(_, a), (_, b)| match usage.as_deref() {
            Some("Text") => a.text_count.cmp(&b.text_count),
            Some("Reactions") => a.reaction_count.cmp(&b.reaction_count),
            Some(_) | None => a.cmp(b),
        }),
        "Descending" => {
            most_used_emotes.sort_unstable_by(|(_, a), (_, b)| match usage.as_deref() {
                Some("Text") => b.text_count.cmp(&a.text_count),
                Some("Reactions") => b.reaction_count.cmp(&a.reaction_count),
                Some(_) | None => b.cmp(a),
            })
        }
        _ => return Err(anyhow!("Invalid ordering.").context(here!())),
    }

    let most_used_emotes = most_used_emotes.into_iter().take(100).collect::<Vec<_>>();

    let title = format!(
        "{} used {}emotes{}",
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
        match usage.as_deref() {
            Some("Text") => " (Not counting reactions)",
            Some("Reactions") => " (Only counting reactions)",
            Some(_) | None => "",
        }
    );

    PaginatedList::new()
        .title(&title)
        .data(&most_used_emotes)
        .layout(PageLayout::Chunked {
            chunk_size: 10,
            chunks_per_page: 3,
        })
        .params(&[&usage.unwrap_or_default()])
        .format(Box::new(|(e, c), params| {
            if !params[0].is_empty() {
                match params[0].as_str() {
                    "Text" => format!("{} {}\r\n", Mention::from(*e), c.text_count),
                    "Reactions" => format!("{} {}\r\n", Mention::from(*e), c.reaction_count),
                    _ => "Invalid usage.".to_string(),
                }
            } else {
                format!(
                    "{} {} ({}T, {}R)\r\n",
                    Mention::from(*e),
                    c.total(),
                    c.text_count,
                    c.reaction_count
                )
            }
        }))
        .display(
            interaction,
            ctx,
            *ctx.cache.current_user_id().await.as_u64(),
        )
        .await?;

    Ok(())
}
