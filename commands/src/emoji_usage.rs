use std::collections::HashMap;

use serenity::model::{guild::Emoji, id::EmojiId};

use super::prelude::*;

interaction_setup! {
    name = "emoji_usage",
    description = "Shows the most used emotes in this server",
}

#[interaction_cmd]
#[allowed_roles("Admin", "Moderator", "Moderator (JP)")]
pub async fn emoji_usage(ctx: &Ctx, interaction: &Interaction) -> anyhow::Result<()> {
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

    most_used_emotes.sort_unstable_by(|(_, a), (_, b)| b.cmp(a));

    let most_used_emotes = most_used_emotes
        .into_iter()
        .take(100)
        .map(|(i, c)| (&guild_emotes[i], c))
        .collect::<Vec<_>>();

    let app_id = *ctx.cache.current_user_id().await.as_u64();

    PaginatedList::new()
        .title(
            format!(
                "Most used emotes in {}",
                interaction.guild_id.name(&ctx.cache).await.unwrap()
            )
            .as_str(),
        )
        .data(&most_used_emotes)
        .layout(PageLayout::Chunked {
            chunk_size: 5,
            chunks_per_page: 2,
        })
        .format(Box::new(|(e, c)| {
            format!("{} {}\r\n", Mention::from(*e), c)
        }))
        .display(interaction, ctx, app_id)
        .await?;

    Ok(())
}
