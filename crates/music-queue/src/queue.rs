use futures::stream::{FuturesOrdered, FuturesUnordered};
use itertools::Itertools;
use nanorand::Rng;
use serenity::{
    client::Cache,
    http::Http,
    model::{channel::Channel, id::ChannelId},
};
use songbird::{
    driver::Bitrate,
    input::Restartable,
    tracks::{LoopState, PlayMode, TrackState},
    CoreEvent, TrackEvent,
};

use super::{event_handlers::*, events::*, metadata::*, parameter_types::*, prelude::*};
use crate::{add_bindings, delegate_events};

#[derive(Debug, Clone)]
pub struct Queue {
    _inner: Arc<QueueInner>,
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct QueueInner {
    update_sender: mpsc::Sender<QueueUpdate>,
    event_sender: broadcast::Sender<QueueEvent>,
    cancellation_token: CancellationToken,
}

impl Queue {
    pub fn new(
        manager: Arc<Songbird>,
        guild_id: &GuildId,
        discord_http: Arc<Http>,
        discord_cache: Arc<Cache>,
    ) -> Self {
        let (update_sender, update_receiver) = mpsc::channel(16);
        let (event_sender, _) = broadcast::channel(16);

        let guild_id = *guild_id;

        let cancellation_token = CancellationToken::new();
        let child_token = cancellation_token.child_token();

        let update_sender_clone = update_sender.clone();

        QueueHandler::start(
            manager,
            guild_id,
            discord_http,
            discord_cache,
            update_receiver,
            update_sender_clone,
            child_token,
        );

        Self {
            _inner: Arc::new(QueueInner {
                update_sender,
                event_sender,
                cancellation_token,
            }),
        }
    }

    pub fn load(
        manager: Arc<Songbird>,
        guild_id: &GuildId,
        discord_http: Arc<Http>,
        discord_cache: Arc<Cache>,
        state: Option<TrackState>,
        tracks: &[EnqueuedItem],
    ) -> Self {
        let (update_sender, update_receiver) = mpsc::channel(16);
        let (event_sender, _) = broadcast::channel(16);

        let guild_id = *guild_id;

        let cancellation_token = CancellationToken::new();
        let child_token = cancellation_token.child_token();

        let update_sender_clone = update_sender.clone();

        QueueHandler::load(
            manager,
            guild_id,
            state,
            tracks.to_vec(),
            discord_http,
            discord_cache,
            update_receiver,
            update_sender_clone,
            child_token,
        );

        Self {
            _inner: Arc::new(QueueInner {
                update_sender,
                event_sender,
                cancellation_token,
            }),
        }
    }

    pub async fn save_and_exit(
        &self,
    ) -> Option<(ChannelId, Option<TrackState>, Vec<EnqueuedItem>)> {
        let (tx, mut rx) = mpsc::channel(1);

        let _ = self
            .update_sender
            .send(QueueUpdate::GetStateAndExit(tx))
            .await;
        rx.recv().await
    }

    add_bindings! {
        enqueue: |enqueue_type: EnqueueType| = QueueUpdate::Enqueued => QueueEnqueueEvent;
        enqueue_top: |track: EnqueuedItem| = QueueUpdate::EnqueueTop => QueueEnqueueEvent;
        play_now: |track: EnqueuedItem| = QueueUpdate::PlayNow => QueuePlayNowEvent;
        skip: |amount: usize| = QueueUpdate::Skip => QueueSkipEvent;
        remove: |condition: ProcessedQueueRemovalCondition| = QueueUpdate::RemoveTracks => QueueRemovalEvent;
        shuffle = QueueUpdate::Shuffle => QueueShuffleEvent;
        set_play_state: |state: PlayStateChange| = QueueUpdate::ChangePlayState => QueuePlayStateEvent;
        set_volume: |volume: f32| = QueueUpdate::ChangeVolume => QueueVolumeEvent;
        now_playing = QueueUpdate::NowPlaying => QueueNowPlayingEvent;
        show = QueueUpdate::ShowQueue => QueueShowEvent;
    }
}

impl Deref for Queue {
    type Target = QueueInner;

    fn deref(&self) -> &Self::Target {
        &self._inner
    }
}

impl Drop for QueueInner {
    fn drop(&mut self) {
        self.cancellation_token.cancel();

        /* if let Err(e) = self.update_sender.try_send(QueueUpdate::Terminated) {
            error!(err = ?e, "Failed to request queue termination.");
        } */
    }
}

struct QueueHandler {
    buffer: TrackQueue,
    remainder: VecDeque<EnqueuedItem>,
    users: HashMap<UserId, UserData>,

