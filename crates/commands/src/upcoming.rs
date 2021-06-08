use std::str::FromStr;

use chrono::{DateTime, Utc};
use serenity::builder::CreateEmbed;

use super::prelude::*;

use apis::holo_api::StreamState;
use utility::config::HoloBranch;

interaction_setup! {
    name = "upcoming",
    description = "Shows scheduled streams.",
    options = [
        //! Show only talents from this branch of Hololive.
        branch: String = enum HoloBranch,
        //! How many minutes to look ahead.
        until: Integer,
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
struct ScheduledEmbedData {
    role: RoleId,
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
    interaction: &Interaction,
    config: &Config,
    app_id: u64,
) -> anyhow::Result<()> {
    parse_interaction_options!(
        interaction.data.as_ref().unwrap(), [
        branch: enum HoloBranch,
        until: i64 = 60,
    ]);

    show_deferred_response(&interaction, &ctx, false).await?;
    let scheduled = get_scheduled(&ctx, branch, until).await;

    PaginatedList::new()
        .title("Upcoming Streams")
        .data(&scheduled)
        .embed(Box::new(|s, _| {
            let mut embed = CreateEmbed::default();

            embed.colour(s.colour);
            embed.thumbnail(s.thumbnail.to_owned());
            embed.timestamp(s.start_at.to_rfc3339());
            embed.description(format!(
                "{}\r\n{}\r\n<https://youtube.com/watch?v={}>",
                Mention::from(s.role),
                s.title,
                s.url
            ));

            embed
        }))
        .display(interaction, ctx, app_id)
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
            if l.state != StreamState::Scheduled || (l.start_at - now).num_minutes() > until {
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
            role: l.streamer.discord_role.into(),
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
