use std::{collections::HashMap, sync::Arc};

use chrono::Utc;
use futures::StreamExt;
use tokio::sync::{mpsc, watch};
use tokio_util::time::DelayQueue;
use tracing::{error, info, instrument, warn};

use utility::config::{Config, Database, EntryEvent, LoadFromDatabase, Reminder, SaveToDatabase};

use crate::discord_api::DiscordMessageData;

pub struct ReminderNotifier;

impl ReminderNotifier {
    #[instrument(skip(config, notifier_sender, reminder_receiver, exit_receiver))]
    pub async fn start(
        config: Arc<Config>,
        notifier_sender: mpsc::Sender<DiscordMessageData>,
        reminder_receiver: mpsc::Receiver<EntryEvent<u64, Reminder>>,
        exit_receiver: watch::Receiver<bool>,
    ) {
        tokio::spawn(async move {
            if let Err(e) = Self::reminder_indexer(
                &config.database,
                notifier_sender,
                reminder_receiver,
                exit_receiver,
            )
            .await
            {
                error!("{:#}", e);
            }

            info!(task = "Reminder notifier", "Shutting down.");
        });
    }

    #[instrument(skip(database, notifier_sender, reminder_receiver, exit_receiver))]
    async fn reminder_indexer(
        database: &Database,
        notifier_sender: mpsc::Sender<DiscordMessageData>,
        mut reminder_receiver: mpsc::Receiver<EntryEvent<u64, Reminder>>,
        mut exit_receiver: watch::Receiver<bool>,
    ) -> anyhow::Result<()> {
        let handle = database.get_handle()?;

        let saved_reminders = Reminder::load_from_database(&handle)?;
        let mut reminders = HashMap::with_capacity(saved_reminders.len());
        let mut reminder_queue = DelayQueue::with_capacity(saved_reminders.len());

        for reminder in saved_reminders {
            let remind_in = match (reminder.time - Utc::now()).to_std() {
                Ok(duration) => duration,
                Err(e) => {
                    error!("{:#}", e);
                    continue;
                }
            };

            let key = reminder_queue.insert(reminder.id, remind_in);
            reminders.insert(reminder.id, (key, reminder));
        }

        loop {
            tokio::select! {
                Some(event) = reminder_receiver.recv() => {
                    match event {
                        EntryEvent::Added { key, value } => {
                            let remind_in = match (value.time - Utc::now()).to_std() {
                                Ok(duration) => duration,
                                Err(e) => {
                                    error!("{:#}", e);
                                    continue;
                                }
                            };

                            let queue_key = reminder_queue.insert(key, remind_in);
                            reminders.insert(key, (queue_key, value));
                        },

                        EntryEvent::Updated { key, value } => {
                            if let Some((queue_key, reminder)) = reminders.get_mut(&key) {
                                if reminder.time != value.time {
                                    let remind_in = match (value.time - Utc::now()).to_std() {
                                        Ok(duration) => duration,
                                        Err(e) => {
                                            error!("{:#}", e);
                                            continue;
                                        }
                                    };

                                    reminder_queue.reset(queue_key, remind_in);
                                }

                                *reminder = value;
                            }
                        }

                        EntryEvent::Removed { key } => {
                            if let Some((key, _)) = reminders.remove(&key) {
                                reminder_queue.remove(&key);
                            }
                        },
                    }

                    let reminders_vec = reminders.values().map(|(_, reminder)| reminder).cloned().collect::<Vec<_>>();

                    if let Err(e) = reminders_vec.as_slice().save_to_database(&handle) {
                        error!("{:#}", e);
                    }
                }

                reminder = reminder_queue.next() => {
                    let reminder_id = match reminder {
                        Some(Ok(r)) => r.into_inner(),
                        Some(Err(e)) => {
                            error!("{:#}", e);
                            continue;
                        }
                        None => {
                            warn!("Reminder queue returned None!");
                            continue;
                        }
                    };

                    if let Some((_, reminder)) = reminders.remove(&reminder_id) {
                        if let Err(e) = notifier_sender.send(DiscordMessageData::Reminder(reminder)).await {
                            error!("{:#}", e);
                        }
                    }

                    let reminders_vec = reminders.values().map(|(_, reminder)| reminder).cloned().collect::<Vec<_>>();

                    if let Err(e) = reminders_vec.as_slice().save_to_database(&handle) {
                        error!("{:#}", e);
                    }
                }

                e = exit_receiver.changed() => {
                    if let Err(e) = e {
                        error!("{:#}", e);
                    }

                    break;
                }
            }
        }

        Ok(())
    }
}