    guild_id: GuildId,
    manager: Arc<Songbird>,
    handler: Arc<Mutex<Call>>,
    discord_http: Arc<Http>,
    discord_cache: Arc<Cache>,

    update_sender: mpsc::Sender<QueueUpdate>,

    extractor: ytextract::Client,
    volume: f32,
}

impl QueueHandler {
    const MAX_QUEUE_LENGTH: usize = 3;
    const MAX_PLAYLIST_LENGTH: usize = 1000;

    pub fn start(
        manager: Arc<Songbird>,
        guild_id: GuildId,
        discord_http: Arc<Http>,
        discord_cache: Arc<Cache>,
        update_receiver: mpsc::Receiver<QueueUpdate>,
        update_sender: mpsc::Sender<QueueUpdate>,
        cancellation_token: CancellationToken,
    ) {
        let handler = match manager.get(guild_id) {
            Some(h) => h,
            None => {
                error!("Failed to get call when initializing queue!");
                return;
            }
        };

        let handler = QueueHandler {
            buffer: TrackQueue::new(),
            remainder: VecDeque::with_capacity(32),
            manager,
            handler,
            discord_http,
            discord_cache,
            update_sender,
            guild_id,
            users: HashMap::new(),
            extractor: ytextract::Client::new(),
            volume: 0.5f32,
        };

        tokio::spawn(async move {
            handler
                .handler_loop(None, update_receiver, cancellation_token)
                .await
        });
    }

    // Yes, I know it's bad, but I kinda need all of these lol.
    #[allow(clippy::too_many_arguments)]
    pub fn load(
        manager: Arc<Songbird>,
        guild_id: GuildId,
        state: Option<TrackState>,
        tracks: Vec<EnqueuedItem>,
        discord_http: Arc<Http>,
        discord_cache: Arc<Cache>,
        update_receiver: mpsc::Receiver<QueueUpdate>,
        update_sender: mpsc::Sender<QueueUpdate>,
        cancellation_token: CancellationToken,
    ) {
        let handler = match manager.get(guild_id) {
            Some(h) => h,
            None => {
                error!("Failed to get call when initializing queue!");
                return;
            }
        };

        let handler = QueueHandler {
            buffer: TrackQueue::new(),
            remainder: tracks.into(),
            manager,
            handler,
            discord_http,
            discord_cache,
            update_sender,
            guild_id,
            users: HashMap::new(),
            extractor: ytextract::Client::new(),
            volume: state.map(|s| s.volume).unwrap_or(0.5),
        };

        tokio::spawn(async move {
            handler
                .handler_loop(state, update_receiver, cancellation_token)
                .await
        });
    }

