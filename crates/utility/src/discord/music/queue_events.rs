use super::{parameter_types::*, prelude::*};

#[derive(Debug, Clone)]
pub enum QueueEvent {
    PlaylistProcessingStart {
        title: String,
        description: String,
        unlisted: bool,
        views: u64,
        video_count: u64,
    },
    PlaylistProcessingProgress {
        index: u64,
        title: String,
        length: Duration,
        thumbnail: Option<String>,
    },
    PlaylistProcessingEnd,
    Error(String),
    Terminated,
}

#[derive(Debug, Clone)]
pub(crate) enum QueueUpdate {
    TrackEnded,
    PlayNow(EnqueuedItem),
    Enqueued(EnqueueType),
    EnqueueTop(EnqueuedItem),
    RemoveTracks(QueueRemovalCondition),
    ChangePlayState(PlayStateChange),
    ChangeVolume(f32),
    Terminated,
}
