use chrono::prelude::*;
use chrono_humanize::HumanTime;
use log::{error, info};
use tokio::{sync::mpsc::Sender, time::sleep};

use crate::apis::discord_api::DiscordMessageData;
use crate::config::{self, User};

pub struct BirthdayReminder {}

impl BirthdayReminder {
    pub async fn start(config: config::Config, notifier_sender: Sender<DiscordMessageData>) {
        tokio::spawn(async move {
            match Self::run(config, notifier_sender).await {
                Ok(()) => (),
                Err(e) => error!("{}", e),
            }
        });
    }

    async fn run(
        config: config::Config,
        notifier_sender: Sender<DiscordMessageData>,
    ) -> anyhow::Result<()> {
        loop {
            for next_birthday in Self::get_upcoming_birthdays(&config.users) {
                let now = Utc::now();

                let time_to_next_birthday = next_birthday.birthday - now;

                info!(
                    "Next birthday is {} {}.",
                    next_birthday.user,
                    HumanTime::from(time_to_next_birthday)
                );

                sleep(time_to_next_birthday.to_std()?).await;

                notifier_sender
                    .send(DiscordMessageData::Birthday(next_birthday))
                    .await?;
            }
        }
    }

    fn get_upcoming_birthdays(users: &[User]) -> Vec<Birthday> {
        let mut birthday_queue = users
            .iter()
            .map(|u| Birthday {
                user: u.display_name.clone(),
                birthday: u.get_next_birthday(),
            })
            .collect::<Vec<_>>();

        birthday_queue.sort_unstable_by_key(|b| b.birthday);
        birthday_queue
    }
}

#[derive(Debug)]
pub struct Birthday {
    pub user: String,
    pub birthday: DateTime<Utc>,
}