    async fn handler_loop(
        mut self,
        start_state: Option<TrackState>,
        mut update_receiver: mpsc::Receiver<QueueUpdate>,
        cancellation_token: CancellationToken,
    ) {
        {
            let mut call = self.handler.lock().await;

            call.set_bitrate(Bitrate::Max);

            call.add_global_event(
                Event::Core(CoreEvent::ClientConnect),
                GlobalEvent {
                    channel: self.update_sender.clone(),
                },
            );

            call.add_global_event(
                Event::Core(CoreEvent::ClientDisconnect),
                GlobalEvent {
                    channel: self.update_sender.clone(),
                },
            );

            let channel = ChannelId(call.current_channel().unwrap().0);

            match channel.to_channel(&self.discord_http).await {
                Ok(Channel::Guild(ch)) => {
                    let connected_members = match ch.members(&self.discord_cache).await {
                        Ok(m) => m,
                        Err(e) => {
                            error!("Failed to get members: {}", e);
                            return;
                        }
                    };

                    for member in connected_members {
                        debug!(user = %member.user.tag(), "Adding connected user.");

                        self.users.insert(
                            member.user.id,
                            UserData {
                                name: member.user.tag(),
                                colour: member.colour(&self.discord_cache).unwrap_or_default(),
                            },
                        );
                    }
                }
                Ok(_) => {
                    error!("Failed to get guild channel!");
                    return;
                }
                Err(e) => {
                    error!("Failed to get guild channel: {:?}", e);
                }
            }
        }

        if !self.remainder.is_empty() {
            let new_queue_length = std::cmp::min(Self::MAX_QUEUE_LENGTH, self.remainder.len());
            let new_remainder = self.remainder.drain(..new_queue_length).collect::<Vec<_>>();

            let (dummy_sender, _) = mpsc::channel(new_queue_length);

            for track in new_remainder {
                if let Err(e) = self.enqueue(&dummy_sender, EnqueueType::Track(track)).await {
                    error!("Failed to enqueue track: {:?}", e);
                    continue;
                }
            }

            if let Some(state) = start_state {
                if let Some(current) = self.buffer.current() {
                    let result = match state.playing {
                        PlayMode::Play => current.play(),
                        PlayMode::Pause => current.pause(),
                        PlayMode::Stop => self.buffer.skip(),
                        PlayMode::End => self.buffer.skip(),
                        /* p => {
                            error!(play_mode = ?p, "Invalid play mode!");
                            continue;
                        } */
                    };

                    let result = result.and(match state.loops {
                        LoopState::Infinite => current.enable_loop(),
                        LoopState::Finite(0) => current.disable_loop(),
                        LoopState::Finite(n) => current.loop_for(n),
                    });

                    let result = if current.is_seekable() {
                        result.and(current.seek_time(state.position))
                    } else {
                        result
                    };

                    if let Err(e) = result {
                        error!("Failed to set state: {:?}", e);
                    }
                }
            }
        }

        while let Some(update) = tokio::select! {
           update = update_receiver.recv() => update,
           _ = cancellation_token.cancelled() => Some(QueueUpdate::Terminated),
        } {
            trace!(?update, "Received update");

            match update {
                QueueUpdate::ClientConnected(user_id) => {
                    let member = match self.guild_id.member(&self.discord_http, &user_id).await {
                        Ok(m) => m,
                        Err(e) => {
                            error!(?e, "Failed to get member data when client connected!");
                            continue;
                        }
                    };

                    debug!(user = %member.user.tag(), "Adding connected user.");

                    self.users.insert(
                        user_id,
                        UserData {
                            name: member.user.tag(),
                            colour: member.colour(&self.discord_cache).unwrap_or_default(),
                        },
                    );
                }

                QueueUpdate::ClientDisconnected(user_id) => {
                    if let Some(user) = self.users.remove(&user_id) {
                        debug!(user = %user.name, "Removing user.");
                    } else {
                        warn!(%user_id, "User not in index disconnected!");
                    }
                }

                QueueUpdate::TrackEnded => {
                    if let Err(e) = self.track_ended().await {
                        error!(err = ?e, "Track ended error!");
                    }
                }

                QueueUpdate::GetStateAndExit(sender) => {
                    let state = if let Some(track) = self.buffer.current() {
                        match track.get_info().await {
                            Ok(info) => Some(info),
                            Err(e) => {
                                error!(err = ?e, "Failed to get track info!");
                                None
                            }
                        }
                    } else {
                        None
                    };

                    let channel = match self
                        .handler
                        .lock()
                        .await
                        .current_channel()
                        .map(|c| ChannelId(c.0))
                    {
                        Some(c) => c,
                        None => {
                            continue;
                        }
                    };

                    let queue = self.buffer.current_queue();

                    let track_extra_metadata = queue
                        .iter()
                        .map(|t| t.typemap().read())
                        .collect::<FuturesOrdered<_>>()
                        .map(|f| f.get::<TrackMetaData>().unwrap().to_owned())
                        .collect::<Vec<_>>()
                        .await;

                    let mut tracks = queue
                        .into_iter()
                        .zip(track_extra_metadata.into_iter())
                        .map(|(t, meta)| {
                            let track_metadata = t.metadata();

                            EnqueuedItem {
                                item: track_metadata.source_url.clone().unwrap_or_default(),
                                metadata: meta,
                                extracted_metadata: Some(ExtractedMetaData {
                                    title: track_metadata.title.clone().unwrap_or_default(),
                                    uploader: track_metadata.channel.clone().unwrap_or_default(),
                                    duration: track_metadata.duration.unwrap_or_default(),
                                    thumbnail: track_metadata.thumbnail.clone(),
                                }),
                            }
                        })
                        .collect::<Vec<_>>();

                    tracks.extend(self.remainder.drain(..));
                    let _ = sender.send((channel, state, tracks)).await;
                    break;
                }

                QueueUpdate::Terminated => {
                    break;
                }

                QueueUpdate::NowPlaying(_user_id, sender) => {
                    if let Err(e) = self.now_playing(&sender).await {
                        Self::report_error(e, &sender).await;
                    }
                }

                _ => {
                    delegate_events! {
                        self, update,
                        enqueue: |enqueued_type| = QueueUpdate::Enqueued,
                        enqueue_top: |item| = QueueUpdate::EnqueueTop,
                        play_now: |item| = QueueUpdate::PlayNow,
                        skip: |amount| = QueueUpdate::Skip,
                        remove_tracks: |condition| = QueueUpdate::RemoveTracks,
                        shuffle: | | = QueueUpdate::Shuffle,
                        change_play_state: |state| = QueueUpdate::ChangePlayState,
                        change_volume: |volume| = QueueUpdate::ChangeVolume,
                        show_queue: | | = QueueUpdate::ShowQueue
                    }
                }
            };
        }

        self.buffer.stop();
        self.remainder.clear();

        match self.manager.remove(self.guild_id).await {
            Ok(()) => debug!("Left voice channel!"),
            Err(e) => {
                error!("{:?}", e);
            }
        }
    }

