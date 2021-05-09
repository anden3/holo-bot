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
        //! If only text or reaction usages should be counted.
        usage: String = [
            "In Messages": "Text",
            "Reactions": "Reactions",
        ],
    ],
}

#[interaction_cmd]
#[allowed_roles("Admin", "Moderator", "Moderator (JP)")]
pub async fn emoji_usage(ctx: &Ctx, interaction: &Interaction) -> anyhow::Result<()> {
    parse_interaction_options!(interaction.data.as_ref().unwrap(), [order: req String]);
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
        .filter(|(i, _)| guild_emotes.contains_key(i))
        .collect::<Vec<_>>();

    match order.as_str() {
        "Ascending" => most_used_emotes.sort_unstable(),
        "Descending" => most_used_emotes.sort_unstable_by(|(_, a), (_, b)| b.cmp(a)),
        _ => return Err(anyhow!("Invalid ordering.").context(here!())),
    }

    let most_used_emotes = most_used_emotes
        .into_iter()
        .take(100)
        .map(|(i, c)| (&guild_emotes[i], c))
        .collect::<Vec<_>>();

    let app_id = *ctx.cache.current_user_id().await.as_u64();

    PaginatedList::new()
        .title(
            match order.as_str() {
                "Ascending" => format!(
                    "Least used emotes in {}",
                    interaction.guild_id.name(&ctx.cache).await.unwrap()
                ),
                "Descending" => format!(
                    "Most used emotes in {}",
                    interaction.guild_id.name(&ctx.cache).await.unwrap()
                ),
                _ => return Err(anyhow!("Invalid ordering.").context(here!())),
            }
            .as_str(),
        )
        .data(&most_used_emotes)
        .layout(PageLayout::Chunked {
            chunk_size: 10,
            chunks_per_page: 3,
        })
        .format(Box::new(|(e, c)| {
            format!(
                "{} {} ({}T, {}R)\r\n",
                Mention::from(*e),
                c.total(),
                c.text_count,
                c.reaction_count
            )
        }))
        .display(interaction, ctx, app_id)
        .await?;

    Ok(())
}
