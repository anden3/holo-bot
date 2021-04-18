use std::str::FromStr;
use std::time::Duration;

use chrono::{DateTime, Utc};

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
        i.name("upcoming")
            .description("Shows scheduled streams.")
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
                o.name("until")
                    .description("How many minutes ahead to look for streams.")
                    .kind(ApplicationCommandOptionType::Integer)
            })
    })
    .await?;

    Ok(cmd)
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
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
pub async fn upcoming(ctx: &Context, interaction: &Interaction) -> anyhow::Result<()> {
    struct ScheduledEmbedData {
        role: RoleId,
        title: String,
        url: String,
        start_at: DateTime<Utc>,
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
    .await?;

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
            url: l.url.clone(),
            start_at: l.start_at,
        })
        .collect::<Vec<_>>();

    std::mem::drop(stream_index);
    scheduled.sort_unstable_by_key(|l| l.start_at);

    let app_id = *ctx.cache.current_user_id().await.as_u64();

    let mut current_page: i32 = 1;
    let required_pages = ((scheduled.len() as f32) / PAGE_LENGTH as f32).ceil() as usize;

    let message =
        Interaction::edit_original_interaction_response(interaction, &ctx.http, app_id, |r| {
            r.embed(|e| {
                e.colour(Colour::new(6_282_735));
                e.description(
                    scheduled
                        .iter()
                        .skip(((current_page - 1) as usize) * PAGE_LENGTH)
                        .take(PAGE_LENGTH)
                        .fold(String::new(), |mut acc, scheduled| {
                            acc += format!(
                                "{} {}\r\n{}\r\n<https://youtube.com/watch?v={}>\r\n\r\n",
                                Mention::from(scheduled.role),
                                chrono_humanize::HumanTime::from(scheduled.start_at - now)
                                    .to_text_en(
                                        chrono_humanize::Accuracy::Precise,
                                        chrono_humanize::Tense::Future
                                    ),
                                scheduled.title,
                                scheduled.url
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

            if let Some(user) = reaction.user_id {
                if user == app_id {
                    continue;
                }
            }

            if reaction.emoji == left.emoji {
                reaction.delete(&ctx).await?;
                current_page -= 1;

                if current_page < 1 {
                    current_page = required_pages as i32;
                }
            } else if reaction.emoji == right.emoji {
                reaction.delete(&ctx).await?;
                current_page += 1;

                if current_page > required_pages as i32 {
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
                            scheduled
                                .iter()
                                .skip(((current_page - 1) as usize) * PAGE_LENGTH)
                                .take(PAGE_LENGTH)
                                .fold(String::new(), |mut acc, scheduled| {
                                    acc += format!(
                                        "{} {}\r\n{}\r\n<https://youtube.com/watch?v={}>\r\n\r\n",
                                        Mention::from(scheduled.role),
                                        chrono_humanize::HumanTime::from(scheduled.start_at - now)
                                            .to_text_en(
                                                chrono_humanize::Accuracy::Precise,
                                                chrono_humanize::Tense::Future
                                            ),
                                        scheduled.title,
                                        scheduled.url
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
