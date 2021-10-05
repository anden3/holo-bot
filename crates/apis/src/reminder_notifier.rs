use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

use anyhow::Context;
use chrono::{DateTime, Utc};
use holo_bot_macros::clone_variables;
use tokio::{
    sync::{mpsc, watch},
    time::Instant,
};
use tracing::{error, info, instrument};

use utility::{
    config::{
        Config, Database, DatabaseHandle, EntryEvent, LoadFromDatabase, Reminder, SaveToDatabase,
    },
    here,
};

use crate::discord_api::DiscordMessageData;

pub struct ReminderNotifier;

impl ReminderNotifier {
    #[instrument(skip(config, notifier_sender, reminder_receiver, exit_receiver))]
    pub async fn start(
        config: Arc<Config>,
        notifier_sender: mpsc::Sender<DiscordMessageData>,
        reminder_receiver: mpsc::Receiver<EntryEvent<u64, Reminder>>,
        mut exit_receiver: watch::Receiver<bool>,
    ) {
        let (index_sender, index_receiver) = watch::channel(VecDeque::new());
        let (index_delete_tx, index_delete_rx) = mpsc::channel(4);

        tokio::spawn(clone_variables!(config; {
            tokio::select! {
                e = Self::reminder_indexer(&config.database, index_sender, reminder_receiver, index_delete_rx) => {
                    if let Err(e) = e {
                        error!("{:#}", e);
                    }
                }
                e = Self::reminder_notifier(index_receiver, index_delete_tx, notifier_sender) => {
                    if let Err(e) = e {
                        error!("{:#}", e);
                    }
                }
                e = exit_receiver.changed() => {
                    if let Err(e) = e {
                        error!("{:#}", e);
                    }
                }
            }

            info!(task = "Reminder notifier", "Shutting down.");
        }));
    }

    #[instrument(skip(database, reminder_receiver, index_sender, index_delete_receiver))]
    async fn reminder_indexer(
        database: &Database,
        index_sender: watch::Sender<VecDeque<Reminder>>,
        mut reminder_receiver: mpsc::Receiver<EntryEvent<u64, Reminder>>,
        mut index_delete_receiver: mpsc::Receiver<u64>,
    ) -> anyhow::Result<()> {
        let handle = database.get_handle()?;

        let mut reminders = Reminder::load_from_database(&handle)?
            .into_iter()
            .map(|r| (r.id, r))
            .collect::<HashMap<_, _>>();

        Self::send_sorted_index(&reminders, &index_sender, &handle)?;

        loop {
            tokio::select! {
                Some(event) = reminder_receiver.recv() => {
                    match event {
                        EntryEvent::Added { key, value } | EntryEvent::Updated { key, value }=> {
                            reminders.insert(key, value);
                            Self::send_sorted_index(&reminders, &index_sender, &handle)?;
                        },

                        EntryEvent::Removed { key } => {
                            if let Some(removed_reminder) = reminders.remove(&key) {
                                if removed_reminder.time >= Utc::now() {
                                    Self::send_sorted_index(&reminders, &index_sender, &handle)?;
                                }
                            }
                        },
                    }
                }

                Some(ref deleted_reminder) = index_delete_receiver.recv() => {
                    reminders.remove(deleted_reminder);
                }
            }
        }
    }

    fn send_sorted_index(
        index: &HashMap<u64, Reminder>,
        channel: &watch::Sender<VecDeque<Reminder>>,
        handle: &DatabaseHandle,
    ) -> anyhow::Result<()> {
        let mut sorted_reminders = index.values().cloned().collect::<VecDeque<_>>();
        sorted_reminders.make_contiguous().sort();

        sorted_reminders.as_slices().0.save_to_database(handle)?;
        channel.send(sorted_reminders).context(here!())
    }

    #[instrument(skip(index_receiver, index_delete_sender, notifier_sender))]
    async fn reminder_notifier(
        mut index_receiver: watch::Receiver<VecDeque<Reminder>>,
        index_delete_sender: mpsc::Sender<u64>,
        notifier_sender: mpsc::Sender<DiscordMessageData>,
    ) -> anyhow::Result<()> {
        let mut next_notification_time: Option<(DateTime<Utc>, Instant)> = None;
        let mut reminders = VecDeque::new();

        loop {
            tokio::select! {
                Ok(()) = index_receiver.changed() => {
                    let index = index_receiver.borrow();

                    next_notification_time = match index.front() {
                        None => None,
                        Some(Reminder { time, ..}) => {
                            reminders = index.clone();
                            let duration = *time - Utc::now();
                            let instant = Instant::now() + duration.to_std()?;

                            Some((*time, instant))
                        }
                    };
                }

                _ = tokio::time::sleep_until(next_notification_time.unwrap().1), if next_notification_time.is_some() => {
                    let (time, _) = next_notification_time.unwrap();

                    let notices = reminders.drain(
                        0..
                        reminders.iter().position(|r| r.time > time).unwrap_or(0));

                    for reminder in notices {
                        index_delete_sender.send(reminder.id).await?;
                        notifier_sender.send(DiscordMessageData::Reminder(reminder)).await?;
                    }
                }
            }
        }
    }
}
