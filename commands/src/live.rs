use std::str::FromStr;

use super::prelude::*;

use apis::holo_api::StreamState;
use utility::config::HoloBranch;

interaction_setup! {
    name = "live",
    description = "Shows the Hololive talents who are live right now.",
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
pub async fn live(ctx: &Ctx, interaction: &Interaction) -> anyhow::Result<()> {
    #[derive(Debug)]
    struct LiveEmbedData {
        role: RoleId,
        title: String,
        url: String,
    }

    let mut branch: Option<HoloBranch> = None;

    if let Some(data) = &interaction.data {
        for option in &data.options {
            if let Some(value) = &option.value {
                if option.name.as_str() == "branch" {
                    branch = HoloBranch::from_str(
                        &serde_json::from_value::<String>(value.clone()).context(here!())?,
                    )
                    .ok()
                } else {
                    error!("Unknown option '{}' found for command 'live'.", option.name)
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

    let currently_live = stream_index
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
        })
        .collect::<Vec<_>>();

    std::mem::drop(stream_index);

    let app_id = *ctx.cache.current_user_id().await.as_u64();

    PaginatedList::new()
        .title("Live Streams")
        .data(&currently_live)
        .format(Box::new(|l| {
            format!(
                "{}\r\n{}\r\n<https://youtube.com/watch?v={}>\r\n\r\n",
                Mention::from(l.role),
                l.title,
                l.url
            )
        }))
        .display(interaction, ctx, app_id)
        .await?;

    Ok(())
}
