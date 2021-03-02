use std::time::Duration;

use chrono::prelude::*;
use job_scheduler::{Job, JobScheduler};
use tokio::{
    sync::mpsc::Sender,
    time::{self, sleep},
};

use super::config;
use super::discord_api::DiscordMessageData;

pub struct BirthdayReminder {}

impl BirthdayReminder {
    pub async fn start(config: config::Config, notifier_sender: Sender<DiscordMessageData>) {
        tokio::spawn(async move {
            BirthdayReminder::run(config, notifier_sender)
                .await
                .unwrap();
        });
    }

    async fn run(
        config: config::Config,
        notifier_sender: Sender<DiscordMessageData>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut schedule = JobScheduler::new();

        let year = Utc::now().year();

        for i in 0..config.users.len() {
            let user = &config.users[i];
            let (day, month) = user.birthday;

            let birthday = user
                .timezone
                .ymd(year, month, day)
                .and_hms(12, 0, 0)
                .with_timezone(&Utc);

            let cron_string = birthday.format("%S %M %H %d %m *").to_string();

            schedule.add(Job::new(cron_string.parse().unwrap(), || {
                notifier_sender
                    .blocking_send(DiscordMessageData::Birthday(Birthday { user: i }))
                    .unwrap();
            }));
        }

        let mut schedule_timer = time::interval(Duration::from_secs(60));

        loop {
            schedule.tick();
            schedule_timer.tick().await;
        }
    }
}

#[derive(Debug)]
pub struct Birthday {
    pub user: usize,
}
