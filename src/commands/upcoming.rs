use std::str::FromStr;

use chrono::{DateTime, Utc};
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
    struct ScheduledEmbedData {
        role: RoleId,
        title: String,
        url: String,
        start_at: DateTime<Utc>,
    }

    let mut branch: Option<HoloBranch> = None;
    let mut minutes: i64 = 60;
    let mut max_count: usize = 5;

    if let Some(data) = &interaction.data {
        for option in &data.options {
            if let Some(value) = &option.value {
                match option.name.as_str() {
                    "branch" => {
                        branch = HoloBranch::from_str(
                            &serde_json::from_value::<String>(value.clone()).unwrap(),
                        )
                        .ok()
                    }
                    "until" => minutes = serde_json::from_value(value.clone()).unwrap(),
                    "count" => max_count = serde_json::from_value(value.clone()).unwrap(),
                    _ => error!(
                        "Unknown option '{}' found for command 'upcoming'.",
                        option.name
                    ),
                }
            }
        }
    }

    Interaction::create_interaction_response(&interaction, &ctx.http, |r| {
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
        .take(max_count)
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

    Interaction::edit_original_interaction_response(&interaction, &ctx.http, app_id, |r| {
        r.embed(|e| {
            e.colour(Colour::new(6282735));
            e.description(
                scheduled
                    .into_iter()
                    .fold(String::new(), |mut acc, scheduled| {
                        acc += format!(
                            "{} {}\r\n{}\r\n<https://youtube.com/watch?v={}>\r\n\r\n",
                            Mention::from(scheduled.role),
                            chrono_humanize::HumanTime::from(scheduled.start_at - now).to_text_en(
                                chrono_humanize::Accuracy::Precise,
                                chrono_humanize::Tense::Future
                            ),
                            scheduled.title,
                            scheduled.url
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
