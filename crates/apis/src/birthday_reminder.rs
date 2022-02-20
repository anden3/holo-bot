use std::sync::Arc;

use anyhow::Context;
use chrono::prelude::*;
use chrono_humanize::HumanTime;
use tokio::{sync::mpsc::Sender, time::sleep};
use tracing::{error, info, instrument};

use super::discord_api::DiscordMessageData;
use utility::{
    config::{Config, Talent},
    here,
};

pub struct BirthdayReminder;

impl BirthdayReminder {
    #[instrument(skip(config, notifier_sender))]
    pub async fn start(config: Arc<Config>, notifier_sender: Sender<DiscordMessageData>) {
        tokio::spawn(async move {
            tokio::select! {
                e = Self::run(&config, notifier_sender) => {
                    if let Err(e) = e {
                        error!("{:#}", e);
                    }
                }
                e = tokio::signal::ctrl_c() => {
                    if let Err(e) = e {
                        error!("{:#}", e);
                    }
                }
            }

            info!(task = "Birthday reminder", "Shutting down.");
        });
    }

    #[instrument(skip(config, notifier_sender))]
    async fn run(
        config: &Config,
        notifier_sender: Sender<DiscordMessageData>,
    ) -> anyhow::Result<()> {
        loop {
            for next_birthday in Self::get_upcoming_birthdays(&config.talents) {
                let now = Utc::now();

                let time_to_next_birthday = next_birthday.birthday - now;

                info!(
                    "Next birthday is {} {}.",
                    next_birthday.user,
                    HumanTime::from(time_to_next_birthday)
                );

                sleep(time_to_next_birthday.to_std().context(here!())?).await;

                notifier_sender
                    .send(DiscordMessageData::Birthday(next_birthday))
                    .await
                    .context(here!())?;
            }
        }
    }

    fn get_upcoming_birthdays(users: &[Talent]) -> Vec<Birthday> {
        let mut birthday_queue = users
            .iter()
            .map(|u| Birthday {
                user: u.english_name.clone(),
                birthday: u.get_next_birthday(),
            })
            .collect::<Vec<_>>();

        birthday_queue.sort_unstable_by_key(|b| b.birthday);
        birthday_queue
    }

    pub fn get_birthdays(users: &[Talent]) -> Vec<BirthdayRef> {
        let mut birthday_queue = users
            .iter()
            .map(|u| BirthdayRef {
                user: u,
                birthday: u.get_next_birthday(),
            })
            .collect::<Vec<_>>();

        birthday_queue.sort_unstable_by_key(|b| b.birthday);
        birthday_queue
    }
}

#[derive(Debug, Clone)]
pub struct Birthday {
    pub user: String,
    pub birthday: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct BirthdayRef<'a> {
    pub user: &'a Talent,
    pub birthday: DateTime<Utc>,
}
