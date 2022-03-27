use std::borrow::Cow;

use chrono::Utc;

use super::prelude::*;

use apis::birthday_reminder::BirthdayReminder;
use utility::config::HoloBranch;

#[poise::command(
    slash_command,
    prefix_command,
    track_edits,
    check = "birthdays_enabled",
    required_permissions = "SEND_MESSAGES"
)]
/// Shows upcoming birthdays.
pub(crate) async fn birthdays(
    ctx: Context<'_>,
    #[description = "Show only talents from this branch of Hololive."] branch: Option<HoloBranch>,
) -> anyhow::Result<()> {
    let config = &ctx.data().config;
    let users = &config.talents;
    let get_birthdays = BirthdayReminder::get_birthdays(users);

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

    PaginatedList::new()
        .title("HoloPro Birthdays")
        .data(&bdays)
        .format(Box::new(|b, _| {
            format!(
                "{:<20} {}\r\n",
                if let Some(role) = b.user.discord_role {
                    Cow::Owned(Mention::from(role).to_string())
                } else {
                    Cow::Borrowed(&b.user.name)
                },
                chrono_humanize::HumanTime::from(b.birthday - Utc::now()).to_text_en(
                    chrono_humanize::Accuracy::Rough,
                    chrono_humanize::Tense::Future
                )
            )
        }))
        .display(ctx)
        .await?;

    Ok(())
}

async fn birthdays_enabled(ctx: Context<'_>) -> anyhow::Result<bool> {
    Ok(ctx.data().config.birthday_alerts.enabled)
}
