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
/// Shows scheduled streams.
pub(crate) async fn upcoming(
    ctx: Context<'_>,
    #[description = "Show only talents from this branch of Hololive."] branch: Option<HoloBranch>,
    #[description = "How many minutes to look ahead."] until: Option<u32>,
) -> anyhow::Result<()> {
    let until = until.unwrap_or(60);

    let scheduled = get_scheduled(ctx, branch, until as i64).await;

    PaginatedList::new()
        .title(format!(
            "Upcoming streams{} in the next {until} minutes",
            branch.map(|b| format!(" from {}", b)).unwrap_or_default()
        ))
        .data(&scheduled)
        .embed(Box::new(|s, _| {
            let mut embed = CreateEmbed::default();

            embed.description(format!(
                "{}\r\n{}\r\n<{}>",
                if let Some(role) = s.role {
                    Cow::Owned(Mention::from(role).to_string())
                } else {
                    Cow::Borrowed(&s.name)
                },
                s.title,
                s.url
            ));

            embed
                .colour(s.colour)
                .thumbnail(s.thumbnail.to_owned())
                .timestamp(s.start_at.to_rfc3339())
                .footer(|f| {
                    f.text(format!(
                        "Starts {}",
                        chrono_humanize::HumanTime::from(s.start_at - Utc::now()).to_text_en(
                            chrono_humanize::Accuracy::Rough,
                            chrono_humanize::Tense::Future
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
struct ScheduledEmbedData {
    role: Option<RoleId>,
    name: String,
    title: String,
    thumbnail: String,
    url: String,
    start_at: DateTime<Utc>,
    colour: u32,
}

async fn get_scheduled(
    ctx: Context<'_>,
    branch: Option<HoloBranch>,
    until: i64,
) -> Vec<ScheduledEmbedData> {
    let data = ctx.data();
    let read_lock = data.data.read().await;

    let stream_index = read_lock.stream_index.as_ref().unwrap().borrow();

    let now = Utc::now();

    let mut scheduled = stream_index
        .iter()
        .filter(|(_, l)| {
            if l.state != VideoStatus::Upcoming
                || (l.start_at - now).num_minutes() > until
                || now > l.start_at
            {
                return false;
            }

            if let Some(branch_filter) = &branch {
                if l.streamer.branch != *branch_filter {
                    return false;
                }
            }

            true
        })
        .map(|(_, l)| ScheduledEmbedData {
            name: l.streamer.english_name.clone(),
            role: l.streamer.discord_role,
            title: l.title.clone(),
            thumbnail: l.thumbnail.clone(),
            url: l.url.clone(),
            start_at: l.start_at,
            colour: l.streamer.colour,
        })
        .collect::<Vec<_>>();

    scheduled.sort_unstable_by_key(|l| l.start_at);
    scheduled
}

async fn stream_tracking_enabled(ctx: Context<'_>) -> anyhow::Result<bool> {
    Ok(ctx.data().config.stream_tracking.enabled)
}