    async fn enqueue(
        &mut self,
        sender: &mpsc::Sender<QueueEnqueueEvent>,
        enqueued_type: EnqueueType,
    ) -> Result<()> {
        let to_be_enqueued = match enqueued_type {
            EnqueueType::Track(mut t) => {
                t.fetch_metadata(&self.extractor).await;
                vec![t]
            }
            EnqueueType::Playlist(EnqueuedItem {
                item: playlist_id,
                metadata,
                ..
            }) => {
                let id = playlist_id.parse()?;
                let playlist_data = self.extractor.playlist(id).await?;

                let description = match playlist_data.description() {
                    "" => None,
                    s => Some(s.to_string()),
                };

                Self::send_event(
                    sender,
                    QueueEnqueueEvent::PlaylistProcessingStart(PlaylistMin {
                        title: playlist_data.title().to_string(),
                        description,
                        uploader: playlist_data
                            .channel()
                            .map(|c| c.name().to_owned())
                            .unwrap_or_else(|| "Unknown Uploader".to_string()),
                        unlisted: playlist_data.unlisted(),
                        views: playlist_data.views(),
                        video_count: std::cmp::min(
                            playlist_data.length(),
                            Self::MAX_PLAYLIST_LENGTH as u64,
                        ),
                    }),
                )
                .await;

                let videos_processed = AtomicU64::new(0);

                let videos = playlist_data.videos().take(Self::MAX_PLAYLIST_LENGTH);
                futures::pin_mut!(videos);

                let mut to_be_enqueued = Vec::with_capacity(std::cmp::min(
                    Self::MAX_PLAYLIST_LENGTH,
                    playlist_data.length() as usize,
                ));

                while let Some(video) = videos.next().await {
                    let videos_processed = videos_processed.fetch_add(1, Ordering::AcqRel) + 1;

                    let video = match video {
                        Ok(v) => v,
                        Err(e) => {
                            warn!(err = ?e, "Failed to get video from playlist.");
                            continue;
                        }
                    };

                    Self::send_event(
                        sender,
                        QueueEnqueueEvent::PlaylistProcessingProgress(TrackMin {
                            index: videos_processed as usize,
                            title: video.title().to_string(),
                            artist: video.channel().name().to_string(),
                            length: video.length(),
                            thumbnail: video
                                .thumbnails()
                                .first()
                                .map(|t| t.url.as_str().to_string()),
                        }),
                    )
                    .await;

                    to_be_enqueued.push(EnqueuedItem {
                        item: format!("https://youtu.be/{}", video.id()),
                        metadata: metadata.clone(),
                        extracted_metadata: Some(video.into()),
                    });
                }

                Self::send_event(sender, QueueEnqueueEvent::PlaylistProcessingEnd).await;
                to_be_enqueued
            }
        };

        let mut remaining_time = self
            .buffer
            .current_queue()
            .into_iter()
            .map(|t| t.metadata().duration.unwrap_or_default())
            .chain(std::iter::once(Duration::from_secs(
                180 * self.remainder.len() as u64,
            )))
            .sum::<Duration>();

        trace!(tracks = ?to_be_enqueued, "New tracks to be enqueued.");

        // TODO: Use drain filter so we can extend at the end.
        for q in to_be_enqueued {
            if self.buffer.len() >= Self::MAX_QUEUE_LENGTH {
                // Add to remainder.
                Self::send_event(
                    sender,
                    QueueEnqueueEvent::TrackEnqueuedBacklog(q.item.clone()),
                )
                .await;
                self.remainder.push_back(q);
                continue;
            }

            // Add to buffer.
            let track = self.buffer_item(q).await?;
            let track_length = track.length;

            debug!(?track, "Enqueued track!");
            Self::send_event(
                sender,
                QueueEnqueueEvent::TrackEnqueued(track, remaining_time),
            )
            .await;

            remaining_time += track_length;
        }

        Ok(())
    }

