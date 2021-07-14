use std::fmt::Display;

use chrono::{DateTime, Utc};

use crate::config::User;

#[derive(Debug, Clone)]
pub struct Livestream {
    pub id: u32,
    pub title: String,
    pub thumbnail: String,
    pub url: String,
    pub streamer: User,

    pub created_at: DateTime<Utc>,
    pub start_at: DateTime<Utc>,

    pub duration: Option<u32>,
    pub state: StreamState,
}

impl Display for Livestream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{}][{:?}] {} by {}",
            self.id, self.state, self.title, self.streamer.display_name
        )
    }
}

impl PartialEq for Livestream {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum StreamState {
    Scheduled,
    Live,
    Ended,
}

#[derive(Debug, Clone)]
pub enum StreamUpdate {
    Scheduled(Livestream),
    Started(Livestream),
    Ended(Livestream),
}
