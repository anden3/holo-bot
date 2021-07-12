use super::prelude::*;

use std::str::FromStr;

use chrono::{Duration, SecondsFormat, Utc};
use chrono_humanize::{Accuracy, HumanTime, Tense};
use chrono_tz::{Tz, UTC};
use futures::stream::StreamExt;
use rand::Rng;
use rusqlite::Connection;
use serenity::model::interactions::{ButtonStyle, InteractionData};
use utility::config::{Reminder, ReminderLocation, ReminderSubscriber};

interaction_setup! {
    name = "reminder",
    group = "utility",
    description = "Set reminders.",
    options = [
        //! Add new reminder.
        add: SubCommand = [
            //! When to remind you.
            req when: String,
            //! What to remind you of.
            message: String,
            //! Where to remind you.
            location: String = enum ReminderLocation,
            //! Your timezone in IANA format (ex. America/New_York).
            timezone: String,
        ],
        //! Remove reminder.
        remove: SubCommand = [
            //! ID of the reminder to remove.
            req id: Integer,
        ],
        //! Show your current reminders.
        list: SubCommand,
    ]
}

#[interaction_cmd]
async fn reminder(ctx: &Ctx, interaction: &Interaction, config: &Config) -> anyhow::Result<()> {
    let handle = config.get_database_handle()?;

    match_sub_commands! {
        "add" => |when: req String, message: String, location: enum ReminderLocation, timezone: String| {
            add_reminder(ctx, interaction, &handle, when, message, location, timezone).await?;
        },
        "remove" => |id: req u64| {
            remove_reminder(ctx, interaction, &handle, id).await?;
        },
        "list" => {
            list_reminders(ctx, interaction, &handle).await?;
        }
    }

    Ok(())
}

async fn add_reminder(
    ctx: &Ctx,
    interaction: &Interaction,
    handle: &Connection,
    when: String,
    message: Option<String>,
    location: Option<ReminderLocation>,
    timezone: Option<String>,
) -> anyhow::Result<()> {
    show_deferred_response(&interaction, &ctx, false).await?;

    let id = {
        let mut rng = rand::thread_rng();
        rng.gen::<u64>()
    };

    let message = message.unwrap_or_default();

    let local_timezone: Tz = timezone.and_then(|tz| tz.parse().ok()).unwrap_or(UTC);
    let local_time = Utc::now().with_timezone(&local_timezone);

    let time = {
        if let Some(s) = when.strip_prefix("in ") {
            s
        } else if let Some(s) = when.strip_prefix("at ") {
            s
        } else {
            when.as_str()
        }
    };

    let time = chrono_english::parse_date_string(time, local_time, chrono_english::Dialect::Us)
        .context(here!())?;
    let time = time.with_timezone(&Utc);

    let location = match location {
        Some(ReminderLocation::DM) => ReminderLocation::DM,
        Some(ReminderLocation::Channel(_)) => {
            ReminderLocation::Channel(interaction.channel_id.unwrap())
        }
        None => ReminderLocation::DM,
    };

    let mut reminder = Reminder {
        id,
        time,
        message: message.clone(),
        subscribers: vec![ReminderSubscriber {
            user: interaction.member.as_ref().unwrap().user.id,
            location,
        }],
    };

    let bytes = bincode::serialize(&reminder).context(here!())?;
    tree.insert(bincode::serialize(&id).context(here!())?, bytes)
        .context(here!())?;

    // TODO: Give confirmation with options for others to subscribe.
    let message = interaction
        .edit_original_interaction_response(&ctx.http, |r| {
            r.create_embed(|e| {
                e.title("Reminder created!")
                    .description(&message)
                    .timestamp(&time)
            });

            r.components(|c| {
                c.create_action_row(|r| {
                    r.create_button(|b| {
                        b.style(ButtonStyle::Secondary)
                            .label("Subscribe (DM)")
                            .custom_id("subscribe_dm")
                    })
                    .create_button(|b| {
                        b.style(ButtonStyle::Secondary)
                            .label("Subscribe (Channel)")
                            .custom_id("subscribe_ch")
                    })
                })
            });

            r
        })
        .await
        .context(here!())?;

    let sender = interaction.member.as_ref().unwrap().user.id;

    let timeout = (time - Utc::now()).min(Duration::minutes(15));

    let mut subscription_stream = message
        .await_component_interactions(&ctx.shard)
        .timeout(timeout.to_std()?)
        .filter(move |i| i.member.as_ref().unwrap().user.id != sender)
        .await;

    while let Some(i) = subscription_stream.next().await {
        let component_data = match &i.data.as_ref().unwrap() {
            InteractionData::MessageComponent(d) => d,
            _ => continue,
        };

        match component_data.custom_id.as_str() {
            "subscribe_dm" => {
                reminder.subscribers.push(ReminderSubscriber {
                    user: i.member.as_ref().unwrap().user.id,
                    location: ReminderLocation::DM,
                });
            }
            "subscribe_ch" => {
                reminder.subscribers.push(ReminderSubscriber {
                    user: i.member.as_ref().unwrap().user.id,
                    location: ReminderLocation::Channel(i.channel_id.unwrap()),
                });
            }
            _ => continue,
        }

        let bytes = bincode::serialize(&reminder).context(here!())?;
        tree.insert(bincode::serialize(&id).context(here!())?, bytes)
            .context(here!())?;
    }

    interaction
        .delete_original_interaction_response(&ctx.http)
        .await
        .context(here!())?;

    Ok(())
}