    async fn enqueue_top(
        &mut self,
        sender: &mpsc::Sender<QueueEnqueueEvent>,
        item: EnqueuedItem,
    ) -> Result<()> {
        let track = match self.buffer_item(item).await? {
            // No difference adding to top or bottom if there's only 1 or 2 elements in the queue.
            TrackMin { index: 0 | 1, .. } => return Ok(()),
            track => track,
        };

        trace!("Modifying queue.");

        self.buffer.modify_queue(|q| {
            let new_element = q.remove(track.index).unwrap();
            q.insert(1, new_element);
        });

        trace!("Queue modified.");

        debug!(?track, "Enqueued track to the top!");
        Self::send_event(sender, QueueEnqueueEvent::TrackEnqueuedTop(track)).await;

        Ok(())
    }

    async fn play_now(
        &mut self,
        sender: &mpsc::Sender<QueuePlayNowEvent>,
        item: EnqueuedItem,
    ) -> Result<()> {
        self.buffer.pause()?;

        let track = match self.buffer_item(item).await? {
            track @ TrackMin { index: 0, .. } => {
                self.buffer.resume()?;
                Self::send_event(sender, QueuePlayNowEvent::Playing(track)).await;

                return Ok(());
            }
            track => track,
        };

        trace!("Modifying queue.");
        self.buffer.modify_queue(|q| {
            let new_element = q.remove(track.index).unwrap();
            q.insert(0, new_element);
        });
        trace!("Queue modified.");

        self.buffer.resume()?;
        Self::send_event(sender, QueuePlayNowEvent::Playing(track)).await;

        Ok(())
    }

    async fn skip(&mut self, sender: &mpsc::Sender<QueueSkipEvent>, amount: usize) -> Result<()> {
        let buffer_skip_amount = std::cmp::min(amount, self.buffer.len());
        let remainder_skip_amount =
            std::cmp::min(amount - buffer_skip_amount, self.remainder.len());

        let skipped_remainders = (0..remainder_skip_amount)
            .filter_map(|_| {
                trace!(track = ?self.remainder.front(), "Skipping track.");
                self.remainder.pop_front()
            })
            .count();

        let skipped_tracks = (0..buffer_skip_amount)
            .filter_map(|i| {
                let current = self.buffer.current().map(|t| t.metadata().to_owned());

                trace!(track = ?current, "Skipping track.");

                if let Err(e) = self.buffer.skip() {
                    return Some(Err(e.into()));
                }

                current.map(|m| {
                    Ok(TrackMin {
                        index: i + 1,
                        title: m.title.unwrap_or_else(|| "Unknown Title".to_string()),
                        artist: m.artist.unwrap_or_else(|| "Unknown Artist".to_string()),
                        length: m.duration.unwrap_or_default(),
                        thumbnail: m.thumbnail,
                    })
                })
            })
            .collect::<Result<Vec<_>>>()?
            .len()
            + skipped_remainders;

        Self::send_event(
            sender,
            QueueSkipEvent::TracksSkipped {
                count: skipped_tracks,
            },
        )
        .await;

        Ok(())
    }

