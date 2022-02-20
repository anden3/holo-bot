use std::{collections::HashMap, sync::Arc};

use chrono::Utc;
use futures::StreamExt;
use rusqlite::{params_from_iter, ToSql};
use tokio::sync::mpsc;
use tokio_util::time::DelayQueue;
use tracing::{error, info, instrument};

use utility::config::{
    Config, Database, DatabaseHandle, DatabaseOperations, EntryEvent, Reminder, ReminderFrequency,
};

use crate::discord_api::DiscordMessageData;

pub struct ReminderNotifier;

impl ReminderNotifier {
    #[instrument(skip(config, notifier_sender, reminder_receiver))]
    pub async fn start(
        config: Arc<Config>,
        notifier_sender: mpsc::Sender<DiscordMessageData>,
        reminder_receiver: mpsc::Receiver<EntryEvent<u32, Reminder>>,
    ) {
        tokio::spawn(async move {
            if let Err(e) =
                Self::reminder_handler(&config.database, notifier_sender, reminder_receiver).await
            {
                error!("{:#}", e);
            }

            info!(task = "Reminder notifier", "Shutting down.");
        });
    }

    #[instrument(skip(database, notifier_sender, reminder_receiver))]
    async fn reminder_handler(
        database: &Database,
        notifier_sender: mpsc::Sender<DiscordMessageData>,
        mut reminder_receiver: mpsc::Receiver<EntryEvent<u32, Reminder>>,
    ) -> anyhow::Result<()> {
        let handle = database.get_handle()?;

        Vec::<Reminder>::create_table(&handle)?;
        let saved_reminders = Vec::<Reminder>::load_from_database(&handle)?;

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

                    if let Err(e) = reminders_vec.save_to_database(&handle) {
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
                            continue;
                        }
                    };

                    let (key, reminder) = match reminders.get_mut(&reminder_id) {
                        Some(r) => r,
                        None => {
                            continue;
                        }
                    };

                    if let Err(e) = notifier_sender.send(DiscordMessageData::Reminder(reminder.clone())).await {
                        error!("{:#}", e);
                    }

                    let time_offset = match &reminder.frequency {
                        ReminderFrequency::Once => {
                            reminders.remove(&reminder_id);

                            let save_result = match &handle {
                                DatabaseHandle::SQLite(h) => h
                                    .execute(
                                        "DELETE FROM Reminders WHERE reminder_id == ?", [reminder_id],
                                    )
                            };

                            if let Err(e) = save_result {
                                error!("{:#}", e);
                            }
                            continue;
                        }

                        ReminderFrequency::Daily => {
                            chrono::Duration::days(1)
                        }
                        ReminderFrequency::Weekly => {
                            chrono::Duration::weeks(1)
                        }
                        ReminderFrequency::Monthly => {
                            chrono::Duration::days(30)
                        }
                        ReminderFrequency::Yearly => {
                            chrono::Duration::days(365)
                        }
                    };

                    reminder.time = reminder.time + time_offset;
                    *key = reminder_queue.insert(reminder_id, time_offset.to_std().unwrap());

                    let save_result = match &handle {
                        DatabaseHandle::SQLite(h) => h
                            .execute(
                                "UPDATE Reminders SET reminder = ? WHERE reminder_id == ?",
                                {
                                    let parameters: Vec<&dyn ToSql> = vec![reminder, &reminder_id];
                                    params_from_iter(parameters)
                                },
                            )
                    };

                    if let Err(e) = save_result {
                        error!("{:#}", e);
                    }
                }

                e = tokio::signal::ctrl_c() => {
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
