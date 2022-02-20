use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use anyhow::Context;
use chrono::prelude::*;
use futures::{future, StreamExt, TryStreamExt};
use holodex::{
    model::{
        builders::VideoFilterBuilder,
        id::{ChannelId, VideoId},
        ChannelMin, Organisation, Video, VideoChannel, VideoFilter, VideoSortingCriteria,
        VideoStatus,
    },
    Client,
};
use tokio::{
    sync::{broadcast, mpsc, watch},
    time::{self, MissedTickBehavior},
};
use tokio_util::time::{delay_queue, DelayQueue};
use tracing::{debug, error, info, instrument, trace, warn};

use utility::{
    config::{Config, Database, DatabaseOperations, StreamTrackingConfig, Talent},
    discord::NotifiedStreamsCache,
    functions::try_run,
    here,
    streams::{Livestream, StreamUpdate},
};

use crate::discord_api::DiscordMessageData;

type StreamIndex = HashMap<VideoId, (Option<delay_queue::Key>, Livestream)>;

#[derive(Debug, Clone)]
pub(crate) enum VideoUpdate {
    Scheduled(VideoId),
    Started(VideoId),
    Ended(VideoId),
    Unscheduled(VideoId),
    Renamed {
        id: VideoId,
        new_name: String,
    },
    Rescheduled {
        id: VideoId,
        new_start: DateTime<Utc>,
    },
}

pub struct HoloApi;

impl HoloApi {
    const INITIAL_STREAM_FETCH_COUNT: u32 = 100;
    const UPDATE_INTERVAL: Duration = Duration::from_secs(60);

    #[instrument(skip(config, live_sender, stream_updates))]
    pub async fn start(
        config: Arc<Config>,
        live_sender: mpsc::Sender<DiscordMessageData>,
        stream_updates: broadcast::Sender<StreamUpdate>,
    ) -> watch::Receiver<HashMap<VideoId, Livestream>> {
        let (index_sender, index_receiver) = watch::channel(HashMap::new());

        tokio::spawn(async move {
            if let Err(e) = Self::stream_producer(
                &config.stream_tracking,
                &config.database,
                &config.talents,
                live_sender,
                index_sender,
                stream_updates,
            )
            .await
            {
                error!("{:?}", e);
            }
            info!(task = "Stream indexer", "Shutting down.");
        });

        index_receiver
    }

