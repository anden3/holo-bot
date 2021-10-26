use std::{collections::HashMap, sync::Arc, time::Duration};

use anyhow::{anyhow, Context};
use chrono::prelude::*;
use futures::{future, TryStreamExt};
use holo_bot_macros::clone_variables;
use holodex::{
    model::{
        builders::VideoFilterBuilder,
        id::{ChannelId, VideoId},
        ChannelMin, Organisation, Video, VideoChannel, VideoFilter, VideoStatus,
    },
    Client,
};
use tokio::{
    sync::{broadcast, mpsc, watch, Mutex},
    time::sleep,
};
use tracing::{debug, debug_span, error, info, instrument, trace, warn, Instrument};

use utility::{
    config::{Config, Database, LoadFromDatabase, SaveToDatabase, StreamTrackingConfig, Talent},
    discord::NotifiedStreamsCache,
    functions::try_run,
    here,
    streams::{Livestream, StreamUpdate},
};

use crate::{discord_api::DiscordMessageData, types::VideoUpdate};

type StreamIndex = Arc<Mutex<HashMap<VideoId, Livestream>>>;

pub struct HoloApi;

impl HoloApi {
    const INITIAL_STREAM_FETCH_COUNT: u32 = 100;
    const UPDATE_INTERVAL: Duration = Duration::from_secs(60);

    #[instrument(skip(config, live_sender, update_sender, exit_receiver))]
    pub async fn start(
        config: Arc<Config>,
        live_sender: mpsc::Sender<DiscordMessageData>,
        update_sender: broadcast::Sender<StreamUpdate>,
        mut exit_receiver: watch::Receiver<bool>,
    ) -> watch::Receiver<HashMap<VideoId, Livestream>> {
        let stream_index = Arc::new(Mutex::new(HashMap::new()));

        let (index_sender, index_receiver) = watch::channel(HashMap::new());

        tokio::spawn(
            clone_variables!(config, stream_index, update_sender, mut exit_receiver; {
                tokio::select! {
                    res = Self::stream_producer(&config.stream_tracking, &config.talents, stream_index, index_sender, update_sender) => {
                        if let Err(e) = res {
                            error!("{:?}", e);
                        }
                    }

                    res = exit_receiver.changed() => {
                        if let Err(e) = res {
                            error!("{:#}", e);
                        }
                    }
                }
                info!(task = "Stream indexer", "Shutting down.");
            })
            .instrument(debug_span!("Starting task.", task_type = "Stream indexer")),
        );

        tokio::spawn(
            clone_variables!(config; {
                let mut notified_streams = NotifiedStreamsCache::new(128);

                tokio::select! {
                    res = Self::stream_notifier(&config.database, stream_index, &mut notified_streams, live_sender, update_sender) => {
                        if let Err(e) = res {
                            error!("{:?}", e);
                        }
                    }

                    res = exit_receiver.changed() => {
                        if let Err(e) = res {
                            error!("{:#}", e);
                        }
                    }
                }

                info!(task = "Stream notifier", "Shutting down.");

                // Save notified streams cache to database.
                if let Ok(handle) = config.database.get_handle() {
                    if let Err(e) = notified_streams.save_to_database(&handle) {
                        error!("{:#}", e);
                    }
                }
            })
            .instrument(debug_span!("Starting task.", task_type = "Stream notifier")),
        );

        index_receiver
    }