async fn remove_reminder(
    ctx: &Ctx,
    interaction: &Interaction,
    handle: &Connection,
    id: u64,
) -> anyhow::Result<()> {
    show_deferred_response(&interaction, &ctx, true).await?;

    let id_bytes = bincode::serialize(&id).context(here!())?;

    let mut reminder = match tree.get(&id_bytes).context(here!())? {
        Some(r) => bincode::deserialize::<Reminder>(&r).context(here!())?,
        None => return Ok(()),
    };

    let user = interaction.member.as_ref().unwrap().user.id;

    if let Some(pos) = reminder.subscribers.iter().position(|s| s.user == user) {
        if reminder.subscribers.len() == 1 {
            tree.remove(&id_bytes)?;
        } else {
            reminder.subscribers.remove(pos);

            tree.insert(id_bytes, bincode::serialize(&reminder).context(here!())?)
                .context(here!())?;
        }
    } else {
        return Ok(());
    }

    interaction
        .edit_original_interaction_response(&ctx.http, |e| e.content("Reminder removed!"))
        .await
        .context(here!())?;

    Ok(())
}

async fn list_reminders(
    ctx: &Ctx,
    interaction: &Interaction,
    handle: &Connection,
) -> anyhow::Result<()> {
    show_deferred_response(&interaction, &ctx, true).await?;

    if tree.is_empty() {
        interaction
            .delete_original_interaction_response(&ctx.http)
            .await?;
        return Ok(());
    }
    let user = interaction.member.as_ref().unwrap().user.id;

    let user_reminders = tree
        .iter()
        .values()
        .filter_map(|b| b.ok())
        .filter_map(|bytes| bincode::deserialize::<Reminder>(&bytes).ok())
        .filter(|r| r.subscribers.iter().any(|s| s.user == user))
        .collect::<Vec<_>>();

    PaginatedList::new()
        .title("Saved Reminders")
        .data(&user_reminders)
        .format(Box::new(|r, _| {
            format!(
                "0x{:0>16x}: {}\n{} ({})",
                r.id,
                r.message,
                HumanTime::from(r.time - Utc::now()).to_text_en(Accuracy::Rough, Tense::Future),
                r.time.to_rfc3339_opts(SecondsFormat::Secs, false),
            )
        }))
        .display(interaction, ctx)
        .await?;

    Ok(())
}
