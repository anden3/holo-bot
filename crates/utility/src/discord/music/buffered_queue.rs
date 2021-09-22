use super::{
    events::QueueUpdate,
    metadata::TrackMetaData,
    parameter_types::{EnqueueType, EnqueuedItem},
    prelude::*,
    QueueEvent,
};

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
    const MAX_TRACK_LENGTH: Duration = Duration::from_secs(30 * 60);

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
                            let id = match playlist_id.parse() {
                                Ok(id) => id,
                                Err(e) => {
                                    error!("{:?}", e);
                                    Self::send_event(
                                        &event_sender,
                                        QueueEvent::Error(format!("{:?}", e)),
                                    );
                                    continue;
                                }
                            };

                            let playlist_data = match extractor.playlist(id).await {
                                Ok(data) => data,
                                Err(e) => {
                                    error!("{:?}", e);
                                    Self::send_event(
                                        &event_sender,
                                        QueueEvent::Error(format!("{:?}", e)),
                                    );
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
                                let video = match video {
                                    Ok(v) => v,
                                    Err(e) => {
                                        error!("{:?}", e);
                                        Self::send_event(
                                            &event_sender,
                                            QueueEvent::Error(format!("{:?}", e)),
                                        );
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

                    for q in to_be_enqueued {
                        if buffer.len() >= Self::MAX_QUEUE_LENGTH {
                            // Add to remainder.
                            remainder.push_back(q);
                            continue;
                        }

                        // Add to buffer.
                        Self::buffer_item(q, volume, &handler, &mut buffer).await;
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

                    Self::buffer_item(item, volume, &handler, &mut buffer).await;
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

    #[instrument(skip(self))]
    pub async fn enqueue(&self, enqueue_type: EnqueueType) -> anyhow::Result<()> {
        self.update_sender
            .send(QueueUpdate::Enqueued(enqueue_type))
            .await
            .map_err(|e| e.into())
    }

    #[instrument(skip(handler, buffer))]
    async fn buffer_item(
        item: EnqueuedItem,
        volume: f32,
        handler: &Arc<Mutex<Call>>,
        buffer: &mut TrackQueue,
    ) {
        let EnqueuedItem { item, metadata } = item;

        let input = match input::ytdl(item).await.context(here!()) {
            Ok(i) => i,
            Err(e) => {
                error!("Downloading track failed! {:?}", e);
                return;
            }
        };

        let (track, handle) = create_player(input);

        if let Err(e) = handle.set_volume(volume).context(here!()) {
            error!("Setting volume failed! {:?}", e);

            if let Err(e) = handle.stop().context(here!()) {
                error!("Stopping track failed! {:?}", e);
            }
            return;
        }

        handle
            .typemap()
            .write()
            .await
            .insert::<TrackMetaData>(metadata);

        {
            let mut handle_lock = handler.lock().await;
            buffer.add(track, &mut handle_lock);
        }
    }

    fn send_event(sender: &broadcast::Sender<QueueEvent>, event: QueueEvent) {
        if sender.receiver_count() == 0 {
            return;
        }

        if let Err(e) = sender.send(event) {
            error!("{:?}", e);
        }
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
