use std::time::Duration;
use std::{str::FromStr, time::SystemTime};

use super::prelude::*;

use crate::apis::meme_api::{Meme, MemeApi, MemeFont};
use crate::discord_bot::{MessageSender, MessageUpdate, ReactionSender, ReactionUpdate};

const CHUNK_SIZE: usize = 10;
const CHUNKS_PER_PAGE: usize = 3;

#[slash_setup]
pub async fn setup(
    ctx: &Context,
    guild: &Guild,
    app_id: u64,
) -> anyhow::Result<ApplicationCommand> {
    let cmd = Interaction::create_guild_application_command(&ctx.http, guild.id, app_id, |i| {
        i.name("meme")
            .description("Create a meme peko")
            .create_interaction_option(|o| {
                o.name("font")
                    .description("Which font to use?")
                    .kind(ApplicationCommandOptionType::String)
                    .add_string_choice("Arial", MemeFont::Arial.to_string())
                    .add_string_choice("Impact", MemeFont::Impact.to_string())
            })
            .create_interaction_option(|o| {
                o.name("max_font_size")
                    .description("Maximum font size in pixels.")
                    .kind(ApplicationCommandOptionType::Integer)
            })
    })
    .await?;

    Ok(cmd)
}

#[allow(
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap
)]
#[slash_command]
#[allowed_roles("Admin", "Moderator", "Moderator (JP)", "Server Booster")]
async fn meme(ctx: &Context, interaction: &Interaction) -> anyhow::Result<()> {
    let mut font: MemeFont = MemeFont::Impact;
    let mut max_font_size: i64 = 50;

    if let Some(data) = &interaction.data {
        for option in &data.options {
            if let Some(value) = &option.value {
                match option.name.as_str() {
                    "font" => {
                        font =
                            MemeFont::from_str(&serde_json::from_value::<String>(value.clone())?)?
                    }
                    "max_font_size" => max_font_size = serde_json::from_value(value.clone())?,
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
    let meme_api = data.get::<MemeApi>().unwrap();

    let app_id = *ctx.cache.current_user_id().await.as_u64();

    let arc = meme_api.get_popular_memes().await?;
    let memes = arc.read().await;
    let chunks = memes
        .chunks(CHUNK_SIZE)
        .enumerate()
        .collect::<Vec<_>>()
        .chunks(CHUNKS_PER_PAGE)
        .map(std::borrow::ToOwned::to_owned)
        .collect::<Vec<_>>();

    let mut current_page: i32 = 1;
    let required_pages = chunks.len();

    let message =
        Interaction::edit_original_interaction_response(interaction, &ctx.http, app_id, |r| {
            r.embed(|e| {
                e.colour(Colour::new(6_282_735));
                e.fields(
                    chunks
                        .iter()
                        .skip(current_page as usize - 1)
                        .take(1)
                        .flatten()
                        .map(|(i, chunk)| {
                            (
                                format!("{}-{}", i * CHUNK_SIZE + 1, i * CHUNK_SIZE + chunk.len()),
                                chunk
                                    .iter()
                                    .map(|m| m.name.clone())
                                    .collect::<Vec<_>>()
                                    .join("\n"),
                                true,
                            )
                        }),
                )
            })
        })
        .await?;

    let left = message.react(&ctx, '⬅').await?;
    let right = message.react(&ctx, '➡').await?;

    let mut message_recv = data.get::<MessageSender>().unwrap().subscribe();
    let mut reaction_recv = data.get::<ReactionSender>().unwrap().subscribe();

    let matching_meme: Option<&Meme>;

    let now = SystemTime::now();

    loop {
        if let Ok(duration) = now.elapsed() {
            if duration >= Duration::from_secs(60 * 5) {
                return Ok(());
            }
        }

        if let Ok(MessageUpdate::Sent(msg)) = message_recv.try_recv() {
            if msg.author.id != interaction.member.user.id {
                continue;
            }

            if msg.channel_id != interaction.channel_id {
                continue;
            }

            let text = msg.content.trim();

            matching_meme = match text.parse::<usize>() {
                Ok(num) => match &memes.get(num - 1) {
                    Some(meme) => Some(meme),
                    None => continue,
                },
                Err(_) => match memes
                    .iter()
                    .find(|m| m.name.to_ascii_lowercase() == text.to_ascii_lowercase())
                {
                    Some(meme) => Some(meme),
                    None => continue,
                },
            };
            msg.delete(&ctx).await?;
            break;
        }

        if let Ok(ReactionUpdate::Added(reaction)) = reaction_recv.try_recv() {
            if reaction.message_id != message.id {
                continue;
            }

            if let Some(user) = reaction.user_id {
                if user == app_id {
                    continue;
                }

                if user != interaction.member.user.id {
                    reaction.delete(&ctx).await?;
                    continue;
                }
            }

            if reaction.emoji == left.emoji {
                current_page -= 1;

                if current_page < 1 {
                    current_page = required_pages as i32;
                }
            } else if reaction.emoji == right.emoji {
                current_page += 1;

                if current_page > required_pages as i32 {
                    current_page = 1;
                }
            } else {
                continue;
            }

            reaction.delete(&ctx).await?;

            Interaction::edit_original_interaction_response(interaction, &ctx.http, app_id, |r| {
                r.embed(|e| {
                    e.colour(Colour::new(6_282_735));
                    e.fields(
                        chunks
                            .iter()
                            .skip(current_page as usize - 1)
                            .take(1)
                            .flatten()
                            .map(|(i, chunk)| {
                                (
                                    format!(
                                        "{}-{}",
                                        i * CHUNK_SIZE + 1,
                                        i * CHUNK_SIZE + chunk.len()
                                    ),
                                    chunk
                                        .iter()
                                        .map(|m| m.name.clone())
                                        .collect::<Vec<_>>()
                                        .join("\n"),
                                    true,
                                )
                            }),
                    )
                })
            })
            .await?;
        }
    }

    let meme = match matching_meme {
        Some(meme) => meme,
        None => return Ok(()),
    };

    message.delete_reactions(&ctx).await?;

    let _message =
        Interaction::edit_original_interaction_response(interaction, &ctx.http, app_id, |r| {
            r.embed(|e| {
                e.title(meme.name.to_owned());
                e.description(format!(
                    "Meme has {} text boxes. Please type each caption on a separate line.",
                    meme.box_count
                ));
                e.colour(Colour::new(6_282_735));
                e.image(meme.url.to_owned())
            })
        })
        .await?;

    let mut captions = Vec::with_capacity(meme.box_count);

    while let Ok(Ok(update)) =
        tokio::time::timeout(Duration::from_secs(60 * 10), message_recv.recv()).await
    {
        if let MessageUpdate::Sent(msg) = update {
            if msg.author.id != interaction.member.user.id {
                continue;
            }

            if msg.channel_id != interaction.channel_id {
                continue;
            }

            captions.extend(
                msg.content
                    .trim()
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .take(meme.box_count)
                    .map(std::borrow::ToOwned::to_owned),
            );

            msg.delete(&ctx).await?;

            if captions.len() == meme.box_count {
                break;
            }
        }
    }

    if captions.len() < meme.box_count {
        return Ok(());
    }

    let url = meme_api
        .create_meme(meme, captions, font, max_font_size)
        .await?;

    let _message =
        Interaction::edit_original_interaction_response(interaction, &ctx.http, app_id, |r| {
            r.embed(|e| {
                e.colour(Colour::new(6_282_735));
                e.image(url)
            })
        })
        .await?;

    Ok(())
}
