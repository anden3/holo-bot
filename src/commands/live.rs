use std::str::FromStr;

use log::error;
use serenity::{
    client::Context,
    model::{
        guild::Guild,
        id::RoleId,
        interactions::{
            ApplicationCommand, ApplicationCommandOptionType, Interaction, InteractionResponseType,
        },
        misc::Mention,
    },
    utils::Colour,
};

use crate::apis::holo_api::StreamState;
use crate::config::HoloBranch;
use crate::discord_bot::StreamIndex;

pub async fn setup_interaction(
    ctx: &Context,
    guild: &Guild,
    app_id: u64,
) -> anyhow::Result<ApplicationCommand> {
    let cmd = Interaction::create_guild_application_command(&ctx.http, guild.id, app_id, |i| {
        i.name("live")
            .description("Shows the Hololive talents who are live right now.")
            .create_interaction_option(|o| {
                o.name("branch")
                    .description("Show only talents from this branch of Hololive.")
                    .kind(ApplicationCommandOptionType::String)
                    .add_string_choice("Hololive JP", HoloBranch::HoloJP.to_string())
                    .add_string_choice("Hololive ID", HoloBranch::HoloID.to_string())
                    .add_string_choice("Hololive EN", HoloBranch::HoloEN.to_string())
                    .add_string_choice("Holostars JP", HoloBranch::HolostarsJP.to_string())
            })
            .create_interaction_option(|o| {
                o.name("count")
                    .description("The maximum number of talents to return.")
                    .kind(ApplicationCommandOptionType::Integer)
            })
    })
    .await?;

    Ok(cmd)
}

pub async fn on_interaction(ctx: &Context, interaction: &Interaction) -> anyhow::Result<()> {
    struct LiveEmbedData {
        role: RoleId,
        title: String,
        url: String,
    }

    let mut branch: Option<HoloBranch> = None;
    let mut max_count: usize = 5;

    if let Some(data) = &interaction.data {
        for option in &data.options {
            if let Some(value) = &option.value {
                match option.name.as_str() {
                    "branch" => {
                        branch =
                            HoloBranch::from_str(&serde_json::from_value::<String>(value.clone())?)
                                .ok()
                    }
                    "count" => max_count = serde_json::from_value(value.clone())?,
                    _ => error!("Unknown option '{}' found for command 'live'.", option.name),
                }
            }
        }
    }

    Interaction::create_interaction_response(interaction, &ctx.http, |r| {
        r.kind(InteractionResponseType::DeferredChannelMessageWithSource)
            .interaction_response_data(|d| d.content("Loading..."))
    })
    .await?;

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
        .take(max_count)
        .map(|(_, l)| LiveEmbedData {
            role: l.streamer.discord_role.into(),
            title: l.title.clone(),
            url: l.url.clone(),
        })
        .collect::<Vec<_>>();

    std::mem::drop(stream_index);

    let app_id = *ctx.cache.current_user_id().await.as_u64();

    Interaction::edit_original_interaction_response(interaction, &ctx.http, app_id, |r| {
        r.embed(|e| {
            e.colour(Colour::new(6_282_735));
            e.description(
                currently_live
                    .into_iter()
                    .fold(String::new(), |mut acc, live| {
                        acc += format!(
                            "{}\r\n{}\r\n<https://youtube.com/watch?v={}>\r\n\r\n",
                            Mention::from(live.role),
                            live.title,
                            live.url
                        )
                        .as_str();
                        acc
                    }),
            )
        })
    })
    .await?;
    Ok(())
}