    async fn remove_tracks(
        &mut self,
        sender: &mpsc::Sender<QueueRemovalEvent>,
        condition: ProcessedQueueRemovalCondition,
    ) -> Result<()> {
        if self.buffer.is_empty() {
            return Ok(());
        }

        if let ProcessedQueueRemovalCondition::All = condition {
            let queue_len = self.buffer.len() + self.remainder.len();

            self.remainder.clear();
            self.buffer.stop();

            Self::send_event(sender, QueueRemovalEvent::QueueCleared { count: queue_len }).await;
            return Ok(());
        }

        let buffered_tracks_to_remove: HashSet<_> = match &condition {
            ProcessedQueueRemovalCondition::All => unreachable!(),
            ProcessedQueueRemovalCondition::Duplicates => self
                .buffer
                .current_queue()
                .iter()
                .filter_map(|t| t.metadata().source_url.as_ref().map(|url| (t, url)))
                .duplicates_by(|(_, url)| *url)
                .map(|(t, _)| t.uuid())
                .collect(),
            ProcessedQueueRemovalCondition::Indices(indices) => indices
                .iter()
                .filter_map(|i| self.buffer.current_queue().get(*i).map(|t| t.uuid()))
                .collect(),
            ProcessedQueueRemovalCondition::FromUser(user_id) => {
                self.buffer
                    .current_queue()
                    .iter()
                    .map(|t| async move {
                        let type_map = t.typemap().read().await;
                        type_map
                            .get::<TrackMetaData>()
                            .and_then(|d| (d.added_by == *user_id).then(|| t.uuid()))
                    })
                    .collect::<FuturesUnordered<_>>()
                    .filter_map(|f| async move { f })
                    .collect()
                    .await
            }
        };

        let unbuffered_tracks_to_remove: HashSet<_> = match &condition {
            ProcessedQueueRemovalCondition::All => unreachable!(),
            ProcessedQueueRemovalCondition::Duplicates => self
                .remainder
                .iter()
                .map(|t| t.item.as_str())
                .duplicates_by(|item| *item)
                .map(|i| i.to_owned())
                .collect(),
            ProcessedQueueRemovalCondition::Indices(indices) => indices
                .iter()
                .filter_map(|i| self.remainder.get(*i).map(|t| t.item.to_owned()))
                .collect(),
            ProcessedQueueRemovalCondition::FromUser(user_id) => self
                .remainder
                .iter()
                .filter(|t| t.metadata.added_by == *user_id)
                .map(|t| t.item.to_owned())
                .collect(),
        };

        if !unbuffered_tracks_to_remove.is_empty() {
            trace!(tracks = ?unbuffered_tracks_to_remove, "Removing unbuffered tracks...");

            self.remainder
                .retain(|t| !unbuffered_tracks_to_remove.contains(&t.item));
        }

        if !buffered_tracks_to_remove.is_empty() {
            trace!(tracks = ?buffered_tracks_to_remove, "Removing buffered tracks...");

            trace!("Modifying queue.");
            self.buffer.modify_queue(|q| -> Result<()> {
                q.iter_mut().try_for_each(|t| {
                    (buffered_tracks_to_remove.contains(&t.uuid()))
                        .then(|| t.stop())
                        .unwrap_or(Ok(()))
                })?;

                q.retain(|t| !buffered_tracks_to_remove.contains(&t.uuid()));

                Ok(())
            })?;
            trace!("Queue modified.");
        }

        let count = buffered_tracks_to_remove.len() + unbuffered_tracks_to_remove.len();

        let event = match condition {
            ProcessedQueueRemovalCondition::All => unreachable!(),
            ProcessedQueueRemovalCondition::Duplicates => {
                QueueRemovalEvent::DuplicatesRemoved { count }
            }
            ProcessedQueueRemovalCondition::Indices(_) => {
                QueueRemovalEvent::TracksRemoved { count }
            }
            ProcessedQueueRemovalCondition::FromUser(user_id) => {
                QueueRemovalEvent::UserPurged { user_id, count }
            }
        };

        Self::send_event(sender, event).await;

        Ok(())
    }

    async fn shuffle(&mut self, sender: &mpsc::Sender<QueueShuffleEvent>) -> Result<()> {
        if self.buffer.len() <= 2 {
            return Ok(());
        }

        {
            let mut rng = nanorand::tls_rng();

            let slice = self.remainder.make_contiguous();
            rng.shuffle(slice);

            trace!("Modifying queue.");
            self.buffer.modify_queue(|q| {
                let (_, slice) = q.make_contiguous().split_at_mut(1);
                rng.shuffle(slice);
            });
            trace!("Queue modified.");
        }

        Self::send_event(sender, QueueShuffleEvent::QueueShuffled).await;

        Ok(())
    }

    async fn change_play_state(
        &mut self,
        sender: &mpsc::Sender<QueuePlayStateEvent>,
        state: PlayStateChange,
    ) -> Result<()> {
        let current = match self.buffer.current() {
            Some(c) => c,
            None => return Ok(()),
        };

        let current_state = current.get_info().await?;

        debug!(current = ?current_state, desired = ?state, "Play state change requested.");

        let event = match (current_state, state) {
            (
                TrackState {
                    playing: PlayMode::Pause,
                    ..
                },
                PlayStateChange::Resume,
            ) => current.play().map(|_| QueuePlayStateEvent::Playing),

            (
                TrackState {
                    playing: PlayMode::Play,
                    ..
                },
                PlayStateChange::Pause,
            ) => current.pause().map(|_| QueuePlayStateEvent::Paused),

            (
                TrackState {
                    loops: LoopState::Finite(0),
                    ..
                },
                PlayStateChange::ToggleLoop,
            ) => current
                .enable_loop()
                .map(|_| QueuePlayStateEvent::StartedLooping),

            (
                TrackState {
                    loops: LoopState::Infinite | LoopState::Finite(_),
                    ..
                },
                PlayStateChange::ToggleLoop,
            ) => current
                .disable_loop()
                .map(|_| QueuePlayStateEvent::StoppedLooping),

            (
                TrackState {
                    playing: PlayMode::Stop | PlayMode::End,
                    ..
                },
                _,
            ) => {
                Self::report_error_msg(
                    "Attempted to change state of a stopped or ended track!",
                    sender,
                )
                .await;
                return Ok(());
            }

            _ => {
                Self::send_event(sender, QueuePlayStateEvent::StateAlreadySet).await;
                return Ok(());
            }
        };

        Self::send_event(sender, event?).await;

        Ok(())
    }