    #[instrument(skip(config, talents, producer_lock, index_sender, stream_updates))]
    async fn stream_producer(
        config: &StreamTrackingConfig,
        talents: &[Talent],
        producer_lock: StreamIndex,
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
            .build();

        // Start by fetching the latest N streams.
        {
            let streams = client
                .videos(
                    &VideoFilterBuilder::new()
                        .limit(Self::INITIAL_STREAM_FETCH_COUNT)
                        .status(&[
                            VideoStatus::New,
                            VideoStatus::Upcoming,
                            VideoStatus::Live,
                            VideoStatus::Past,
                        ])
                        .build(),
                )
                .await?;

            let mut stream_index = producer_lock.lock().await;
            *stream_index = Self::process_streams(&streams, &user_map).await?;

            trace!("Starting stream index update!");
            index_sender.send(stream_index.clone()).context(here!())?;
            debug!(size = %stream_index.len(), "Stream index updated!");
        }

        loop {
            let mut index_dirty = false;

            // Fetch updates for the streams that are currently live or scheduled.
            {
                let updates = Self::get_stream_updates(&client, Arc::clone(&producer_lock)).await?;

                for update in updates {
                    index_dirty = true;

                    trace!(?update, "Stream update received!");

                    stream_updates.send(update).context(here!())?;
                }
            }

            // Fetch streams until reaching indexed ones.
            {
                for (id, stream) in
                    Self::fetch_new_streams(&client, &filter, Arc::clone(&producer_lock), &user_map)
                        .await?
                {
                    trace!(name = %stream.title, "New stream added to index!");
                    index_dirty = true;

                    stream_updates
                        .send(StreamUpdate::Scheduled(stream.clone()))
                        .context(here!())?;

                    let mut stream_index = producer_lock.lock().await;
                    stream_index.insert(id, stream);
                }
            }

            if index_dirty {
                let stream_index = producer_lock.lock().await;

                trace!("Starting stream index update!");
                index_sender.send(stream_index.clone()).context(here!())?;
                debug!(size = %stream_index.len(), "Stream index updated!");
            }

            sleep(Self::UPDATE_INTERVAL).await;
        }
    }