    #[instrument(skip(config, database, talents, live_sender, index_sender, stream_updates))]
    async fn stream_producer(
        config: &StreamTrackingConfig,
        database: &Database,
        talents: &[Talent],
        live_sender: mpsc::Sender<DiscordMessageData>,
        index_sender: watch::Sender<HashMap<VideoId, Livestream>>,
        stream_updates: broadcast::Sender<StreamUpdate>,
    ) -> anyhow::Result<()> {
        let client = Client::new(&config.holodex_token)?;

        let user_map = talents
            .iter()
            .filter_map(|u| u.youtube_ch_id.as_ref().map(|id| (id.clone(), u.clone())))
            .collect::<HashMap<_, _>>();

        let filter = VideoFilterBuilder::new()
            .status(&[VideoStatus::Upcoming])
            .sort_by(VideoSortingCriteria::PublishedAt)
            .build();

        let mut notified_streams = NotifiedStreamsCache::new(128);

        // See if there's any cached notified streams in the database, to prevent duplicate alerts.
        if let Ok(handle) = database.get_handle() {
            debug!("Fetching notified streams from database...");

            HashSet::<VideoId>::create_table(&handle)?;

            match HashSet::<VideoId>::load_from_database(&handle) {
                Ok(cached_data) => {
                    debug!(
                        "{} notified streams found in database cache!",
                        cached_data.len()
                    );

                    for stream_id in cached_data {
                        notified_streams.put(stream_id, ());
                    }
                }
                Err(e) => {
                    error!("Failed to load notified stream cache!\n{:#}", e);
                }
            }
        }

        let mut stream_index = HashMap::with_capacity(64);
        let mut stream_queue = DelayQueue::with_capacity(64);

        // Start by fetching the latest N streams.
        {
            let streams = client.videos(
                &VideoFilterBuilder::new()
                    .limit(Self::INITIAL_STREAM_FETCH_COUNT)
                    .status(&[
                        VideoStatus::New,
                        VideoStatus::Upcoming,
                        VideoStatus::Live,
                        VideoStatus::Past,
                    ])
                    .build(),
            )?;

            let streams = Self::process_streams(&streams, &user_map).await?;

            for (id, stream) in streams {
                if stream.state == VideoStatus::Upcoming {
                    let remind_in = match (stream.start_at - Utc::now()).to_std() {
                        Ok(duration) => duration,
                        Err(_) => {
                            let time_since_started = Utc::now() - stream.start_at;

                            if time_since_started > chrono::Duration::minutes(2) {
                                warn!(
                                    "Stream {} was supposed to start {:?} ago, but it's still marked as upcoming.",
                                    stream.title,
                                    time_since_started,
                                );
                                continue;
                            } else {
                                Duration::ZERO
                            }
                        }
                    };

                    let key = stream_queue.insert(id.clone(), remind_in);
                    stream_index.insert(id, (Some(key), stream));
                } else {
                    stream_index.insert(id, (None, stream));
                }
            }

            trace!("Starting stream index update!");
            let index = stream_index
                .clone()
                .into_iter()
                .map(|(id, (_, s))| (id, s))
                .collect();
            index_sender.send(index).context(here!())?;
            debug!(size = %stream_index.len(), "Stream index updated!");
        }

        let mut update_interval = time::interval(Self::UPDATE_INTERVAL);
        update_interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                live = stream_queue.next() => {
                    let live_id = match live {
                        Some(Ok(r)) => r.into_inner(),
                        Some(Err(e)) => {
                            error!("{:#}", e);
                            continue;
                        }
                        None => {
                            continue;
                        }
                    };

                    let (opt, stream) = match stream_index.get_mut(&live_id) {
                        Some(v) => v,
                        None => {
                            warn!("Stream {} not found in index!", live_id);
                            continue;
                        }
                    };

                    // Remove reference to queue key.
                    *opt = None;
                    stream.state = VideoStatus::Live;

                    if !notified_streams.contains(&live_id) {
                        notified_streams.put(live_id, ());

                        stream_updates
                            .send(StreamUpdate::Started((*stream).clone()))
                            .context(here!())?;

                        live_sender
                            .send(DiscordMessageData::ScheduledLive((*stream).clone()))
                            .await
                            .context(here!())?;
                    }

                }

                // Poll Holodex API
                _ = update_interval.tick() => {
                    let mut index_dirty = false;

                    // Fetch updates for the streams that are currently live or scheduled.
                    for update in Self::get_stream_updates(&client, &stream_index).await? {
                        index_dirty = true;
                        trace!(?update, "Stream update received!");

                        match update {
                            VideoUpdate::Scheduled(id) => {
                                if let Some((opt_key, entry)) = stream_index.get_mut(&id) {
                                    (*entry).state = VideoStatus::Upcoming;

                                    if let Some(key) = opt_key {
                                        warn!("Stream already in queue despite just being scheduled.");
                                        if let Some(start_at) = Self::get_duration_until_stream(entry) {
                                            stream_queue.reset(key, start_at);
                                        }
                                    }

                                    stream_updates.send(StreamUpdate::Scheduled(entry.clone())).context(here!())?;
                                } else {
                                    warn!(%id, "Entry not found in index!");
                                }
                            }
                            VideoUpdate::Started(id) => {
                                if let Some((_, entry)) = stream_index.get_mut(&id) {
                                    if entry.state != VideoStatus::Live {
                                        warn!("Stream didn't get set to live automatically, did notification not happen?");
                                        (*entry).state = VideoStatus::Live;
                                    }

                                    stream_updates.send(StreamUpdate::Started(entry.clone())).context(here!())?;
                                }
                            }
                            VideoUpdate::Ended(id) => {
                                if let Some((_, entry)) = stream_index.get_mut(&id) {
                                    (*entry).state = VideoStatus::Past;
                                    stream_updates.send(StreamUpdate::Ended(id)).context(here!())?;
                                }
                            }
                            VideoUpdate::Unscheduled(id) => {
                                if let Some((opt_key, entry)) = stream_index.remove(&id) {
                                    if let Some(key) = opt_key {
                                        info!(title = %entry.title, "Unscheduled video!");
                                        stream_queue.remove(&key);
                                    }

                                    stream_updates.send(StreamUpdate::Unscheduled(id)).context(here!())?;
                                }
                            }
                            VideoUpdate::Renamed { id, new_name } => {
                                if let Some((_, entry)) = stream_index.get_mut(&id) {
                                    info!(%new_name, "Renaming video!");
                                    (*entry).title = new_name.clone();
                                    stream_updates.send(StreamUpdate::Renamed(id, new_name)).context(here!())?;
                                } else {
                                    warn!(?id, name = ?new_name, "Entry not found in index!");
                                }
                            }
                            VideoUpdate::Rescheduled { id, new_start } => {
                                if let Some((opt_key, entry)) = stream_index.get_mut(&id) {
                                    (*entry).start_at = new_start;

                                    if let Some(key) = opt_key {
                                        if let Some(start_at) = Self::get_duration_until_stream(entry) {
                                            stream_queue.reset(key, start_at);
                                        }
                                    }

                                    stream_updates.send(StreamUpdate::Rescheduled(id, new_start)).context(here!())?;
                                } else {
                                    warn!(%id, "Entry not found in index!");
                                }
                            }
                        }
                    }

                    // Fetch streams until reaching indexed ones.
                    for (id, stream) in
                        Self::fetch_new_streams(&client, &filter, &stream_index, &user_map).await?
                    {
                        trace!(name = %stream.title, "New stream added to index!");
                        index_dirty = true;

                        stream_updates
                            .send(StreamUpdate::Scheduled(stream.clone()))
                            .context(here!())?;

                        let now = Utc::now();

                        match &stream.state {
                            VideoStatus::Upcoming if stream.start_at > now => {
                                // Unwrap is fine because we just checked that the start time is in the future.
                                let key = stream_queue.insert(id.clone(), (stream.start_at - now).to_std().unwrap());
                                stream_index.insert(id, (Some(key), stream));
                            }
                            VideoStatus::Upcoming => {
                                warn!(?stream, "Upcoming stream has a start time that has already passed!");
                                stream_index.insert(id, (None, stream));
                            }
                            _ => {
                                stream_index.insert(id, (None, stream));
                            }
                        }
                    }

                    if index_dirty {
                        trace!("Starting stream index update!");
                        let index = stream_index
                            .clone()
                            .into_iter()
                            .map(|(id, (_, s))| (id, s))
                            .collect();
                        index_sender.send(index).context(here!())?;
                        debug!(size = %stream_index.len(), "Stream index updated!");
                    }
                }

                res = tokio::signal::ctrl_c() => {
                    if let Err(e) = res {
                        error!("{:#}", e);
                    }

                    break;
                }
            }
        }

        // Save notified streams cache to database.
        if let Ok(handle) = database.get_handle() {
            let notified_set = notified_streams
                .into_iter()
                .map(|(id, _)| id.to_owned())
                .collect::<HashSet<_>>();

            if let Err(e) = notified_set.save_to_database(&handle) {
                error!("{:#}", e);
            }
        }

        Ok(())
    }

    #[instrument(skip(videos, users))]
    async fn process_streams(
        videos: &[Video],
        users: &HashMap<ChannelId, Talent>,
    ) -> anyhow::Result<HashMap<VideoId, Livestream>> {
        Ok(videos
            .iter()
            .filter_map(|v| {
                match &v.channel {
                    VideoChannel::Min(ChannelMin { org, .. })
                        if !matches!(*org, Some(Organisation::Hololive)) =>
                    {
                        return None
                    }
                    _ => (),
                }

                let streamer = users.get(v.channel.id())?.clone();

                let id = v.id.clone();
                let thumbnail = format!("https://i3.ytimg.com/vi/{}/maxresdefault.jpg", &v.id);
                let url = format!("https://youtube.com/watch?v={}", &v.id);

                Some((
                    v.id.clone(),
                    Livestream {
                        id,
                        title: v.title.clone(),
                        thumbnail,
                        created_at: v.available_at,
                        start_at: v.live_info.start_scheduled.unwrap_or(v.available_at),
                        duration: v
                            .duration
                            .and_then(|d| d.is_zero().then(|| None).unwrap_or(Some(d))),
                        streamer,
                        state: v.status,
                        url,
                    },
                ))
            })
            .collect::<HashMap<_, _>>())
    }

    fn get_duration_until_stream(stream: &Livestream) -> Option<std::time::Duration> {
        match (stream.start_at - Utc::now()).to_std() {
            Ok(duration) => Some(duration),
            Err(e) => {
                error!("{:#}", e);
                None
            }
        }
    }

    #[instrument(skip(client, stream_index))]
    async fn get_stream_updates(
        client: &Client,
        stream_index: &StreamIndex,
    ) -> anyhow::Result<Vec</* StreamUpdate */ VideoUpdate>> {
        let streams_to_update = {
            stream_index
                .iter()
                .filter_map(|(id, (_, stream))| {
                    matches!(
                        stream.state,
                        VideoStatus::New | VideoStatus::Upcoming | VideoStatus::Live
                    )
                    .then(|| id.clone())
                })
                .collect::<Vec<_>>()
        };

        debug!(count = streams_to_update.len(), "Streams to update!");

        if streams_to_update.is_empty() {
            return Ok(Vec::new());
        }

        try_run(|| async {
            Self::check_stream_updates(client, &streams_to_update, stream_index).await
        })
        .await
    }

    #[instrument(skip(client, streams, index))]
    async fn check_stream_updates(
        client: &Client,
        streams: &[VideoId],
        index: &StreamIndex,
    ) -> anyhow::Result<Vec<VideoUpdate>> {
        let filter = VideoFilterBuilder::new()
            .id(streams)
            .status(&[
                VideoStatus::Upcoming,
                VideoStatus::Live,
                VideoStatus::Past,
                VideoStatus::Missing,
                VideoStatus::New,
            ])
            .build();

        let streams = client.video_stream(&filter);
        futures::pin_mut!(streams);

        let mut updates = Vec::with_capacity(8);
        let now = Utc::now();

        while let Some(stream) = streams.try_next().await? {
            let (_, entry) = match index.get(&stream.id) {
                Some(l) => l,
                None => {
                    if (stream.available_at - now).num_hours() < 48 {
                        warn!(?stream, "Couldn't find stream in index.");
                    }

                    continue;
                }
            };

            if entry.title != stream.title && !stream.title.is_empty() {
                info!(before = %entry.title, after = %stream.title, "Video renamed!");
                updates.push(VideoUpdate::Renamed {
                    id: entry.id.clone(),
                    new_name: stream.title.clone(),
                });
            }

            if entry.state != VideoStatus::Past
                && entry.start_at
                    != stream
                        .live_info
                        .start_scheduled
                        .unwrap_or(stream.available_at)
            {
                info!(
                    before = ?entry.start_at,
                    after = ?stream.live_info.start_scheduled.unwrap_or(stream.available_at),
                    video = %stream.title,
                    "Video rescheduled!"
                );

                updates.push(VideoUpdate::Rescheduled {
                    id: entry.id.clone(),
                    new_start: stream
                        .live_info
                        .start_scheduled
                        .unwrap_or(stream.available_at),
                });
            }

            updates.push(match (entry.state, stream.status) {
                (VideoStatus::Missing | VideoStatus::New, VideoStatus::Upcoming) => {
                    debug!(video = %stream.title, "Video scheduled!");
                    VideoUpdate::Scheduled(entry.id.clone())
                }
                (VideoStatus::Upcoming | VideoStatus::Missing, VideoStatus::Live) => {
                    debug!(video = %stream.title, "Video started!");
                    VideoUpdate::Started(entry.id.clone())
                }
                (VideoStatus::Live, VideoStatus::Past | VideoStatus::Missing) => {
                    info!(video = %stream.title, "Video ended!");
                    VideoUpdate::Ended(entry.id.clone())
                }
                (VideoStatus::Upcoming, VideoStatus::Missing) => {
                    info!(video = %stream.title, "Video unscheduled!");
                    VideoUpdate::Unscheduled(entry.id.clone())
                }
                // Compensate for cache delay in Holodex.
                (VideoStatus::Live, VideoStatus::Upcoming)
                    if (now - stream.available_at) < chrono::Duration::minutes(5) =>
                {
                    continue
                }
                _ if entry.state != stream.status => {
                    warn!(before = ?entry.state, after = ?stream.status,
                        video = %stream.title, "Unknown status transition!");
                    continue;
                }
                _ => continue,
            });
        }

        Ok(updates)
    }

    #[instrument(skip(client, index, user_map))]
    async fn fetch_new_streams(
        client: &Client,
        filter: &VideoFilter,
        index: &StreamIndex,
        user_map: &HashMap<holodex::model::id::ChannelId, Talent>,
    ) -> anyhow::Result<HashMap<VideoId, Livestream>> {
        let new_streams: Vec<Video> = {
            client
                .video_stream(filter)
                .try_take_while(|v| {
                    future::ready(Ok({
                        if let Some((_, indexed_stream)) = index.get(&v.id) {
                            indexed_stream.state == VideoStatus::Missing
                        } else {
                            true
                        }
                    }))
                })
                .try_collect()
                .await?
        };

        Self::process_streams(&new_streams[..], user_map).await
    }
}
