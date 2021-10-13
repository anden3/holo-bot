use futures::stream::{FuturesOrdered, FuturesUnordered};
use itertools::Itertools;
use rand::{prelude::SliceRandom, thread_rng};
use songbird::{
    input::Restartable,
    tracks::{LoopState, PlayMode, TrackState},
    TrackEvent,
};

use super::{event_handlers::*, metadata::*, parameter_types::*, prelude::*, queue_events::*};
use crate::add_bindings;

#[derive(Debug, Clone)]
pub struct BufferedQueue {
    _inner: Arc<BufferedQueueInner>,
}

#[derive(Debug)]
pub struct BufferedQueueInner {
    update_sender: mpsc::Sender<QueueUpdate>,
    event_sender: broadcast::Sender<QueueEvent>,
    cancellation_token: CancellationToken,
}

impl BufferedQueue {
    pub fn new(manager: Arc<Songbird>, guild_id: &GuildId) -> Self {
        let (update_sender, update_receiver) = mpsc::channel(16);
        let (event_sender, _) = broadcast::channel(16);

        let guild_id = *guild_id;

        let cancellation_token = CancellationToken::new();
        let child_token = cancellation_token.child_token();

        let update_sender_clone = update_sender.clone();

        BufferedQueueHandler::start(
            manager,
            guild_id,
            update_receiver,
            update_sender_clone,
            cancellation_token,
        );

        Self {
            _inner: Arc::new(BufferedQueueInner {
                update_sender,
                event_sender,
                cancellation_token: child_token,
            }),
        }
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

impl Deref for BufferedQueue {
    type Target = BufferedQueueInner;

    fn deref(&self) -> &Self::Target {
        &self._inner
    }
}

impl Drop for BufferedQueue {
    fn drop(&mut self) {
        self.cancellation_token.cancel();
    }
}

struct BufferedQueueHandler {
    buffer: TrackQueue,
    remainder: VecDeque<EnqueuedItem>,
    manager: Arc<Songbird>,
    handler: Arc<Mutex<Call>>,
    update_sender: mpsc::Sender<QueueUpdate>,
    guild_id: GuildId,

    extractor: ytextract::Client,
    volume: f32,
}

impl BufferedQueueHandler {
    const MAX_QUEUE_LENGTH: usize = 10;
    const MAX_PLAYLIST_LENGTH: usize = 1000;

    pub fn start(
        manager: Arc<Songbird>,
        guild_id: GuildId,
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

        let handler = BufferedQueueHandler {
            buffer: TrackQueue::new(),
            remainder: VecDeque::with_capacity(32),
            manager,
            handler,
            update_sender,
            extractor: ytextract::Client::new(),
            volume: 0.5f32,
            guild_id,
        };

        tokio::spawn(async move {
            handler
                .handler_loop(update_receiver, cancellation_token)
                .await
        });
    }

    async fn handler_loop(
        mut self,
        mut update_receiver: mpsc::Receiver<QueueUpdate>,
        cancellation_token: CancellationToken,
    ) {
        while let Some(update) = tokio::select! {
           update = update_receiver.recv() => update,
           _ = cancellation_token.cancelled() => Some(QueueUpdate::Terminated),
        } {
            trace!(?update, "Received update");

            match update {
                QueueUpdate::Enqueued(sender, enqueued_type) => {
                    if let Err(e) = self.enqueue(&sender, enqueued_type).await {
                        Self::report_error(e, &sender).await;
                    }
                }

                QueueUpdate::EnqueueTop(sender, item) => {
                    if let Err(e) = self.enqueue_top(&sender, item).await {
                        Self::report_error(e, &sender).await;
                    }
                }

                QueueUpdate::PlayNow(sender, item) => {
                    if let Err(e) = self.play_now(&sender, item).await {
                        Self::report_error(e, &sender).await;
                    }
                }

                QueueUpdate::Skip(sender, amount) => {
                    if let Err(e) = self.skip(&sender, amount).await {
                        Self::report_error(e, &sender).await;
                    }
                }

                QueueUpdate::RemoveTracks(sender, condition) => {
                    if let Err(e) = self.remove_tracks(&sender, condition).await {
                        Self::report_error(e, &sender).await;
                    }
                }

                QueueUpdate::Shuffle(sender) => {
                    if let Err(e) = self.shuffle(&sender).await {
                        Self::report_error(e, &sender).await;
                    }
                }

                QueueUpdate::ChangePlayState(sender, state) => {
                    if let Err(e) = self.change_play_state(&sender, state).await {
                        Self::report_error(e, &sender).await;
                    }
                }

                QueueUpdate::ChangeVolume(sender, volume) => {
                    if let Err(e) = self.change_volume(&sender, volume).await {
                        Self::report_error(e, &sender).await;
                    }
                }

                QueueUpdate::NowPlaying(sender) => {
                    if let Err(e) = self.now_playing(&sender).await {
                        Self::report_error(e, &sender).await;
                    }
                }

                QueueUpdate::ShowQueue(sender) => {
                    if let Err(e) = self.show_queue(&sender).await {
                        Self::report_error(e, &sender).await;
                    }
                }

                QueueUpdate::TrackEnded => {
                    if let Err(e) = self.track_ended().await {
                        error!(err = ?e, "Track ended error!");
                    }
                }

                QueueUpdate::Terminated => {
                    self.buffer.stop();
                    self.remainder.clear();
                    break;
                }
            };
        }

        match self.manager.remove(self.guild_id).await.context(here!()) {
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
    ) -> anyhow::Result<()> {
        let to_be_enqueued = match enqueued_type {
            EnqueueType::Track(t) => vec![t],
            EnqueueType::Playlist(EnqueuedItem {
                item: playlist_id,
                metadata,
            }) => {
                let id = playlist_id.parse().context(here!())?;
                let playlist_data = self.extractor.playlist(id).await.context(here!())?;

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
                    let video = video.context(here!())?;

                    let videos_processed = videos_processed.fetch_add(1, Ordering::AcqRel) + 1;

                    to_be_enqueued.push(EnqueuedItem {
                        item: format!("https://youtu.be/{}", video.id()),
                        metadata: metadata.clone(),
                    });

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
                self.remainder.push_back(q);
                continue;
            }

            // Add to buffer.
            let track = self.buffer_item(q).await.context(here!())?;
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
    ) -> anyhow::Result<()> {
        let track = match self.buffer_item(item).await.context(here!())? {
            // No difference adding to top or bottom if there's only 1 or 2 elements in the queue.
            TrackMin { index: 0 | 1, .. } => return Ok(()),
            track => track,
        };

        trace!(pos = here!(), "Modifying queue.");

        self.buffer.modify_queue(|q| {
            let new_element = q.remove(track.index).unwrap();
            q.insert(1, new_element);
        });

        trace!(pos = here!(), "Queue modified.");

        debug!(?track, "Enqueued track to the top!");
        Self::send_event(sender, QueueEnqueueEvent::TrackEnqueuedTop(track)).await;

        Ok(())
    }

    async fn play_now(
        &mut self,
        sender: &mpsc::Sender<QueuePlayNowEvent>,
        item: EnqueuedItem,
    ) -> anyhow::Result<()> {
        self.buffer.pause().context(here!())?;

        let track = match self.buffer_item(item).await.context(here!())? {
            track @ TrackMin { index: 0, .. } => {
                self.buffer.resume().context(here!())?;
                Self::send_event(sender, QueuePlayNowEvent::Playing(track)).await;

                return Ok(());
            }
            track => track,
        };

        trace!(pos = here!(), "Modifying queue.");
        self.buffer.modify_queue(|q| {
            let new_element = q.remove(track.index).unwrap();
            q.insert(0, new_element);
        });
        trace!(pos = here!(), "Queue modified.");

        self.buffer.resume().context(here!())?;
        Self::send_event(sender, QueuePlayNowEvent::Playing(track)).await;

        Ok(())
    }

    async fn skip(
        &mut self,
        sender: &mpsc::Sender<QueueSkipEvent>,
        amount: usize,
    ) -> anyhow::Result<()> {
        let amount = std::cmp::min(amount, self.buffer.len());

        let skipped_tracks = (0..amount)
            .filter_map(|i| {
                let current = self.buffer.current().map(|t| t.metadata().to_owned());

                trace!(pos = here!(), track = ?current, "Skipping track.");

                if let Err(e) = self.buffer.skip() {
                    return Some(Err(e));
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
            .collect::<Result<Vec<_>, _>>()?
            .len();

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
    ) -> anyhow::Result<()> {
        if self.buffer.is_empty() {
            return Ok(());
        }

        let tracks_to_remove: HashSet<_> = match &condition {
            ProcessedQueueRemovalCondition::All => {
                let queue_len = self.buffer.len();
                self.buffer.stop();
                Self::send_event(sender, QueueRemovalEvent::QueueCleared { count: queue_len })
                    .await;
                return Ok(());
            }
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
                            .and_then(|d| (d.added_by != *user_id).then(|| t.uuid()))
                    })
                    .collect::<FuturesUnordered<_>>()
                    .filter_map(|f| async move { f })
                    .collect()
                    .await
            }
        };

        if tracks_to_remove.is_empty() {
            return Ok(());
        }

        trace!(tracks = ?tracks_to_remove, "Removing tracks...");

        trace!(pos = here!(), "Modifying queue.");
        self.buffer.modify_queue(|q| -> anyhow::Result<()> {
            q.iter_mut().try_for_each(|t| {
                (tracks_to_remove.contains(&t.uuid()))
                    .then(|| t.stop())
                    .unwrap_or(Ok(()))
            })?;

            q.retain(|t| !tracks_to_remove.contains(&t.uuid()));

            Ok(())
        })?;
        trace!(pos = here!(), "Queue modified.");

        let event = match condition {
            ProcessedQueueRemovalCondition::All => return Ok(()),
            ProcessedQueueRemovalCondition::Duplicates => QueueRemovalEvent::DuplicatesRemoved {
                count: tracks_to_remove.len(),
            },
            ProcessedQueueRemovalCondition::Indices(_) => QueueRemovalEvent::TracksRemoved {
                count: tracks_to_remove.len(),
            },
            ProcessedQueueRemovalCondition::FromUser(user_id) => QueueRemovalEvent::UserPurged {
                user_id,
                count: tracks_to_remove.len(),
            },
        };

        Self::send_event(sender, event).await;

        Ok(())
    }

    async fn shuffle(&mut self, sender: &mpsc::Sender<QueueShuffleEvent>) -> anyhow::Result<()> {
        if self.buffer.len() <= 2 {
            return Ok(());
        }

        trace!(pos = here!(), "Modifying queue.");
        self.buffer.modify_queue(|q| {
            let (_, slice) = q.make_contiguous().split_at_mut(1);
            slice.shuffle(&mut thread_rng());
        });
        trace!(pos = here!(), "Queue modified.");

        Self::send_event(sender, QueueShuffleEvent::QueueShuffled).await;

        Ok(())
    }

    async fn change_play_state(
        &mut self,
        sender: &mpsc::Sender<QueuePlayStateEvent>,
        state: PlayStateChange,
    ) -> anyhow::Result<()> {
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
            ) => current
                .play()
                .context(here!())
                .map(|_| QueuePlayStateEvent::Playing),

            (
                TrackState {
                    playing: PlayMode::Play,
                    ..
                },
                PlayStateChange::Pause,
            ) => current
                .pause()
                .context(here!())
                .map(|_| QueuePlayStateEvent::Paused),

            (
                TrackState {
                    loops: LoopState::Finite(0),
                    ..
                },
                PlayStateChange::ToggleLoop,
            ) => current
                .enable_loop()
                .context(here!())
                .map(|_| QueuePlayStateEvent::StartedLooping),

            (
                TrackState {
                    loops: LoopState::Infinite | LoopState::Finite(_),
                    ..
                },
                PlayStateChange::ToggleLoop,
            ) => current
                .disable_loop()
                .context(here!())
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
    ) -> anyhow::Result<()> {
        if (new_volume - self.volume).abs() <= 0.01 {
            return Ok(());
        }

        self.volume = new_volume;

        trace!(pos = here!(), "Modifying queue.");

        if let Err(e) = self
            .buffer
            .modify_queue(|q| q.iter_mut().try_for_each(|t| t.set_volume(self.volume)))
        {
            trace!(pos = here!(), "Queue modified.");
            Self::report_error_msg(format!("Failed to set volume: {:?}", e), sender).await;
            return Ok(());
        }

        trace!(pos = here!(), "Queue modified.");
        Self::send_event(sender, QueueVolumeEvent::VolumeChanged(self.volume)).await;

        Ok(())
    }

    async fn now_playing(&self, sender: &mpsc::Sender<QueueNowPlayingEvent>) -> anyhow::Result<()> {
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

    async fn show_queue(&self, sender: &mpsc::Sender<QueueShowEvent>) -> anyhow::Result<()> {
        let queue = self.buffer.current_queue();

        let track_extra_metadata = queue
            .iter()
            .map(|t| t.typemap().read())
            .collect::<FuturesOrdered<_>>()
            .map(|f| f.get::<TrackMetaData>().unwrap().to_owned())
            .collect::<Vec<_>>()
            .await;

        let track_metadata = queue.into_iter().map(|t| t.metadata().to_owned());

        let track_data = track_extra_metadata
            .into_iter()
            .zip(track_metadata)
            .enumerate()
            .map(|(i, (extra, track))| QueueItem {
                index: i,
                track_metadata: track,
                extra_metadata: extra,
            })
            .collect::<Vec<_>>();

        Self::send_event(sender, QueueShowEvent::CurrentQueue(track_data)).await;

        Ok(())
    }

    async fn track_ended(&mut self) -> anyhow::Result<()> {
        if self.buffer.len() >= Self::MAX_QUEUE_LENGTH {
            return Ok(());
        }

        let item = match self.remainder.pop_front() {
            Some(t) => t,
            None => return Ok(()),
        };

        debug!(track = ?item, "Track ended!");
        self.buffer_item(item).await.context(here!())?;

        Ok(())
    }

    #[instrument(skip(self))]
    async fn buffer_item(&mut self, item: EnqueuedItem) -> anyhow::Result<TrackMin> {
        trace!(?item, "Item to be buffered.");

        let EnqueuedItem { item, metadata } = item;

        debug!(track = %item, "Starting track streaming.");

        let input = match Restartable::ytdl(item, true).await.context(here!()) {
            Ok(i) => i,
            Err(e) => {
                return Err(anyhow!("Downloading track failed! {:?}", e));
            }
        };

        debug!("Track streaming acquired.");

        let (track, handle) = create_player(input.into());

        if let Err(e) = handle.set_volume(self.volume).context(here!()) {
            let error = Err(anyhow!("Setting volume failed! {:?}", e));

            return if let Err(e) = handle.stop().context(here!()) {
                error.context(format!("Stopping track failed! {:?}", e))
            } else {
                error
            };
        }

        /* handle.add_event(Event::Delayed(Duration::from_millis(20)), TrackStarted {
            channel: update_sender.clone(),
            event: QueueUpdate::TrackStarted,
        }) */

        handle
            .add_event(
                Event::Track(TrackEvent::End),
                UpdateBufferAfterSongEnded {
                    channel: self.update_sender.clone(),
                },
            )
            .context(here!())?;

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
        E: Into<anyhow::Error>,
        T: HasErrorVariant + std::fmt::Debug,
    {
        let err = err.into();
        error!("{:?}", err);
        Self::send_event(sender, T::new_error(format!("```\n{:?}\n```", err))).await;
    }

    async fn report_error_msg<S, T>(message: S, sender: &mpsc::Sender<T>)
    where
        S: AsRef<str> + std::fmt::Display,
        T: HasErrorVariant + std::fmt::Debug,
    {
        error!("{}", message);
        Self::send_event(sender, T::new_error(format!("```\n{}\n```", message))).await;
    }
}
