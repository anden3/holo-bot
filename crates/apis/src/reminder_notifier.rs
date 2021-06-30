use std::collections::{HashMap, VecDeque};

use anyhow::Context;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serenity::model::id::{ChannelId, UserId};
use sled::Event;
use strum_macros::{EnumIter, EnumString, ToString};
use tokio::{
    sync::{mpsc, watch},
    time::Instant,
};
use tracing::{error, info, instrument};

use utility::{config::Config, here};

use crate::discord_api::DiscordMessageData;

pub struct ReminderNotifier;

impl ReminderNotifier {
    #[instrument(skip(config, notifier_sender, exit_receiver))]
    pub async fn start(
        config: Config,
        notifier_sender: mpsc::Sender<DiscordMessageData>,
        mut exit_receiver: watch::Receiver<bool>,
    ) {
        let (index_sender, index_receiver) = watch::channel(VecDeque::new());
        let (index_delete_tx, index_delete_rx) = mpsc::channel(4);

        tokio::spawn(async move {
            tokio::select! {
                e = Self::reminder_indexer(config, index_sender, index_delete_rx) => {
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
        });
    }

    #[instrument(skip(config, index_sender, index_delete_receiver))]
    async fn reminder_indexer(
        config: Config,
        index_sender: watch::Sender<VecDeque<Reminder>>,
        mut index_delete_receiver: mpsc::Receiver<u64>,
    ) -> anyhow::Result<()> {
        let tree: sled::Tree = config
            .database
            .as_ref()
            .unwrap()
            .open_tree("reminders")
            .context(here!())?;

        let mut subscriber = tree.watch_prefix(vec![]);

        let mut reminders = tree
            .iter()
            .filter_map(|r| r.ok())
            .map(|(_, r)| bincode::deserialize::<Reminder>(&r).map(|r| (r.id, r)))
            .collect::<Result<HashMap<_, _>, _>>()?;

        Self::send_sorted_index(&reminders, &index_sender)?;

        loop {
            tokio::select! {
                Some(event) = (&mut subscriber) => {
                    match event {
                        Event::Insert { key, value } => {
                            reminders.insert(
                                bincode::deserialize::<u64>(&key)?,
                                bincode::deserialize::<Reminder>(&value)?);

                            Self::send_sorted_index(&reminders, &index_sender)?;
                        },

                        Event::Remove { key } => {
                            let id = bincode::deserialize::<u64>(&key)?;

                            if let Some(removed_reminder) = reminders.remove(&id) {
                                if removed_reminder.time >= Utc::now() {
                                    Self::send_sorted_index(&reminders, &index_sender)?;
                                }
                            }
                        },
                    }
                }

                Some(deleted_reminder) = index_delete_receiver.recv() => {
                    tree.remove(bincode::serialize(&deleted_reminder)?)?;
                }
            }
        }
    }

    fn send_sorted_index(
        index: &HashMap<u64, Reminder>,
        channel: &watch::Sender<VecDeque<Reminder>>,
    ) -> anyhow::Result<()> {
        let mut sorted_reminders = index.values().cloned().collect::<VecDeque<_>>();
        sorted_reminders.make_contiguous().sort();

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

#[derive(Debug, Clone, Serialize, Deserialize, Eq)]
pub struct Reminder {
    pub id: u64,
    pub message: String,
    pub time: DateTime<Utc>,
    pub subscribers: Vec<ReminderSubscriber>,
}

impl PartialEq for Reminder {
    fn eq(&self, other: &Self) -> bool {
        self.id.eq(&other.id)
    }
}

impl PartialOrd for Reminder {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.time.partial_cmp(&other.time)
    }
}

impl Ord for Reminder {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.time.cmp(&other.time)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct ReminderSubscriber {
    pub user: UserId,
    pub location: ReminderLocation,
}

#[derive(
    Debug,
    Clone,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    EnumIter,
    ToString,
    EnumString,
)]
pub enum ReminderLocation {
    DM,
    Channel(ChannelId),
}
