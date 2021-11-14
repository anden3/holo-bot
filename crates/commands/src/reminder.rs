use super::prelude::*;

use chrono::{Duration, SecondsFormat, Utc};
use chrono_humanize::{Accuracy, HumanTime, Tense};
use chrono_tz::{Tz, UTC};
use futures::stream::StreamExt;
use nanorand::Rng;
use serenity::model::interactions::message_component::ButtonStyle;

use utility::config::{
    EntryEvent, LoadFromDatabase, Reminder, ReminderFrequency, ReminderLocation, ReminderSubscriber,
};

interaction_setup! {
    name = "reminder",
    group = "utility",
    description = "Set reminders.",
    enabled_if = |config| config.reminders.enabled,
    options = {
        //! Add new reminder.
        add: SubCommand = {
            //! When to remind you.
            when: String,
            //! What to remind you of.
            message: String,
            //! How often to remind you.
            frequency: Option<ReminderFrequency>,
            //! Where to remind you.
            location: Option<ReminderLocation>,
            //! Your timezone in IANA format (ex. America/New_York).
            timezone: Option<String>,
        },
        //! Remove reminder.
        remove: SubCommand = {
            //! ID of the reminder to remove.
            id: Integer,
        },
        //! Show your current reminders.
        list: SubCommand,
    }
}

#[interaction_cmd]
async fn reminder(
    ctx: &Ctx,
    interaction: &ApplicationCommandInteraction,
    config: &Config,
) -> anyhow::Result<()> {
    let reminder_sender = {
        let data = ctx.data.read().await;
        ReminderSender(data.get::<ReminderSender>().unwrap().0.clone())
    };

    match_sub_commands! {
        "add" => |
            when: String,
            message: Option<String>,
            frequency: ReminderFrequency = ReminderFrequency::Once,
            location: Option<ReminderLocation>,
            timezone: Option<String>
        | {
            add_reminder(ctx, interaction, &reminder_sender, when, frequency, message, location, timezone).await?;
        },
        "remove" => |id: u32| {
            remove_reminder(ctx, interaction, &reminder_sender, id).await?;
        },
        "list" => {
            list_reminders(ctx, interaction, config).await?;
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn add_reminder(
    ctx: &Ctx,
    interaction: &ApplicationCommandInteraction,
    reminder_sender: &ReminderSender,
    when: String,
    frequency: ReminderFrequency,
    message: Option<String>,
    location: Option<ReminderLocation>,
    timezone: Option<String>,
) -> anyhow::Result<()> {
    show_deferred_response(interaction, ctx, false).await?;

    let id = nanorand::tls_rng().generate();
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
        Some(ReminderLocation::Channel(_)) => ReminderLocation::Channel(interaction.channel_id),
        None => ReminderLocation::DM,
    };

    let mut reminder = Reminder {
        id,
        time,
        frequency,
        message: message.clone(),
        subscribers: vec![ReminderSubscriber {
            user: interaction.user.id,
            location,
        }],
    };

    reminder_sender
        .send(EntryEvent::Added {
            key: id,
            value: reminder.clone(),
        })
        .await?;

    let message = interaction
        .edit_original_interaction_response(&ctx.http, |r| {
            r.create_embed(|e| {
                e.title("Reminder created!")
                    .colour(
                        interaction
                            .member
                            .as_ref()
                            .and_then(|m| m.colour(&ctx.cache))
                            .unwrap_or_default(),
                    )
                    .description(&message)
                    .footer(|f| f.text(frequency.to_string()))
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
        match i.data.custom_id.as_str() {
            "subscribe_dm" => {
                reminder.subscribers.push(ReminderSubscriber {
                    user: i.member.as_ref().unwrap().user.id,
                    location: ReminderLocation::DM,
                });
            }
            "subscribe_ch" => {
                reminder.subscribers.push(ReminderSubscriber {
                    user: i.member.as_ref().unwrap().user.id,
                    location: ReminderLocation::Channel(i.channel_id),
                });
            }
            _ => continue,
        }

        reminder_sender
            .send(EntryEvent::Updated {
                key: id,
                value: reminder.clone(),
            })
            .await?;
    }

    interaction
        .delete_original_interaction_response(&ctx.http)
        .await
        .context(here!())?;

    Ok(())
}

async fn remove_reminder(
    ctx: &Ctx,
    interaction: &ApplicationCommandInteraction,
    reminder_sender: &ReminderSender,
    id: u32,
) -> anyhow::Result<()> {
    show_deferred_response(interaction, ctx, true).await?;

    reminder_sender
        .send(EntryEvent::Removed { key: id })
        .await?;

    interaction
        .edit_original_interaction_response(&ctx.http, |e| e.content("Reminder removed!"))
        .await
        .context(here!())?;

    Ok(())
}

async fn list_reminders(
    ctx: &Ctx,
    interaction: &ApplicationCommandInteraction,
    config: &Config,
) -> anyhow::Result<()> {
    show_deferred_response(interaction, ctx, true).await?;

    let user = interaction.user.id;

    let database = config.database.get_handle()?;
    let reminders = Reminder::load_from_database(&database)?
        .into_iter()
        .filter(|r| r.subscribers.iter().any(|s| s.user == user))
        .collect::<Vec<_>>();

    if reminders.is_empty() {
        interaction
            .delete_original_interaction_response(&ctx.http)
            .await?;
        return Ok(());
    }

    PaginatedList::new()
        .title("Saved Reminders")
        .data(&reminders)
        .format(Box::new(|r, _| {
            format!(
                "**{:0>16x}**: __{}__\n{} ({})\n",
                r.id,
                r.message,
                HumanTime::from(r.time - Utc::now()).to_text_en(Accuracy::Rough, Tense::Future),
                r.time.to_rfc3339_opts(SecondsFormat::Secs, false),
            )
        }))
        .display(ctx, interaction)
        .await?;

    Ok(())
}
