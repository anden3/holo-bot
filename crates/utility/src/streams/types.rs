use std::fmt::Display;

use chrono::{DateTime, Duration, Utc};
use holodex::model::{id::VideoId, Video, VideoStatus};

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

impl Livestream {
    pub fn from_video_and_talent(video: Video, talent: &Talent) -> Livestream {
        let id = video.id.clone();
        let thumbnail = format!("https://i3.ytimg.com/vi/{}/maxresdefault.jpg", &video.id);
        let url = format!("https://youtube.com/watch?v={}", &video.id);

        Livestream {
            id,
            title: video.title.clone(),
            thumbnail,
            created_at: video.available_at,
            start_at: video
                .live_info
                .start_scheduled
                .unwrap_or(video.available_at),
            duration: video
                .duration
                .and_then(|d| d.is_zero().then(|| None).unwrap_or(Some(d))),
            streamer: talent.clone(),
            state: video.status,
            url,
        }
    }
}

impl Display for Livestream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{}][{:?}] {} by {}",
            self.id, self.state, self.title, self.streamer.name
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
