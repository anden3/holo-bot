use std::str::FromStr;

use serenity::builder::CreateEmbed;

use super::prelude::*;

use apis::holo_api::StreamState;
use utility::config::HoloBranch;

interaction_setup! {
    name = "live",
    description = "Shows the Hololive talents who are live right now.",
    options = [
        //! Show only talents from this branch of Hololive.
        branch: String = enum HoloBranch,
    ],
    restrictions = [
        allowed_roles = [
            "Admin",
            "Moderator",
            "Moderator (JP)",
            "20 m deep",
            "30 m deep",
            "40 m deep",
            "50 m deep",
            "60 m deep",
            "70 m deep"
        ]
    ]
}

#[derive(Debug)]
struct LiveEmbedData {
    role: RoleId,
    title: String,
    url: String,
    colour: u32,
    thumbnail: String,
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
#[interaction_cmd]
pub async fn live(
    ctx: &Ctx,
    interaction: &Interaction,
    config: &Config,
    app_id: u64,
) -> anyhow::Result<()> {
    parse_interaction_options!(
        interaction.data.as_ref().unwrap(), [
        branch: enum HoloBranch,
    ]);

    show_deferred_response(&interaction, &ctx, false).await?;

    let currently_live = get_currently_live(&ctx, branch).await;

    PaginatedList::new()
        .title("Live Streams")
        .data(&currently_live)
        .embed(Box::new(|l, _| {
            let mut embed = CreateEmbed::default();

            embed.colour(l.colour);
            embed.thumbnail(l.thumbnail.to_owned());
            embed.description(format!(
                "{}\r\n{}\r\n<https://youtube.com/watch?v={}>",
                Mention::from(l.role),
                l.title,
                l.url
            ));

            embed
        }))
        .display(interaction, ctx, app_id)
        .await?;

    Ok(())
}

async fn get_currently_live(ctx: &Ctx, branch: Option<HoloBranch>) -> Vec<LiveEmbedData> {
    let data = ctx.data.read().await;
    let stream_index = data.get::<StreamIndex>().unwrap().borrow();

    stream_index
        .iter()
        .filter(|(_, l)| {
            if l.state != StreamState::Live {
                return false;
            }

            if let Some(branch_filter) = &branch {
                if l.streamer.branch != *branch_filter {
                    return false;
                }
            }

            true
        })
        .map(|(_, l)| LiveEmbedData {
            role: l.streamer.discord_role.into(),
            title: l.title.clone(),
            url: l.url.clone(),
            colour: l.streamer.colour,
            thumbnail: l.thumbnail.clone(),
        })
        .collect::<Vec<_>>()
}
