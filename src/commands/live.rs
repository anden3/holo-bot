use std::str::FromStr;
use std::time::Duration;

use super::prelude::*;

use crate::apis::holo_api::StreamState;
use crate::config::HoloBranch;
use crate::discord_bot::{ReactionSender, ReactionUpdate, StreamIndex};

const PAGE_LENGTH: usize = 5;

#[slash_setup]
pub async fn setup(
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
    })
    .await?;

    Ok(cmd)
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
#[slash_command]
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
pub async fn live(ctx: &Context, interaction: &Interaction) -> anyhow::Result<()> {
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
                    branch =
                        HoloBranch::from_str(&serde_json::from_value::<String>(value.clone())?).ok()
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
        .map(|(_, l)| LiveEmbedData {
            role: l.streamer.discord_role.into(),
            title: l.title.clone(),
            url: l.url.clone(),
        })
        .collect::<Vec<_>>();

    std::mem::drop(stream_index);

    let app_id = *ctx.cache.current_user_id().await.as_u64();

    let mut current_page: usize = 1;
    let required_pages = ((currently_live.len() as f32) / PAGE_LENGTH as f32).ceil() as usize;

    let message =
        Interaction::edit_original_interaction_response(interaction, &ctx.http, app_id, |r| {
            r.embed(|e| {
                e.colour(Colour::new(6_282_735));
                e.description(
                    currently_live
                        .iter()
                        .skip((current_page - 1) * PAGE_LENGTH)
                        .take(PAGE_LENGTH)
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
                );
                e.footer(|f| f.text(format!("Page {} of {}", current_page, required_pages)))
            })
        })
        .await?;

    if required_pages == 1 {
        return Ok(());
    }

    let left = message.react(&ctx, '⬅').await?;
    let right = message.react(&ctx, '➡').await?;

    let mut reaction_recv = data.get::<ReactionSender>().unwrap().subscribe();

    while let Ok(Ok(update)) =
        tokio::time::timeout(Duration::from_secs(60 * 15), reaction_recv.recv()).await
    {
        if let ReactionUpdate::Added(reaction) = update {
            if reaction.message_id != message.id {
                continue;
            }

            if reaction.emoji == left.emoji {
                reaction.delete(&ctx).await?;
                current_page -= 1;

                if current_page < 1 {
                    current_page = required_pages;
                }
            } else if reaction.emoji == right.emoji {
                reaction.delete(&ctx).await?;
                current_page += 1;

                if current_page > required_pages {
                    current_page = 1;
                }
            } else {
                continue;
            }

            interaction
                .edit_original_interaction_response(&ctx.http, app_id, |e| {
                    e.embed(|e| {
                        e.colour(Colour::new(6_282_735));
                        e.description(
                            currently_live
                                .iter()
                                .skip((current_page - 1) * PAGE_LENGTH)
                                .take(PAGE_LENGTH)
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
                        );
                        e.footer(|f| f.text(format!("Page {} of {}", current_page, required_pages)))
                    })
                })
                .await?;
        }
    }

    Ok(())
}
