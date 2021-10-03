use std::{borrow::Cow, str::FromStr};

use chrono::{DateTime, Utc};
use serenity::builder::CreateEmbed;

use super::prelude::*;

use utility::config::HoloBranch;

interaction_setup! {
    name = "live",
    group = "utility",
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
    name: String,
    role: Option<RoleId>,
    title: String,
    url: String,
    start_at: DateTime<Utc>,
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
    interaction: &ApplicationCommandInteraction,
    config: &Config,
) -> anyhow::Result<()> {
    parse_interaction_options!(
        interaction.data, [
        branch: enum HoloBranch,
    ]);

    show_deferred_response(interaction, ctx, false).await?;

    let currently_live = get_currently_live(ctx, branch).await;

    PaginatedList::new()
        .title(format!(
            "Live streams{}",
            branch
                .map(|b| format!(" from {}", b.to_string()))
                .unwrap_or_default()
        ))
        .data(&currently_live)
        .embed(Box::new(|l, _| {
            let mut embed = CreateEmbed::default();

            embed.colour(l.colour);
            embed.thumbnail(l.thumbnail.to_owned());
            embed.timestamp(l.start_at.to_rfc3339());
            embed.description(format!(
                "{}\r\n{}\r\n<{}>",
                if let Some(role) = l.role {
                    Cow::Owned(Mention::from(role).to_string())
                } else {
                    Cow::Borrowed(&l.name)
                },
                l.title,
                l.url
            ));
            embed.footer(|f| {
                f.text(format!(
                    "Started streaming {}.",
                    chrono_humanize::HumanTime::from(Utc::now() - l.start_at).to_text_en(
                        chrono_humanize::Accuracy::Rough,
                        chrono_humanize::Tense::Past
                    )
                ))
            });

            embed
        }))
        .display(ctx, interaction)
        .await?;

    Ok(())
}

async fn get_currently_live(ctx: &Ctx, branch: Option<HoloBranch>) -> Vec<LiveEmbedData> {
    let data = ctx.data.read().await;
    let stream_index = data.get::<StreamIndex>().unwrap().borrow();

    stream_index
        .iter()
        .filter(|(_, l)| {
            if l.state != VideoStatus::Live {
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
            name: l.streamer.english_name.clone(),
            role: l.streamer.discord_role,
            title: l.title.clone(),
            url: l.url.clone(),
            start_at: l.start_at,
            colour: l.streamer.colour,
            thumbnail: l.thumbnail.clone(),
        })
        .collect::<Vec<_>>()
}
