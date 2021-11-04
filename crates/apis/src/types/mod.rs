use chrono::{DateTime, Utc};
use holodex::model::id::VideoId;

pub(crate) enum VideoUpdate {
    Scheduled(VideoId),
    Started(VideoId),
    Ended(VideoId),
    Unscheduled(VideoId),
    Renamed {
        id: VideoId,
        new_name: String,
    },
    Rescheduled {
        id: VideoId,
        new_start: DateTime<Utc>,
    },
}
