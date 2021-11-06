use std::fmt::Display;

use chrono::{DateTime, Duration, Utc};
use holodex::model::{id::VideoId, VideoStatus};

use crate::config::Talent;

#[derive(Debug, Clone)]
pub struct Livestream {
    pub id: VideoId,
    pub title: String,
    pub thumbnail: String,
    pub url: String,
    pub streamer: Talent,

    pub created_at: DateTime<Utc>,
    pub start_at: DateTime<Utc>,

    pub duration: Option<Duration>,
    pub state: VideoStatus,
}

impl Display for Livestream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{}][{:?}] {} by {}",
            self.id, self.state, self.title, self.streamer.english_name
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
    Ended(VideoId),
    Unscheduled(VideoId),
    Renamed(VideoId, String),
    Rescheduled(VideoId, DateTime<Utc>),
}
