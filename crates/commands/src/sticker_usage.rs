use std::collections::HashMap;

use serenity::model::{id::StickerId, prelude::Sticker};
use tokio::sync::oneshot;

use super::prelude::*;

interaction_setup! {
    name = "sticker_usage",
    group = "utility",
    description = "Shows the most used stickers in this server",
    enabled_if = |config| config.emoji_tracking.enabled,
    options = {
        //! How the stickers should be sorted.
        sort_by: String = {
            "Usage",
            "Created at",
        },

        //! What order to display the stickers in.
        order: Option<String> = {
            "Ascending",
            "Descending",
        },
        //! Filter stickers by name.
        search: Option<String>,
        //! Number of stickers to fetch.
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
pub async fn sticker_usage(
    ctx: &Ctx,
    interaction: &ApplicationCommandInteraction,
    config: &Config,
) -> anyhow::Result<()> {
    parse_interaction_options!(
        interaction.data,
        [
            sort_by: String,
            order: Option<String>,
            search: Option<String>,
            count: Option<usize>,
        ]
    );
    show_deferred_response(interaction, ctx, false).await?;

    let mut stickers = {
        let guild_stickers = interaction
            .guild_id
            .unwrap()
            .stickers(&ctx.http)
            .await?
            .into_iter()
            .map(|s| (s.id, s))
            .collect::<HashMap<StickerId, Sticker>>();

        let sticker_response = {
            let data = ctx.data.read().await;

            let (sticker_request, sticker_response) = oneshot::channel();

            data.get::<StickerUsageSender>()
                .ok_or_else(|| anyhow!("Failed to reach emoji usage tracker!"))?
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

    let order = order.as_deref().unwrap_or("Descending");

    match sort_by.as_str() {
        "Usage" => match order {
            "Ascending" => stickers.sort_unstable_by(|(_, a), (_, b)| a.cmp(b)),
            "Descending" => stickers.sort_unstable_by(|(_, a), (_, b)| b.cmp(a)),
            _ => return Err(anyhow!("Invalid ordering.").context(here!())),
        },
        "Created at" => match order {
            "Ascending" => stickers
                .sort_unstable_by(|(a, _), (b, _)| a.id.created_at().cmp(&b.id.created_at())),
            "Descending" => stickers
                .sort_unstable_by(|(a, _), (b, _)| b.id.created_at().cmp(&a.id.created_at())),
            _ => return Err(anyhow!("Invalid ordering.").context(here!())),
        },
        _ => return Err(anyhow!("Invalid sort qualifier.").context(here!())),
    }

    let top_stickers = stickers
        .into_iter()
        .take(count.unwrap_or(100))
        .collect::<Vec<_>>();

    let title = format!(
        "{} stickers{}",
        match (sort_by.as_str(), order) {
            ("Usage", "Ascending") => "Least used",
            ("Usage", "Descending") => "Most used",
            ("Created at", "Ascending") => "Oldest",
            ("Created at", "Descending") => "Newest",
            _ => "",
        },
        match &search {
            Some(search) => format!(" matching \"*{}*\"", search),
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
        .params(&[&sort_by])
        .format(Box::new(|(e, c), params| match params[0].as_str() {
            "Usage" => {
                format!(
                    "[{}]({}) {}\r\n",
                    e.name,
                    e.image_url().unwrap_or_default(),
                    c,
                )
            }
            "Created at" => format!(
                "[{}]({}) <t:{}:f>\r\n",
                e.name,
                e.image_url().unwrap_or_default(),
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