    async fn change_volume(
        &mut self,
        sender: &mpsc::Sender<QueueVolumeEvent>,
        new_volume: f32,
    ) -> Result<()> {
        if (new_volume - self.volume).abs() <= 0.01 {
            return Ok(());
        }

        self.volume = new_volume;

        trace!("Modifying queue.");

        if let Err(e) = self
            .buffer
            .modify_queue(|q| q.iter_mut().try_for_each(|t| t.set_volume(self.volume)))
        {
            trace!("Queue modified.");
            Self::report_error_msg(format!("Failed to set volume: {:?}", e), sender).await;
            return Ok(());
        }

        trace!("Queue modified.");
        Self::send_event(sender, QueueVolumeEvent::VolumeChanged(self.volume)).await;

        Ok(())
    }

    async fn now_playing(&self, sender: &mpsc::Sender<QueueNowPlayingEvent>) -> Result<()> {
        if let Some(current) = self.buffer.current() {
            let m = current.metadata().to_owned();

            Self::send_event(
                sender,
                QueueNowPlayingEvent::NowPlaying(Some(TrackMin {
                    index: 0,
                    title: m.title.unwrap_or_else(|| "Unknown Title".to_string()),
                    artist: m.artist.unwrap_or_else(|| "Unknown Artist".to_string()),
                    length: m.duration.unwrap_or_default(),
                    thumbnail: m.thumbnail,
                })),
            )
            .await;
        } else {
            Self::send_event(sender, QueueNowPlayingEvent::NowPlaying(None)).await;
        }

        Ok(())
    }

    async fn show_queue(&mut self, sender: &mpsc::Sender<QueueShowEvent>) -> Result<()> {
        let mut track_data: Vec<QueueItem<TrackMetaDataFull>> =
            Vec::with_capacity(self.buffer.len() + self.remainder.len());

        track_data.extend({
            let queue = self.buffer.current_queue();

            let track_extra_metadata = queue
                .iter()
                .map(|t| t.typemap().read())
                .collect::<FuturesOrdered<_>>()
                .map(|f| f.get::<TrackMetaData>().unwrap().to_owned())
                .collect::<Vec<_>>()
                .await;

            let track_metadata = queue.into_iter().map(|t| t.metadata().to_owned());

            track_extra_metadata
                .into_iter()
                .zip(track_metadata)
                .enumerate()
                .map(|(i, (extra, track))| {
                    let (name, colour) = self
                        .users
                        .get(&extra.added_by)
                        .map(|u| (u.name.clone(), u.colour))
                        .unwrap_or_else(|| ("Unknown".to_string(), Colour::from_rgb(0, 0, 0)));

                    QueueItem::<TrackMetaDataFull> {
                        index: i,
                        data: QueueItemData::BufferedTrack { metadata: track },
                        extra_metadata: TrackMetaDataFull {
                            added_at: extra.added_at,
                            colour,
                            added_by: extra.added_by,
                            added_by_name: name,
                        },
                    }
                })
        });

        let buffer_length = self.buffer.len();

        track_data.extend({
            let extractor = &self.extractor;

            futures::stream::iter(self.remainder.iter_mut())
                .for_each_concurrent(None, |t| async move {
                    debug!("Fetching metadata for {}", t.item);
                    t.fetch_metadata(extractor).await;
                })
                .await;

            self.remainder.iter().cloned().enumerate().map(|(i, t)| {
                let (name, colour) = self
                    .users
                    .get(&t.metadata.added_by)
                    .map(|u| (u.name.clone(), u.colour))
                    .unwrap_or_else(|| ("Unknown".to_string(), Colour::from_rgb(0, 0, 0)));

                QueueItem::<TrackMetaDataFull> {
                    index: buffer_length + i,
                    data: QueueItemData::UnbufferedTrack {
                        metadata: t.extracted_metadata,
                        url: t.item,
                    },
                    extra_metadata: TrackMetaDataFull {
                        added_at: t.metadata.added_at,
                        colour,
                        added_by: t.metadata.added_by,
                        added_by_name: name,
                    },
                }
            })
        });

        trace!(data_len = track_data.len(), "Extended data!");

        Self::send_event(sender, QueueShowEvent::CurrentQueue(track_data)).await;

        Ok(())
    }

