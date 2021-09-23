use tokio::sync::mpsc::Sender;

use super::{parameter_types::*, prelude::*};
use crate::impl_error_variants;

#[derive(Debug, Clone)]
pub struct TrackMin {
    pub index: usize,
    pub title: String,
    pub artist: String,
    pub length: Duration,
    pub thumbnail: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PlaylistMin {
    pub title: String,
    pub description: String,
    pub uploader: String,
    pub unlisted: bool,
    pub views: u64,
    pub video_count: u64,
}

#[derive(Debug, Clone)]
pub enum QueueEvent {
    PlaylistProcessingStart(PlaylistMin),
    PlaylistProcessingProgress(TrackMin),
    PlaylistProcessingEnd,
    Error(String),
    Terminated,
}

#[derive(Debug, Clone)]
pub enum QueuePlayNowEvent {
    Playing(TrackMin),
    Error(String),
}

#[derive(Debug, Clone)]
pub enum QueueEnqueueEvent {
    TrackEnqueued(TrackMin),
    TrackEnqueuedTop(TrackMin),
    PlaylistProcessingStart(PlaylistMin),
    PlaylistProcessingProgress(TrackMin),
    PlaylistProcessingEnd,
    Error(String),
}

#[derive(Debug, Clone)]
pub enum QueueSkipEvent {
    TrackSkipped(TrackMin),
    Error(String),
}

#[derive(Debug, Clone)]
pub enum QueueRemovalEvent {
    TrackRemoved(TrackMin),
    DuplicatesRemoved { removed_count: u64 },
    UserPurged { user_id: UserId, removed_count: u64 },
    QueueCleared { removed_count: u64 },
    Error(String),
}

#[derive(Debug, Clone)]
pub enum QueueShuffleEvent {
    QueueShuffled,
    Error(String),
}

#[derive(Debug, Clone)]
pub enum QueuePlayStateEvent {
    Playing(TrackMin),
    Paused(TrackMin),
    StartedLooping(TrackMin),
    StoppedLooping(TrackMin),
    Error(String),
}

#[derive(Debug, Clone)]
pub enum QueueVolumeEvent {
    VolumeChanged(f32),
    Error(String),
}

#[derive(Debug, Clone)]
pub enum QueueShowEvent {
    CurrentQueue(Vec<TrackMin>),
    Error(String),
}

#[derive(Debug, Clone)]
pub(crate) enum QueueUpdate {
    TrackEnded,
    PlayNow(Sender<QueuePlayNowEvent>, EnqueuedItem),
    Enqueued(Sender<QueueEnqueueEvent>, EnqueueType),
    EnqueueTop(Sender<QueueEnqueueEvent>, EnqueuedItem),
    Skip(Sender<QueueSkipEvent>, usize),
    RemoveTracks(Sender<QueueRemovalEvent>, ProcessedQueueRemovalCondition),
    Shuffle(Sender<QueueShuffleEvent>),
    ChangePlayState(Sender<QueuePlayStateEvent>, PlayStateChange),
    ChangeVolume(Sender<QueueVolumeEvent>, f32),
    ShowQueue(Sender<QueueShowEvent>),
    Terminated,
}

pub(crate) trait HasErrorVariant {
    fn new_error(error: String) -> Self;
}

impl_error_variants![
    QueueEvent,
    QueuePlayNowEvent,
    QueueEnqueueEvent,
    QueueSkipEvent,
    QueueRemovalEvent,
    QueueShuffleEvent,
    QueuePlayStateEvent,
    QueueVolumeEvent,
    QueueShowEvent
];
