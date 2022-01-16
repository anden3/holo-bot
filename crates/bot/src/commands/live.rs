use std::borrow::Cow;

use chrono::{DateTime, Utc};
use serenity::builder::CreateEmbed;

use super::prelude::*;

use utility::config::HoloBranch;

#[poise::command(
    slash_command,
    prefix_command,
    track_edits,
    check = "stream_tracking_enabled",
    required_permissions = "SEND_MESSAGES"
)]
/// Shows the Hololive talents who are live right now.
pub(crate) async fn live(
    ctx: Context<'_>,
    #[description = "Show only talents from this branch of Hololive."] branch: Option<HoloBranch>,
) -> anyhow::Result<()> {
    ctx.defer().await?;

    let currently_live = get_currently_live(ctx, branch).await;

    PaginatedList::new()
        .title(format!(
            "Live streams{}",
            branch.map(|b| format!(" from {b}")).unwrap_or_default()
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
        .display(ctx)
        .await?;

    Ok(())
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

async fn get_currently_live(ctx: Context<'_>, branch: Option<HoloBranch>) -> Vec<LiveEmbedData> {
    let data = ctx.data();
    let read_lock = data.data.read().await;

    let stream_index = match read_lock.stream_index.as_ref() {
        Some(index) => index.borrow(),
        None => {
            warn!("Stream index is not loaded.");
            return Vec::new();
        }
    };

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

async fn stream_tracking_enabled(ctx: Context<'_>) -> anyhow::Result<bool> {
    Ok(ctx.data().config.stream_tracking.enabled)
}