    async fn track_ended(&mut self) -> Result<()> {
        if self.buffer.len() >= Self::MAX_QUEUE_LENGTH {
            return Ok(());
        }

        let item = match self.remainder.pop_front() {
            Some(t) => t,
            None => return Ok(()),
        };

        debug!(track = ?item, "Track ended!");
        self.buffer_item(item).await?;

        Ok(())
    }

    #[instrument(skip(self))]
    async fn buffer_item(&mut self, item: EnqueuedItem) -> Result<TrackMin> {
        trace!(?item, "Item to be buffered.");

        let EnqueuedItem { item, metadata, .. } = item;

        debug!(track = %item, "Starting track streaming.");

        let input = match Restartable::ytdl(item, true).await {
            Ok(i) => i,
            Err(e) => {
                return Err(Error::OperationFailed(format!(
                    "Downloading track failed! {:?}",
                    e
                )));
            }
        };

        debug!("Track streaming acquired.");

        let (track, handle) = create_player(input.into());

        if let Err(e) = handle.set_volume(self.volume) {
            let error = Err(Error::OperationFailed(format!(
                "Setting volume failed! {:?}",
                e
            )));

            if let Err(e) = handle.stop() {
                error!("Stopping track failed! {:?}", e);
            }

            return error;
        }

        /* handle.add_event(Event::Delayed(Duration::from_millis(20)), TrackStarted {
            channel: update_sender.clone(),
            event: QueueUpdate::TrackStarted,
        }) */

        handle.add_event(
            Event::Track(TrackEvent::End),
            UpdateBufferAfterSongEnded {
                channel: self.update_sender.clone(),
            },
        )?;

        trace!("Locking handle typemap.");
        handle
            .typemap()
            .write()
            .await
            .insert::<TrackMetaData>(metadata);
        trace!("Handle typemap finished.");

        trace!("Locking queue.");
        {
            let mut handle_lock = self.handler.lock().await;
            self.buffer.add(track, &mut handle_lock);

            // TODO: Might need to add a pause here if it doesn't do it automatically.
        }
        trace!("Queue unlocked.");

        let metadata = handle.metadata();

        Ok(TrackMin {
            index: self.buffer.len() - 1,
            title: metadata
                .title
                .clone()
                .unwrap_or_else(|| "Unknown Title".to_string()),
            artist: metadata
                .artist
                .clone()
                .unwrap_or_else(|| "Unknown Artist".to_string()),
            length: metadata.duration.unwrap_or_default(),
            thumbnail: metadata.thumbnail.clone(),
        })
    }

    async fn send_event<T: std::fmt::Debug>(sender: &mpsc::Sender<T>, event: T) {
        if sender.is_closed() {
            trace!(
                ?event,
                "Attempted to send event, but no listeners were found."
            );
            return;
        }

        if let Err(e) = sender.send(event).await {
            error!("{:?}", e);
        }
    }

    async fn report_error<E, T>(err: E, sender: &mpsc::Sender<T>)
    where
        E: Into<Error>,
        T: HasErrorVariant + std::fmt::Debug,
    {
        let err = err.into();
        error!("{:?}", err);
        Self::send_event(
            sender,
            T::new_error(QueueError::Other(format!("```\n{:?}\n```", err))),
        )
        .await;
    }

    async fn report_error_msg<S, T>(message: S, sender: &mpsc::Sender<T>)
    where
        S: AsRef<str> + std::fmt::Display,
        T: HasErrorVariant + std::fmt::Debug,
    {
        error!("{}", message);
        Self::send_event(
            sender,
            T::new_error(QueueError::Other(format!("```\n{}\n```", message))),
        )
        .await;
    }

    /* async fn is_user_in_voice_channel<T>(&self, user_id: UserId, sender: &mpsc::Sender<T>) -> bool
    where
        T: HasErrorVariant + std::fmt::Debug,
    {
        if !self.users.contains_key(&user_id) {
            Self::send_event(sender, T::new_error(QueueError::NotInVoiceChannel)).await;
            return false;
        }

        true
    } */
}