    #[instrument(skip(stream_index, notified_streams, discord_sender, live_sender))]
    async fn stream_notifier(
        database: &Database,
        stream_index: StreamIndex,
        notified_streams: &mut NotifiedStreamsCache,
        discord_sender: mpsc::Sender<DiscordMessageData>,
        live_sender: broadcast::Sender<StreamUpdate>,
    ) -> anyhow::Result<()> {
        // See if there's any cached notified streams in the database, to prevent duplicate alerts.
        if let Ok(handle) = database.get_handle() {
            debug!("Fetching notified streams from database...");

            match NotifiedStreamsCache::load_from_database(&handle) {
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

        let mut next_stream_start = Utc::now();

        loop {
            let mut sleep_duration = Self::UPDATE_INTERVAL;
            let mut index = stream_index.lock().await;

            let mut sorted_streams = index
                .iter()
                .filter(|(_, s)| {
                    !notified_streams.contains(&s.url)
                        && (s.state == VideoStatus::Upcoming
                            || (s.state == VideoStatus::Live
                                && ((Utc::now() - s.start_at) <= chrono::Duration::minutes(1))))
                })
                .collect::<Vec<_>>();

            if sorted_streams.is_empty() {
                std::mem::drop(index);
                sleep(sleep_duration).await;
                continue;
            }

            sorted_streams.sort_unstable_by_key(|(_, s)| s.start_at);

            let start_at = sorted_streams[0].1.start_at;
            let remaining_time = start_at - Utc::now();

            // Only write to log if the time for the next stream changes.
            if start_at != next_stream_start {
                next_stream_start = start_at;

                info!(
                    "Next streams are {}.",
                    chrono_humanize::HumanTime::from(remaining_time).to_text_en(
                        chrono_humanize::Accuracy::Precise,
                        chrono_humanize::Tense::Future
                    )
                );
            }

            if remaining_time.num_seconds() > 10 {
                std::mem::drop(index);

                let remaining_time_std = remaining_time.to_std().context(here!())?;

                if remaining_time_std <= sleep_duration {
                    sleep_duration = remaining_time_std;
                }

                sleep(sleep_duration).await;
                continue;
            }

            let next_streams = sorted_streams
                .into_iter()
                .take_while(|(_, s)| s.start_at == start_at)
                .collect::<Vec<_>>();

            info!(
                "{}",
                next_streams
                    .iter()
                    .fold("Time to watch:".to_owned(), |acc, (_, s)| {
                        acc + format!("\n{}", s.streamer.english_name).as_str()
                    })
            );

            for (_, stream) in &next_streams {
                assert!(notified_streams.put(stream.url.clone(), ()).is_none());
                trace!(?stream, "Stream going live!");

                live_sender
                    .send(StreamUpdate::Started((*stream).clone()))
                    .context(here!())?;

                discord_sender
                    .send(DiscordMessageData::ScheduledLive((*stream).clone()))
                    .await
                    .context(here!())?;
            }

            // Update the live status with a new write lock afterwards.
            let live_ids = next_streams
                .into_iter()
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>();

            for id in live_ids {
                if let Some(s) = index.get_mut(&id) {
                    s.state = VideoStatus::Live;
                }
            }

            std::mem::drop(index);

            sleep(sleep_duration).await;
        }
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

    #[instrument(skip(client, stream_index))]
    async fn get_stream_updates(
        client: &Client,
        stream_index: StreamIndex,
    ) -> anyhow::Result<Vec<StreamUpdate>> {
        let streams_to_update = {
            let index = stream_index.lock().await;

            index
                .iter()
                .filter_map(|(_, stream)| {
                    matches!(stream.state, VideoStatus::Upcoming | VideoStatus::Live)
                        .then(|| stream.id.clone())
                })
                .collect::<Vec<_>>()
        };

        debug!(count = streams_to_update.len(), "Streams to update!");

        if streams_to_update.is_empty() {
            return Ok(Vec::new());
        }

        let mut index = stream_index.lock().await;

        let updated_streams = try_run(|| async {
            Self::check_stream_updates(client, &streams_to_update, &*index).await
        })
        .await?;

        let mut updates = Vec::new();

        for update in updated_streams {
            match update {
                VideoUpdate::Scheduled(id) => {
                    let entry = index
                        .get_mut(&id)
                        .ok_or_else(|| anyhow!("Entry wasn't in index!"))?;
                    (*entry).state = VideoStatus::Upcoming;

                    updates.push(StreamUpdate::Scheduled(entry.clone()));
                }
                VideoUpdate::Started(id) => {
                    let entry = index
                        .get_mut(&id)
                        .ok_or_else(|| anyhow!("Entry wasn't in index!"))?;

                    (*entry).state = VideoStatus::Live;

                    updates.push(StreamUpdate::Started(entry.clone()));
                }
                VideoUpdate::Ended(id) => {
                    let entry = index
                        .get_mut(&id)
                        .ok_or_else(|| anyhow!("Entry wasn't in index!"))?;

                    (*entry).state = VideoStatus::Past;

                    updates.push(StreamUpdate::Ended(entry.clone()));
                }
                VideoUpdate::Unscheduled(id) => {
                    if let Some(entry) = index.remove(&id) {
                        updates.push(StreamUpdate::Unscheduled(entry));
                    }
                }
                VideoUpdate::Renamed { id, new_name } => {
                    let entry = index
                        .get_mut(&id)
                        .ok_or_else(|| anyhow!("Entry wasn't in index!"))?;

                    (*entry).title = new_name;
                }
                VideoUpdate::Rescheduled { id, new_start } => {
                    let entry = index
                        .get_mut(&id)
                        .ok_or_else(|| anyhow!("Entry wasn't in index!"))?;

                    (*entry).start_at = new_start;
                }
            }
        }

        Ok(updates)
    }

    #[instrument(skip(client, streams, index))]
    async fn check_stream_updates(
        client: &Client,
        streams: &[VideoId],
        index: &HashMap<VideoId, Livestream>,
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
            let entry = match index.get(&stream.id) {
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

            if entry.state == VideoStatus::Upcoming
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

    #[instrument(skip(client, index_lock, user_map))]
    async fn fetch_new_streams(
        client: &Client,
        filter: &VideoFilter,
        index_lock: StreamIndex,
        user_map: &HashMap<holodex::model::id::ChannelId, Talent>,
    ) -> anyhow::Result<HashMap<VideoId, Livestream>> {
        let new_streams: Vec<Video> = {
            let index = index_lock.lock().await;

            client
                .video_stream(filter)
                .try_take_while(|v| future::ready(Ok(!index.contains_key(&v.id))))
                .try_collect()
                .await?
        };

        Self::process_streams(&new_streams[..], user_map).await
    }
}
