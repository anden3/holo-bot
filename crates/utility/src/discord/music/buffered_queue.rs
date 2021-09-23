use itertools::Itertools;
use songbird::tracks::LoopState;

use super::{metadata::TrackMetaData, parameter_types::*, prelude::*, queue_events::*};

macro_rules! add_bindings {
    ( $($i:ident: |$($a:ident: $t:ty),*| = $e:expr ),* ) => {
        $(
            #[instrument(skip(self))]
            pub async fn $i(&self, $($a: $t)*) -> anyhow::Result<()> {
                self.update_sender
                    .send($e)
                    .await
                    .map_err(|e| e.into())
            }
        )+
    }
}

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
        let event_sender_copy = event_sender.clone();

        let guild_id = *guild_id;

        let cancellation_token = CancellationToken::new();
        let child_token = cancellation_token.child_token();

        tokio::spawn(async move {
            Self::queue_handler(
                manager,
                guild_id,
                update_receiver,
                event_sender_copy,
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

    #[instrument(skip(manager, update_receiver))]
    async fn queue_handler(
        manager: Arc<Songbird>,
        guild_id: GuildId,
        mut update_receiver: mpsc::Receiver<QueueUpdate>,
        event_sender: broadcast::Sender<QueueEvent>,
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
                QueueUpdate::Enqueued(enqueued_type) => {
                    let to_be_enqueued = match enqueued_type {
                        EnqueueType::Track(t) => vec![t],
                        EnqueueType::Playlist(EnqueuedItem {
                            item: playlist_id,
                            metadata,
                        }) => {
                            let id = match playlist_id.parse().context(here!()) {
                                Ok(id) => id,
                                Err(e) => {
                                    Self::report_error(e, &event_sender);
                                    continue;
                                }
                            };

                            let playlist_data = match extractor.playlist(id).await.context(here!())
                            {
                                Ok(data) => data,
                                Err(e) => {
                                    Self::report_error(e, &event_sender);
                                    continue;
                                }
                            };

                            Self::send_event(
                                &event_sender,
                                QueueEvent::PlaylistProcessingStart {
                                    title: playlist_data.title().to_string(),
                                    description: playlist_data.description().to_string(),
                                    unlisted: playlist_data.unlisted(),
                                    views: playlist_data.views(),
                                    video_count: playlist_data.length(),
                                },
                            );

                            let videos_processed = AtomicU64::new(0);

                            let videos = playlist_data.videos().take(Self::MAX_PLAYLIST_LENGTH);
                            futures::pin_mut!(videos);

                            let mut to_be_enqueued = Vec::new();

                            while let Some(video) = videos.next().await {
                                let video = match video.context(here!()) {
                                    Ok(v) => v,
                                    Err(e) => {
                                        Self::report_error(e, &event_sender);
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
                                    &event_sender,
                                    QueueEvent::PlaylistProcessingProgress {
                                        index: videos_processed,
                                        title: video.title().to_string(),
                                        length: video.length(),
                                        thumbnail: video
                                            .thumbnails()
                                            .first()
                                            .map(|t| t.url.as_str().to_string()),
                                    },
                                );
                            }

                            Self::send_event(&event_sender, QueueEvent::PlaylistProcessingEnd);
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
                        if let Err(e) = Self::buffer_item(q, volume, &handler, &mut buffer)
                            .await
                            .context(here!())
                        {
                            Self::report_error(e, &event_sender);
                            continue 'event;
                        }
                    }
                }
                QueueUpdate::EnqueueTop(item) => {
                    let index = match Self::buffer_item(item, volume, &handler, &mut buffer)
                        .await
                        .context(here!())
                    {
                        // No difference adding to top or bottom if there's only 1 or 2 elements in the queue.
                        Ok(0 | 1) => continue,
                        Ok(i) => i,
                        Err(e) => {
                            Self::report_error(e, &event_sender);
                            continue;
                        }
                    };

                    buffer.modify_queue(|q| {
                        let new_element = q.remove(index).unwrap();
                        q.insert(1, new_element);
                    });
                }
                QueueUpdate::PlayNow(item) => {
                    if let Err(e) = buffer.pause().context(here!()) {
                        Self::report_error(e, &event_sender);
                        continue;
                    }

                    let index = match Self::buffer_item(item, volume, &handler, &mut buffer)
                        .await
                        .context(here!())
                    {
                        Ok(0) => continue,
                        Ok(i) => i,
                        Err(e) => {
                            Self::report_error(e, &event_sender);
                            continue;
                        }
                    };

                    buffer.modify_queue(|q| {
                        let new_element = q.remove(index).unwrap();
                        q.insert(0, new_element);
                    })
                }
                QueueUpdate::RemoveTracks(condition) => {
                    let indices_to_remove: Vec<_> = match condition {
                        QueueRemovalCondition::All => {
                            buffer.stop();
                            continue;
                        }
                        QueueRemovalCondition::Duplicates => {
                            let current_queue = buffer.current_queue();
                            current_queue
                                .iter()
                                .enumerate()
                                .duplicates_by(|(_, t)| t.uuid())
                                .map(|(i, _)| i)
                                .collect()
                        }
                        QueueRemovalCondition::Indices(indices) => {}
                        QueueRemovalCondition::FromUser(_) => todo!(),
                    };
                }
                QueueUpdate::ChangePlayState(state) => {
                    let current = match buffer.current() {
                        Some(c) => c,
                        None => continue,
                    };

                    let result = match state {
                        PlayStateChange::Resume => current.play().context(here!()),
                        PlayStateChange::Pause => current.pause().context(here!()),
                        PlayStateChange::ToggleLoop => {
                            let loop_state = match current.get_info().await.context(here!()) {
                                Ok(info) => info.loops,
                                Err(e) => {
                                    Self::report_error(e, &event_sender);
                                    continue;
                                }
                            };

                            match loop_state {
                                LoopState::Finite(0) => current.enable_loop().context(here!()),
                                LoopState::Infinite | LoopState::Finite(_) => {
                                    current.disable_loop().context(here!())
                                }
                            }
                        }
                    };

                    if let Err(e) = result {
                        Self::report_error(e, &event_sender);
                        continue;
                    }
                }
                QueueUpdate::ChangeVolume(new_volume) => {
                    if (new_volume - volume).abs() <= 0.01 {
                        continue;
                    }

                    volume = new_volume;

                    if let Err(e) =
                        buffer.modify_queue(|q| q.iter_mut().try_for_each(|t| t.set_volume(volume)))
                    {
                        Self::report_error_msg(
                            format!("Failed to set volume: {:?}", e),
                            &event_sender,
                        );
                    }
                }
                QueueUpdate::TrackEnded => {
                    if buffer.len() >= Self::MAX_QUEUE_LENGTH {
                        continue;
                    }

                    let item = match remainder.pop_front() {
                        Some(t) => t,
                        None => continue,
                    };

                    if let Err(e) = Self::buffer_item(item, volume, &handler, &mut buffer)
                        .await
                        .context(here!())
                    {
                        Self::report_error(e, &event_sender);
                        continue;
                    }
                }
                QueueUpdate::Terminated => {
                    buffer.stop();
                    remainder.clear();

                    Self::send_event(&event_sender, QueueEvent::Terminated);
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

    add_bindings! [
        enqueue: |enqueue_type: EnqueueType| = QueueUpdate::Enqueued(enqueue_type),
        enqueue_top: |track: EnqueuedItem| = QueueUpdate::EnqueueTop(track),
        play_now: |track: EnqueuedItem| = QueueUpdate::PlayNow(track),
        remove: |condition: QueueRemovalCondition| = QueueUpdate::RemoveTracks(condition),
        set_play_state: |state: PlayStateChange| = QueueUpdate::ChangePlayState(state),
        set_volume: |volume: f32| = QueueUpdate::ChangeVolume(volume)
    ];

    #[instrument(skip(handler, buffer))]
    async fn buffer_item(
        item: EnqueuedItem,
        volume: f32,
        handler: &Arc<Mutex<Call>>,
        buffer: &mut TrackQueue,
    ) -> anyhow::Result<usize> {
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
            .typemap()
            .write()
            .await
            .insert::<TrackMetaData>(metadata);

        {
            let mut handle_lock = handler.lock().await;
            buffer.add(track, &mut handle_lock);

            // TODO: Might need to add a pause here if it doesn't do it automatically.
        }

        Ok(buffer.len() - 1)
    }

    fn send_event(sender: &broadcast::Sender<QueueEvent>, event: QueueEvent) {
        if sender.receiver_count() == 0 {
            return;
        }

        if let Err(e) = sender.send(event) {
            error!("{:?}", e);
        }
    }

    fn report_error(err: anyhow::Error, sender: &broadcast::Sender<QueueEvent>) {
        error!("{:?}", err);
        Self::send_event(sender, QueueEvent::Error(format!("{:?}", err)));
    }

    fn report_error_msg<S: AsRef<str> + std::fmt::Display>(
        message: S,
        sender: &broadcast::Sender<QueueEvent>,
    ) {
        error!("{}", message);
        Self::send_event(sender, QueueEvent::Error(message.to_string()));
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
