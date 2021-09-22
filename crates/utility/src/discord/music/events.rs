use super::{parameter_types::EnqueueType, prelude::*};

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
    Enqueued(EnqueueType),
    VolumeChanged(f32),
    Terminated,
}
