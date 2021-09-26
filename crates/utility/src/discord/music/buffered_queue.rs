use futures::stream::{FuturesOrdered, FuturesUnordered};
use itertools::Itertools;
use rand::{prelude::SliceRandom, thread_rng};
use songbird::{
    tracks::{LoopState, PlayMode, TrackError, TrackState},
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
    const MAX_QUEUE_LENGTH: usize = 10;
    const MAX_PLAYLIST_LENGTH: usize = 100;
    // const MAX_TRACK_LENGTH: Duration = Duration::from_secs(30 * 60);

    pub fn new(manager: Arc<Songbird>, guild_id: &GuildId) -> Self {
        let (update_sender, update_receiver) = mpsc::channel(16);
        let (event_sender, _) = broadcast::channel(16);

        let guild_id = *guild_id;

        let cancellation_token = CancellationToken::new();
        let child_token = cancellation_token.child_token();

        let update_sender_clone = update_sender.clone();

        tokio::spawn(async move {
            Self::queue_handler(
                manager,
                guild_id,
                update_receiver,
                update_sender_clone,
                cancellation_token,
            )
            .await
        });

        Self {
            _inner: Arc::new(BufferedQueueInner {
                update_sender,
                event_sender,
                cancellation_token: child_token,
            }),
        }
    }

    #[instrument(skip(manager, update_receiver, update_sender, cancellation_token))]
    async fn queue_handler(
        manager: Arc<Songbird>,
        guild_id: GuildId,
        mut update_receiver: mpsc::Receiver<QueueUpdate>,
        update_sender: mpsc::Sender<QueueUpdate>,
        cancellation_token: CancellationToken,
    ) {
        let mut buffer = TrackQueue::new();
        let mut remainder = VecDeque::<EnqueuedItem>::with_capacity(32);

        let mut volume = 1.0f32;

        let extractor = ytextract::Client::new();

        let handler = match manager.get(guild_id) {
            Some(h) => h,
            None => {
                error!("Failed to get call when initializing queue!");
                return;
            }
        };

        'event: while let Some(update) = tokio::select! {
           update = update_receiver.recv() => update,
           _ = cancellation_token.cancelled() => Some(QueueUpdate::Terminated),
        } {
            match update {
                QueueUpdate::Enqueued(sender, enqueued_type) => {
                    let to_be_enqueued = match enqueued_type {
                        EnqueueType::Track(t) => vec![t],
                        EnqueueType::Playlist(EnqueuedItem {
                            item: playlist_id,
                            metadata,
                        }) => {
                            let id = match playlist_id.parse().context(here!()) {
                                Ok(id) => id,
                                Err(e) => {
                                    Self::report_error(e, &sender).await;
                                    continue;
                                }
                            };

                            let playlist_data = match extractor.playlist(id).await.context(here!())
                            {
                                Ok(data) => data,
                                Err(e) => {
                                    Self::report_error(e, &sender).await;
                                    continue;
                                }
                            };

                            Self::send_event(
                                &sender,
                                QueueEnqueueEvent::PlaylistProcessingStart(PlaylistMin {
                                    title: playlist_data.title().to_string(),
                                    description: playlist_data.description().to_string(),
                                    uploader: playlist_data
                                        .channel()
                                        .map(|c| c.name().to_owned())
                                        .unwrap_or_else(|| "Unknown Uploader".to_string()),
                                    unlisted: playlist_data.unlisted(),
                                    views: playlist_data.views(),
                                    video_count: playlist_data.length(),
                                }),
                            )
                            .await;

                            let videos_processed = AtomicU64::new(0);

                            let videos = playlist_data.videos().take(Self::MAX_PLAYLIST_LENGTH);
                            futures::pin_mut!(videos);

                            let mut to_be_enqueued = Vec::new();

                            while let Some(video) = videos.next().await {
                                let video = match video.context(here!()) {
                                    Ok(v) => v,
                                    Err(e) => {
                                        Self::report_error(e, &sender).await;
                                        continue 'event;
                                    }
                                };

                                let videos_processed =
                                    videos_processed.fetch_add(1, Ordering::AcqRel) + 1;

                                to_be_enqueued.push(EnqueuedItem {
                                    item: format!("https://youtu.be/{}", video.id()),
                                    metadata: metadata.clone(),
                                });

                                Self::send_event(
                                    &sender,
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

                            Self::send_event(&sender, QueueEnqueueEvent::PlaylistProcessingEnd)
                                .await;
                            to_be_enqueued
                        }
                    };

                    // TODO: Use drain filter so we can extend at the end.
                    for q in to_be_enqueued {
                        if buffer.len() >= Self::MAX_QUEUE_LENGTH {
                            // Add to remainder.
                            remainder.push_back(q);
                            continue;
                        }

                        // Add to buffer.
                        let track = match Self::buffer_item(
                            q,
                            volume,
                            &handler,
                            &mut buffer,
                            &update_sender,
                        )
                        .await
                        .context(here!())
                        {
                            Ok(track) => track,
                            Err(e) => {
                                Self::report_error(e, &sender).await;
                                continue 'event;
                            }
                        };

                        Self::send_event(&sender, QueueEnqueueEvent::TrackEnqueued(track)).await;
                    }
                }
                QueueUpdate::EnqueueTop(sender, item) => {
                    let track = match Self::buffer_item(
                        item,
                        volume,
                        &handler,
                        &mut buffer,
                        &update_sender,
                    )
                    .await
                    .context(here!())
                    {
                        // No difference adding to top or bottom if there's only 1 or 2 elements in the queue.
                        Ok(TrackMin { index: 0 | 1, .. }) => continue,
                        Ok(track) => track,
                        Err(e) => {
                            Self::report_error(e, &sender).await;
                            continue;
                        }
                    };

                    buffer.modify_queue(|q| {
                        let new_element = q.remove(track.index).unwrap();
                        q.insert(1, new_element);
                    });

                    Self::send_event(&sender, QueueEnqueueEvent::TrackEnqueuedTop(track)).await;
                }
                QueueUpdate::PlayNow(sender, item) => {
                    if let Err(e) = buffer.pause().context(here!()) {
                        Self::report_error(e, &sender).await;
                        continue;
                    }

                    let track = match Self::buffer_item(
                        item,
                        volume,
                        &handler,
                        &mut buffer,
                        &update_sender,
                    )
                    .await
                    .context(here!())
                    {
                        Ok(TrackMin { index: 0, .. }) => continue,
                        Ok(track) => track,
                        Err(e) => {
                            Self::report_error(e, &sender).await;
                            continue;
                        }
                    };

                    buffer.modify_queue(|q| {
                        let new_element = q.remove(track.index).unwrap();
                        q.insert(0, new_element);
                    });

                    Self::send_event(&sender, QueuePlayNowEvent::Playing(track)).await;
                }
                QueueUpdate::Skip(sender, amount) => {
                    let amount = std::cmp::min(amount, buffer.len());

                    let skipped_tracks = match (0..amount)
                        .filter_map(|i| {
                            let current = buffer.current().map(|t| t.metadata().to_owned());

                            if let Err(e) = buffer.skip() {
                                return Some(Err(e));
                            }

                            current.map(|m| {
                                Ok(TrackMin {
                                    index: i + 1,
                                    title: m.title.unwrap_or_else(|| "Unknown Title".to_string()),
                                    artist: m
                                        .artist
                                        .unwrap_or_else(|| "Unknown Artist".to_string()),
                                    length: m.duration.unwrap_or_default(),
                                    thumbnail: m.thumbnail,
                                })
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()
                    {
                        Ok(skipped_tracks) => skipped_tracks.len(),
                        Err(e) => {
                            Self::report_error(e, &sender).await;
                            continue;
                        }
                    };

                    Self::send_event(
                        &sender,
                        QueueSkipEvent::TracksSkipped {
                            count: skipped_tracks,
                        },
                    )
                    .await;
                }
                QueueUpdate::RemoveTracks(sender, condition) => {
                    if buffer.is_empty() {
                        continue;
                    }

                    let tracks_to_remove: HashSet<_> = match &condition {
                        ProcessedQueueRemovalCondition::All => {
                            let queue_len = buffer.len();
                            buffer.stop();
                            Self::send_event(
                                &sender,
                                QueueRemovalEvent::QueueCleared { count: queue_len },
                            )
                            .await;
                            continue;
                        }
                        ProcessedQueueRemovalCondition::Duplicates => buffer
                            .current_queue()
                            .iter()
                            .filter_map(|t| t.metadata().source_url.as_ref().map(|url| (t, url)))
                            .duplicates_by(|(_, url)| *url)
                            .map(|(t, _)| t.uuid())
                            .collect(),
                        ProcessedQueueRemovalCondition::Indices(indices) => indices
                            .iter()
                            .filter_map(|i| buffer.current_queue().get(*i).map(|t| t.uuid()))
                            .collect(),
                        ProcessedQueueRemovalCondition::FromUser(user_id) => {
                            buffer
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
                        continue;
                    }

                    let result: Result<(), TrackError> = buffer.modify_queue(|q| {
                        q.iter_mut().try_for_each(|t| {
                            (!tracks_to_remove.contains(&t.uuid()))
                                .then(|| t.stop())
                                .unwrap_or(Ok(()))
                        })?;

                        q.retain(|t| !tracks_to_remove.contains(&t.uuid()));

                        Ok(())
                    });

                    if let Err(e) = result {
                        Self::report_error(e, &sender).await;
                        continue;
                    }

                    let event = match condition {
                        ProcessedQueueRemovalCondition::All => continue,
                        ProcessedQueueRemovalCondition::Duplicates => {
                            QueueRemovalEvent::DuplicatesRemoved {
                                count: tracks_to_remove.len(),
                            }
                        }
                        ProcessedQueueRemovalCondition::Indices(_) => {
                            QueueRemovalEvent::TracksRemoved {
                                count: tracks_to_remove.len(),
                            }
                        }
                        ProcessedQueueRemovalCondition::FromUser(user_id) => {
                            QueueRemovalEvent::UserPurged {
                                user_id,
                                count: tracks_to_remove.len(),
                            }
                        }
                    };

                    Self::send_event(&sender, event).await;
                }
                QueueUpdate::Shuffle(sender) => {
                    if buffer.len() <= 2 {
                        continue;
                    }

                    buffer.modify_queue(|q| {
                        let (_, slice) = q.make_contiguous().split_at_mut(1);
                        slice.shuffle(&mut thread_rng());
                    });

                    Self::send_event(&sender, QueueShuffleEvent::QueueShuffled).await;
                }
                QueueUpdate::ChangePlayState(sender, state) => {
                    let current = match buffer.current() {
                        Some(c) => c,
                        None => continue,
                    };

                    let current_state = match current.get_info().await {
                        Ok(info) => info,
                        Err(e) => {
                            Self::report_error(e, &sender).await;
                            continue;
                        }
                    };

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
                                &sender,
                            )
                            .await;
                            continue;
                        }

                        _ => {
                            Self::send_event(&sender, QueuePlayStateEvent::StateAlreadySet).await;
                            continue;
                        }
                    };

                    match event {
                        Ok(evt) => Self::send_event(&sender, evt).await,
                        Err(e) => Self::report_error(e, &sender).await,
                    }
                }
                QueueUpdate::ChangeVolume(sender, new_volume) => {
                    if (new_volume - volume).abs() <= 0.01 {
                        continue;
                    }

                    volume = new_volume;

                    if let Err(e) =
                        buffer.modify_queue(|q| q.iter_mut().try_for_each(|t| t.set_volume(volume)))
                    {
                        Self::report_error_msg(format!("Failed to set volume: {:?}", e), &sender)
                            .await;
                        continue;
                    }

                    Self::send_event(&sender, QueueVolumeEvent::VolumeChanged(volume)).await;
                }
                QueueUpdate::ShowQueue(sender) => {
                    let queue = buffer.current_queue();

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

                    Self::send_event(&sender, QueueShowEvent::CurrentQueue(track_data)).await;
                }
                QueueUpdate::TrackEnded => {
                    if buffer.len() >= Self::MAX_QUEUE_LENGTH {
                        continue;
                    }

                    let item = match remainder.pop_front() {
                        Some(t) => t,
                        None => continue,
                    };

                    if let Err(e) =
                        Self::buffer_item(item, volume, &handler, &mut buffer, &update_sender)
                            .await
                            .context(here!())
                    {
                        error!("{:?}", e);
                        continue;
                    }
                }
                QueueUpdate::Terminated => {
                    buffer.stop();
                    remainder.clear();
                    break;
                }
            }
        }

        match manager.remove(guild_id).await.context(here!()) {
            Ok(()) => debug!("Left voice channel!"),
            Err(e) => {
                error!("{:?}", e);
            }
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
        show = QueueUpdate::ShowQueue => QueueShowEvent;
    }

    #[instrument(skip(handler, buffer))]
    async fn buffer_item(
        item: EnqueuedItem,
        volume: f32,
        handler: &Arc<Mutex<Call>>,
        buffer: &mut TrackQueue,
        update_sender: &mpsc::Sender<QueueUpdate>,
    ) -> anyhow::Result<TrackMin> {
        let EnqueuedItem { item, metadata } = item;

        let input = match input::ytdl(item).await.context(here!()) {
            Ok(i) => i,
            Err(e) => {
                return Err(anyhow!("Downloading track failed! {:?}", e));
            }
        };

        let (track, handle) = create_player(input);

        if let Err(e) = handle.set_volume(volume).context(here!()) {
            let error = Err(anyhow!("Setting volume failed! {:?}", e));

            return if let Err(e) = handle.stop().context(here!()) {
                error.context(format!("Stopping track failed! {:?}", e))
            } else {
                error
            };
        }

        handle
            .add_event(
                Event::Track(TrackEvent::End),
                UpdateBufferAfterSongEnded {
                    channel: update_sender.clone(),
                },
            )
            .context(here!())?;

        handle
            .typemap()
            .write()
            .await
            .insert::<TrackMetaData>(metadata);

        {
            let mut handle_lock = handler.lock().await;
            buffer.add(track, &mut handle_lock);

            // TODO: Might need to add a pause here if it doesn't do it automatically.
        }

        let metadata = handle.metadata();

        Ok(TrackMin {
            index: buffer.len() - 1,
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
