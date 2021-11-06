use serenity::model::id::ChannelId;
use songbird::tracks::TrackState;
use tokio::sync::mpsc::Sender;

use super::{metadata::TrackMetaDataFull, parameter_types::*, prelude::*};
use crate::impl_error_variants;

#[derive(Debug, Clone)]
pub enum QueueError {
    AccessDenied,
    NotInVoiceChannel,
    Other(String),
}

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
    pub description: Option<String>,
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
    Error(QueueError),
    Terminated,
}

#[derive(Debug, Clone)]
pub enum QueuePlayNowEvent {
    Playing(TrackMin),
    Error(QueueError),
}

#[derive(Debug, Clone)]
pub enum QueueEnqueueEvent {
    TrackEnqueuedBacklog(String),
    TrackEnqueued(TrackMin, Duration),
    TrackEnqueuedTop(TrackMin),
    PlaylistProcessingStart(PlaylistMin),
    PlaylistProcessingProgress(TrackMin),
    PlaylistProcessingEnd,
    Error(QueueError),
}

#[derive(Debug, Clone)]
pub enum QueueSkipEvent {
    TracksSkipped { count: usize },
    Error(QueueError),
}

#[derive(Debug, Clone)]
pub enum QueueRemovalEvent {
    DuplicatesRemoved { count: usize },
    UserPurged { user_id: UserId, count: usize },
    QueueCleared { count: usize },
    TracksRemoved { count: usize },
    Error(QueueError),
}

#[derive(Debug, Clone)]
pub enum QueueShuffleEvent {
    QueueShuffled,
    Error(QueueError),
}

#[derive(Debug, Clone)]
pub enum QueuePlayStateEvent {
    Playing,
    Paused,
    StartedLooping,
    StoppedLooping,
    StateAlreadySet,
    Error(QueueError),
}

#[derive(Debug, Clone)]
pub enum QueueVolumeEvent {
    VolumeChanged(f32),
    Error(QueueError),
}

#[derive(Debug, Clone)]
pub enum QueueNowPlayingEvent {
    NowPlaying(Option<TrackMin>),
    Error(QueueError),
}

#[derive(Debug, Clone)]
pub enum QueueShowEvent {
    CurrentQueue(Vec<QueueItem<TrackMetaDataFull>>),
    Error(QueueError),
}

#[derive(Debug, Clone)]
pub enum QueueUpdate {
    PlayNow(UserId, Sender<QueuePlayNowEvent>, EnqueuedItem),
    Enqueued(UserId, Sender<QueueEnqueueEvent>, EnqueueType),
    EnqueueTop(UserId, Sender<QueueEnqueueEvent>, EnqueuedItem),
    Skip(UserId, Sender<QueueSkipEvent>, usize),
    RemoveTracks(
        UserId,
        Sender<QueueRemovalEvent>,
        ProcessedQueueRemovalCondition,
    ),
    Shuffle(UserId, Sender<QueueShuffleEvent>),
    ChangePlayState(UserId, Sender<QueuePlayStateEvent>, PlayStateChange),
    ChangeVolume(UserId, Sender<QueueVolumeEvent>, f32),
    NowPlaying(UserId, Sender<QueueNowPlayingEvent>),
    ShowQueue(UserId, Sender<QueueShowEvent>),

    TrackEnded,
    ClientConnected(UserId),
    ClientDisconnected(UserId),
    GetStateAndExit(Sender<(ChannelId, Option<TrackState>, Vec<EnqueuedItem>)>),
    Terminated,
}

pub(crate) trait HasErrorVariant {
    fn new_error(error: QueueError) -> Self;
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
    QueueNowPlayingEvent,
    QueueShowEvent
];
