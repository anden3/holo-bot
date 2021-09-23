use super::{metadata::TrackMetaData, prelude::*};

#[derive(Debug, Clone)]
pub struct EnqueuedItem {
    pub item: String,
    pub metadata: TrackMetaData,
}

#[derive(Debug, Clone)]
pub enum EnqueueType {
    Track(EnqueuedItem),
    Playlist(EnqueuedItem),
}

#[derive(Debug, Clone)]
pub enum ProcessedQueueRemovalCondition {
    All,
    Duplicates,
    Indices(Vec<usize>),
    FromUser(UserId),
}

#[derive(Debug, Clone)]
pub enum PlayStateChange {
    Resume,
    Pause,
    ToggleLoop,
}
