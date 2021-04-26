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
        branch: String = [
            "Hololive JP": HoloBranch::HoloJP.to_string(),
            "Hololive ID": HoloBranch::HoloID.to_string(),
            "Hololive EN": HoloBranch::HoloEN.to_string(),
            "Holostars JP": HoloBranch::HolostarsJP.to_string(),
        ],
    ],
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
#[interaction_cmd]
#[allowed_roles(
    "Admin",
    "Moderator",
    "Moderator (JP)",
    "Server Booster",
    "40 m deep",
    "50 m deep",
    "60 m deep",
    "70 m deep",
    "80 m deep",
    "90 m deep",
    "100 m deep"
)]
pub async fn upcoming(ctx: &Ctx, interaction: &Interaction) -> anyhow::Result<()> {
    #[derive(Debug)]
    struct ScheduledEmbedData {
        role: RoleId,
        title: String,
        thumbnail: String,
        url: String,
        start_at: DateTime<Utc>,
        colour: u32,
    }

    let mut branch: Option<HoloBranch> = None;
    let mut minutes: i64 = 60;

    if let Some(data) = &interaction.data {
        for option in &data.options {
            if let Some(value) = &option.value {
                match option.name.as_str() {
                    "branch" => {
                        branch =
                            HoloBranch::from_str(&serde_json::from_value::<String>(value.clone())?)
                                .ok()
                    }
                    "until" => minutes = serde_json::from_value(value.clone())?,
                    _ => error!("Unknown option '{}' found for command 'live'.", option.name),
                }
            }
        }
    }

    Interaction::create_interaction_response(interaction, &ctx.http, |r| {
        r.kind(InteractionResponseType::DeferredChannelMessageWithSource)
            .interaction_response_data(|d| d.content("Loading..."))
    })
    .await
    .context(here!())?;

    let data = ctx.data.read().await;
    let stream_index = data.get::<StreamIndex>().unwrap().read().await;

    let now = Utc::now();

    let mut scheduled = stream_index
        .iter()
        .filter(|(_, l)| {
            if l.state != StreamState::Scheduled || (l.start_at - now).num_minutes() > minutes {
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

    std::mem::drop(stream_index);
    scheduled.sort_unstable_by_key(|l| l.start_at);

    let app_id = *ctx.cache.current_user_id().await.as_u64();

    PaginatedList::new()
        .title("Upcoming Streams")
        .data(&scheduled)
        /* .format(Box::new(|s| {
            format!(
                "{} {}\r\n{}\r\n<https://youtube.com/watch?v={}>\r\n\r\n",
                Mention::from(s.role),
                chrono_humanize::HumanTime::from(s.start_at - Utc::now()).to_text_en(
                    chrono_humanize::Accuracy::Precise,
                    chrono_humanize::Tense::Future
                ),
                s.title,
                s.url
            )
        })) */
        .embed(Box::new(|s| {
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
