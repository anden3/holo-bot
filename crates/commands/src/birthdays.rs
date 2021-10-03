use std::{borrow::Cow, str::FromStr};

use chrono::Utc;

use super::prelude::*;

use apis::birthday_reminder::BirthdayReminder;
use utility::config::HoloBranch;

interaction_setup! {
    name = "birthdays",
    group = "utility",
    description = "Shows upcoming birthdays.",
    options = [
        //! Show only talents from this branch of Hololive.
        branch: String = enum HoloBranch,
    ],
    restrictions = [
        allowed_roles = [
            "Admin",
            "Moderator",
            "Moderator (JP)",
            "20 m deep",
            "30 m deep",
            "40 m deep",
            "50 m deep",
            "60 m deep",
            "70 m deep"
        ]
    ]
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
#[interaction_cmd]
pub async fn birthdays(
    ctx: &Ctx,
    interaction: &ApplicationCommandInteraction,
    config: &Config,
) -> anyhow::Result<()> {
    parse_interaction_options!(interaction.data, [branch: enum HoloBranch]);
    show_deferred_response(interaction, ctx, false).await?;

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
                    Cow::Borrowed(&b.user.english_name)
                },
                chrono_humanize::HumanTime::from(b.birthday - Utc::now()).to_text_en(
                    chrono_humanize::Accuracy::Rough,
                    chrono_humanize::Tense::Future
                )
            )
        }))
        .display(ctx, interaction)
        .await?;
    Ok(())
}
