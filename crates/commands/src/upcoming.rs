use std::borrow::Cow;

use chrono::{DateTime, Utc};
use serenity::builder::CreateEmbed;

use super::prelude::*;

use utility::config::HoloBranch;

interaction_setup! {
    name = "upcoming",
    group = "utility",
    description = "Shows scheduled streams.",
    enabled_if = |config| config.stream_tracking.enabled,
    options = {
        //! Show only talents from this branch of Hololive.
        branch: Option<HoloBranch>,
        //! How many minutes to look ahead.
        until: Option<Integer>,
    },
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
struct ScheduledEmbedData {
    role: Option<RoleId>,
    name: String,
    title: String,
    thumbnail: String,
    url: String,
    start_at: DateTime<Utc>,
    colour: u32,
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
#[interaction_cmd]
pub async fn upcoming(
    ctx: &Ctx,
    interaction: &ApplicationCommandInteraction,
    config: &Config,
) -> anyhow::Result<()> {
    parse_interaction_options!(
        interaction.data,
        [branch: Option<HoloBranch>, until: i64 = 60,]
    );

    show_deferred_response(interaction, ctx, false).await?;
    let scheduled = get_scheduled(ctx, branch, until).await;

    PaginatedList::new()
        .title(format!(
            "Upcoming streams{} in the next {} minutes",
            branch
                .map(|b| format!(" from {}", b.to_string()))
                .unwrap_or_default(),
            until
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
        .display(ctx, interaction)
        .await?;

    Ok(())
}

async fn get_scheduled(
    ctx: &Ctx,
    branch: Option<HoloBranch>,
    until: i64,
) -> Vec<ScheduledEmbedData> {
    let data = ctx.data.read().await;
    let stream_index = data.get::<StreamIndex>().unwrap().borrow();

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
