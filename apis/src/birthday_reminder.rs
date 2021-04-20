use anyhow::Context;
use chrono::prelude::*;
use chrono_humanize::HumanTime;
use log::{error, info};
use tokio::{sync::mpsc::Sender, time::sleep};

use super::discord_api::DiscordMessageData;
use utility::{
    config::{self, User},
    here,
};

pub struct BirthdayReminder {}

impl BirthdayReminder {
    pub async fn start(config: config::Config, notifier_sender: Sender<DiscordMessageData>) {
        tokio::spawn(async move {
            match Self::run(config, notifier_sender).await {
                Ok(()) => (),
                Err(e) => error!("{:#}", e),
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

                sleep(time_to_next_birthday.to_std().context(here!())?).await;

                notifier_sender
                    .send(DiscordMessageData::Birthday(next_birthday))
                    .await
                    .context(here!())?;
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

    pub fn get_birthdays(users: &[User]) -> Vec<BirthdayRef> {
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

#[derive(Debug)]
pub struct Birthday {
    pub user: String,
    pub birthday: DateTime<Utc>,
}

#[derive(Debug)]
pub struct BirthdayRef<'a> {
    pub user: &'a User,
    pub birthday: DateTime<Utc>,
}
