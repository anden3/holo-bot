use std::str::FromStr;

use chrono::Utc;

use super::prelude::*;

use utility::config::HoloBranch;

#[slash_setup]
pub async fn setup(ctx: &Ctx, guild: &Guild, app_id: u64) -> anyhow::Result<ApplicationCommand> {
    let cmd = Interaction::create_guild_application_command(&ctx.http, guild.id, app_id, |i| {
        i.name("birthdays")
            .description("Shows upcoming birthdays.")
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
    .await
    .context(here!())?;

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
pub async fn birthdays(ctx: &Ctx, interaction: &Interaction) -> anyhow::Result<()> {
    let mut branch: Option<HoloBranch> = None;

    if let Some(data) = &interaction.data {
        for option in &data.options {
            if let Some(value) = &option.value {
                match option.name.as_str() {
                    "branch" => {
                        branch =
                            HoloBranch::from_str(&serde_json::from_value::<String>(value.clone())?)
                                .ok()
                    }
                    _ => error!(
                        "Unknown option '{}' found for command '{}'.",
                        option.name,
                        file!()
                    ),
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
    let conf = data.get::<Config>().unwrap();

    let get_birthdays = apis::birthday_reminder::BirthdayReminder::get_birthdays(&conf.users);

    let bdays = get_birthdays
        .iter()
        .filter(|b| {
            if let Some(branch_filter) = &branch {
                if b.user.branch != *branch_filter {
                    return false;
                }
            }

            true
        })
        .collect::<Vec<_>>();

    let app_id = *ctx.cache.current_user_id().await.as_u64();

    PaginatedList::new()
        .title("HoloPro Birthdays")
        .data(&bdays)
        .format(Box::new(|b| {
            format!(
                "{} {}\r\n\r\n",
                Mention::from(RoleId(b.user.discord_role)),
                chrono_humanize::HumanTime::from(b.birthday - Utc::now()).to_text_en(
                    chrono_humanize::Accuracy::Rough,
                    chrono_humanize::Tense::Future
                )
            )
        }))
        .display(interaction, ctx, app_id)
        .await?;

    /*
    let mut current_page: i32 = 1;
    let required_pages = ((bdays.len() as f32) / PAGE_LENGTH as f32).ceil() as usize;

    let message =
        Interaction::edit_original_interaction_response(interaction, &ctx.http, app_id, |r| {
            r.embed(|e| {
                e.colour(Colour::new(6_282_735));
                e.description(
                    bdays
                        .iter()
                        .skip(((current_page - 1) as usize) * PAGE_LENGTH)
                        .take(PAGE_LENGTH)
                        .fold(String::new(), |mut acc, bday| {
                            acc += format!(
                                "{} {}\r\n\r\n",
                                Mention::from(RoleId(bday.user.discord_role)),
                                chrono_humanize::HumanTime::from(bday.birthday - now).to_text_en(
                                    chrono_humanize::Accuracy::Rough,
                                    chrono_humanize::Tense::Future
                                ),
                            )
                            .as_str();
                            acc
                        }),
                );
                e.footer(|f| f.text(format!("Page {} of {}", current_page, required_pages)))
            })
        })
        .await
        .context(here!())?;

    if required_pages == 1 {
        return Ok(());
    }

    let left = message.react(&ctx, '⬅').await.context(here!())?;
    let right = message.react(&ctx, '➡').await.context(here!())?;

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
                            bdays
                                .iter()
                                .skip(((current_page - 1) as usize) * PAGE_LENGTH)
                                .take(PAGE_LENGTH)
                                .fold(String::new(), |mut acc, bday| {
                                    acc += format!(
                                        "{} {}\r\n\r\n",
                                        Mention::from(RoleId(bday.user.discord_role)),
                                        chrono_humanize::HumanTime::from(bday.birthday - now)
                                            .to_text_en(
                                                chrono_humanize::Accuracy::Rough,
                                                chrono_humanize::Tense::Future
                                            ),
                                    )
                                    .as_str();
                                    acc
                                }),
                        );
                        e.footer(|f| f.text(format!("Page {} of {}", current_page, required_pages)))
                    })
                })
                .await
                .context(here!())?;
        }
    }
    */
    Ok(())
}
