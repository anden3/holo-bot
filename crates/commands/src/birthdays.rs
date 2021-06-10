use std::str::FromStr;

use chrono::Utc;

use super::prelude::*;

use apis::birthday_reminder::BirthdayReminder;
use utility::config::HoloBranch;

interaction_setup! {
    name = "birthdays",
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
    interaction: &Interaction,
    config: &Config,
    app_id: u64,
) -> anyhow::Result<()> {
    parse_interaction_options!(interaction.data.as_ref().unwrap(), [branch: enum HoloBranch]);
    show_deferred_response(&interaction, &ctx, false).await?;

    let users = &config.users;
    let get_birthdays = BirthdayReminder::get_birthdays(&users);

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
                Mention::from(RoleId(b.user.discord_role)),
                chrono_humanize::HumanTime::from(b.birthday - Utc::now()).to_text_en(
                    chrono_humanize::Accuracy::Rough,
                    chrono_humanize::Tense::Future
                )
            )
        }))
        .display(interaction, ctx, app_id)
        .await?;
    Ok(())
}
