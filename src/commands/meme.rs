use std::str::FromStr;
use std::time::Duration;

use super::prelude::*;

use crate::apis::meme_api::{Meme, MemeApi, MemeFont};
use crate::discord_bot::{MessageSender, MessageUpdate};

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

    let _message =
        Interaction::edit_original_interaction_response(interaction, &ctx.http, app_id, |r| {
            r.embed(|e| {
                e.colour(Colour::new(6_282_735));
                e.fields(memes.chunks(10).enumerate().map(|(i, chunk)| {
                    (
                        format!("{}-{}", i * 10 + 1, i * 10 + chunk.len()),
                        chunk
                            .iter()
                            .map(|m| m.name.clone())
                            .collect::<Vec<_>>()
                            .join("\n"),
                        true,
                    )
                }))
            })
        })
        .await?;

    let mut message_recv = data.get::<MessageSender>().unwrap().subscribe();

    let mut matching_meme: Option<&Meme> = None;

    while let Ok(Ok(update)) =
        tokio::time::timeout(Duration::from_secs(60 * 5), message_recv.recv()).await
    {
        if let MessageUpdate::Sent(msg) = update {
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
    }

    let meme = match matching_meme {
        Some(meme) => meme,
        None => return Ok(()),
    };

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
