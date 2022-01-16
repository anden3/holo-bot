use std::sync::Arc;

use anyhow::anyhow;
use chrono::Utc;
use music_queue::{
    events::*, metadata::*, EnqueueType, EnqueuedItem, PlayStateChange,
    ProcessedQueueRemovalCondition, Queue, QueueItem, QueueItemData,
};
use poise::serenity_prelude::User;
use regex::Regex;
use serenity::{builder::CreateEmbed, model::id::UserId};

use super::prelude::*;

#[poise::command(
    slash_command,
    check = "can_play_music",
    required_permissions = "SPEAK"
)]
/// Play music from YouTube.
pub(crate) async fn music(_ctx: Context<'_>) -> anyhow::Result<()> {
    Ok(())
}

#[poise::command(
    prefix_command,
    slash_command,
    aliases("j"),
    check = "can_play_music",
    required_permissions = "SPEAK"
)]
/// Join your voice channel.
pub(crate) async fn join(ctx: Context<'_>) -> anyhow::Result<()> {
    let guild_id = ctx
        .guild_id()
        .ok_or_else(|| anyhow!("Guild ID is not available."))?;

    ctx.defer().await?;

    {
        let data = ctx.data();
        let read_lock = data.data.read().await;

        if read_lock.music_data.is_none() {
            ctx.say("Failed to get access to music data!").await?;
            return Err(anyhow!("Music data is not initialized."));
        }
    }

    let channel_id = ctx
        .discord()
        .cache
        .guild_field(guild_id, |g| {
            g.voice_states
                .get(&ctx.author().id)
                .and_then(|vs| vs.channel_id)
        })
        .flatten();

    let connect_to = match channel_id {
        Some(channel_id) => channel_id,
        None => {
            ctx.say("Could not find your voice channel, make sure you are in one peko.")
                .await?;

            return Ok(());
        }
    };

    let manager = songbird::get(ctx.discord())
        .await
        .ok_or_else(|| anyhow!("Songbird manager not available."))?;

    match manager.join(guild_id, connect_to).await {
        (_, Ok(())) => debug!("Joined voice channel!"),
        (_, Err(e)) => {
            ctx.say(&format!("Failed to join voice channel: {e:?}"))
                .await?;
            return Ok(());
        }
    }

    {
        let data = ctx.data();
        let mut write_lock = data.data.write().await;
        let music_data = write_lock.music_data.as_mut().unwrap();

        music_data.register_guild(
            Arc::clone(&manager),
            &guild_id,
            Arc::clone(&ctx.discord().http),
            Arc::clone(&ctx.discord().cache),
        );
    }

    Ok(())
}

#[poise::command(
    prefix_command,
    slash_command,
    aliases("l"),
    check = "can_play_music",
    required_permissions = "SPEAK"
)]
/// Leaves your voice channel.
pub(crate) async fn leave(ctx: Context<'_>) -> anyhow::Result<()> {
    let guild_id = ctx
        .guild_id()
        .ok_or_else(|| anyhow!("Guild ID is not available."))?;

    ctx.defer().await?;

    let manager = songbird::get(ctx.discord())
        .await
        .ok_or_else(|| anyhow!("Songbird manager not available."))?;

    if manager.get(guild_id).is_none() {
        ctx.say("I am not in a voice channel peko.").await?;
        return Ok(());
    }

    match manager.leave(guild_id).await {
        Ok(()) => debug!("Joined voice channel!"),
        Err(e) => {
            return notify_error(&ctx, format!("Failed to leave voice channel: {e:?}")).await;
        }
    }

    {
        let data = ctx.data();
        let mut write_lock = data.data.write().await;

        let music_data = match write_lock.music_data.as_mut() {
            Some(d) => d,
            None => {
                return notify_error(&ctx, "Music data is not initialized.").await;
            }
        };

        music_data.deregister_guild(&guild_id);
    }

    Ok(())
}

#[poise::command(
    prefix_command,
    slash_command,
    aliases("vol"),
    check = "can_play_music",
    required_permissions = "SPEAK",
    ephemeral
)]
/// Set the volume.
pub(crate) async fn volume(
    ctx: Context<'_>,
    #[description = "The volume you'd like."]
    #[min = 0]
    #[max = 100]
    volume: u32,
) -> anyhow::Result<()> {
    ctx.defer_ephemeral().await?;

    let queue = get_queue(&ctx).await?;

    let volume = (volume.clamp(0, 100) as f32) / 100.0;
    let mut collector = queue.set_volume(ctx.author().id, volume).await?;

    if let Some(evt) = collector.recv().await {
        match evt {
            QueueVolumeEvent::VolumeChanged(vol) => {
                ctx.say(format!("Volume set to {}!", (vol * 100.0) as i32))
                    .await?;
            }
            QueueVolumeEvent::Error(e) => {
                return notify_error(&ctx, format!("Failed to set volume: {e:?}")).await;
            }
        }
    }

    Ok(())
}

#[poise::command(
    prefix_command,
    slash_command,
    check = "can_play_music",
    required_permissions = "SPEAK"
)]
/// Play a song immediately.
pub(crate) async fn play_now(
    ctx: Context<'_>,
    #[description = "The song name or url you'd like to play."] song: String,
) -> anyhow::Result<()> {
    ctx.defer().await?;

    let queue = get_queue(&ctx).await?;

    let video_id_rgx = regex!(r"[0-9A-Za-z_-]{10}[048AEIMQUYcgkosw]");

    let url = video_id_rgx
        .find(&song)
        .map(|u| u.as_str().to_owned())
        .unwrap_or_else(|| format!("ytsearch1:{}", song.trim()));

    let mut collector = queue
        .play_now(
            ctx.author().id,
            EnqueuedItem {
                item: url,
                metadata: TrackMetaData {
                    added_by: ctx.author().id,
                    added_at: Utc::now(),
                },
                extracted_metadata: None,
            },
        )
        .await?;

    if let Some(evt) = collector.recv().await {
        match evt {
            QueuePlayNowEvent::Playing(track) => {
                let user = ctx.author().tag();

                ctx.send(|m| {
                    m.embed(|e| {
                        e.author(|a| a.name("Queue Update"))
                            .title("Track playing now!")
                            .fields([
                                ("Track", track.title, true),
                                ("Artist", track.artist, true),
                                (
                                    "Duration",
                                    format!(
                                        "{:02}:{:02}",
                                        track.length.as_secs() / 60,
                                        track.length.as_secs() % 60
                                    ),
                                    true,
                                ),
                            ])
                            .footer(|f| f.text(format!("Added by {}", user)))
                    })
                })
                .await?;
            }
            QueuePlayNowEvent::Error(e) => {
                return notify_error(&ctx, format!("Failed to play track: {e:?}")).await;
            }
        }
    }

    Ok(())
}

#[poise::command(
    prefix_command,
    slash_command,
    check = "can_play_music",
    required_permissions = "SPEAK",
    ephemeral
)]
/// Pauses the current song.
pub(crate) async fn pause(ctx: Context<'_>) -> anyhow::Result<()> {
    set_play_state(ctx, PlayStateChange::Pause).await
}

#[poise::command(
    prefix_command,
    slash_command,
    check = "can_play_music",
    required_permissions = "SPEAK",
    ephemeral
)]
/// Resumes the current song.
pub(crate) async fn resume(ctx: Context<'_>) -> anyhow::Result<()> {
    set_play_state(ctx, PlayStateChange::Resume).await
}

#[poise::command(
    prefix_command,
    slash_command,
    rename = "loop",
    check = "can_play_music",
    required_permissions = "SPEAK",
    ephemeral
)]
/// Toggle looping the current song.
pub(crate) async fn loop_song(ctx: Context<'_>) -> anyhow::Result<()> {
    set_play_state(ctx, PlayStateChange::ToggleLoop).await
}

#[poise::command(
    prefix_command,
    slash_command,
    reuse_response,
    aliases("s"),
    check = "can_play_music",
    required_permissions = "SPEAK"
)]
/// Skip current song.
pub(crate) async fn skip(
    ctx: Context<'_>,
    #[description = "How many songs to skip."]
    #[min = 1]
    amount: Option<u32>,
) -> anyhow::Result<()> {
    let amount = amount.unwrap_or(1);

    ctx.defer().await?;

    let queue = get_queue(&ctx).await?;

    let mut collector = queue.skip(ctx.author().id, amount as usize).await?;

    if let Some(evt) = collector.recv().await {
        match evt {
            QueueSkipEvent::TracksSkipped { count } => {
                ctx.say(format!(
                    "Skipped {count} {}!",
                    if count > 1 { "tracks" } else { "track" }
                ))
                .await?;
            }
            QueueSkipEvent::Error(e) => {
                return notify_error(&ctx, format!("Failed to skip track: {e:?}")).await;
            }
        };
    }

    Ok(())
}

#[poise::command(prefix_command, slash_command, check = "can_play_music", ephemeral)]
/// Get the currently playing song, if any.
pub(crate) async fn now_playing(ctx: Context<'_>) -> anyhow::Result<()> {
    ctx.defer_ephemeral().await?;

    let queue = get_queue(&ctx).await?;
    let mut collector = queue.now_playing(ctx.author().id).await?;

    if let Some(evt) = collector.recv().await {
        match evt {
            QueueNowPlayingEvent::NowPlaying(track) => {
                let track = match track {
                    Some(t) => t,
                    None => {
                        ctx.say("Nothing is playing right now!").await?;
                        return Ok(());
                    }
                };

                ctx.send(|m| {
                    m.embed(|e| {
                        e.title("Now playing!").fields([
                            ("Track", track.title, true),
                            ("Artist", track.artist, true),
                            (
                                "Duration",
                                format!(
                                    "{:02}:{:02}",
                                    track.length.as_secs() / 60,
                                    track.length.as_secs() % 60
                                ),
                                true,
                            ),
                        ])
                    })
                })
                .await?;
            }
            QueueNowPlayingEvent::Error(e) => {
                return notify_error(
                    &ctx,
                    format!("Failed to get currently playing track: {e:?}"),
                )
                .await;
            }
        }
    }

    Ok(())
}

#[poise::command(
    prefix_command,
    slash_command,
    aliases("q"),
    check = "can_play_music",
    required_permissions = "SEND_MESSAGES",
    ephemeral
)]
/// Show the current queue.
pub(crate) async fn queue(ctx: Context<'_>) -> anyhow::Result<()> {
    ctx.defer_ephemeral().await?;

    let queue = get_queue(&ctx).await?;
    let mut collector = queue.show(ctx.author().id).await?;

    let queue_data = match collector.recv().await {
        Some(QueueShowEvent::CurrentQueue(queue)) => queue,
        Some(QueueShowEvent::Error(e)) => {
            return notify_error(&ctx, format!("Failed to get queue: {e:?}")).await;
        }
        None => {
            ctx.say("Queue is empty!").await?;
            return Ok(());
        }
    };

    PaginatedList::new()
        .title("Queue")
        .data(&queue_data)
        .embed(Box::new(
            |QueueItem::<TrackMetaDataFull> {
                 index,
                 data,
                 extra_metadata,
             },
             _| {
                let mut embed = CreateEmbed::default();

                embed.field("Pos", format!("#{}", index + 1), true);

                match data {
                    QueueItemData::BufferedTrack { metadata } => {
                        if let Some(thumbnail) = &metadata.thumbnail {
                            embed.thumbnail(&thumbnail);
                        }

                        if let Some(title) = &metadata.title {
                            embed.field("Track", title, true);
                        }

                        if let Some(artist) = &metadata.artist {
                            embed.field("Artist", artist, true);
                        }

                        if let Some(duration) = &metadata.duration {
                            embed.field(
                                "Duration",
                                format!(
                                    "{:02}:{:02}",
                                    duration.as_secs() / 60,
                                    duration.as_secs() % 60
                                ),
                                true,
                            );
                        }
                    }

                    QueueItemData::UnbufferedTrack { url, metadata } => {
                        if let Some(metadata) = metadata {
                            if let Some(thumbnail) = &metadata.thumbnail {
                                embed.thumbnail(&thumbnail);
                            }

                            embed.field("Track", &metadata.title, true);
                            embed.field("Uploader", &metadata.uploader, true);
                            embed.field(
                                "Duration",
                                format!(
                                    "{:02}:{:02}",
                                    metadata.duration.as_secs() / 60,
                                    metadata.duration.as_secs() % 60
                                ),
                                true,
                            );
                        } else {
                            embed.field("Track", &url, true);
                        }
                    }

                    QueueItemData::UnbufferedSearch { query } => {
                        embed.field("Query", &query, true);
                    }
                }

                embed.colour(extra_metadata.colour);
                embed.footer(|f| f.text(format!("Added by: {}", extra_metadata.added_by_name)));
                embed.timestamp(&extra_metadata.added_at);

                embed
            },
        ))
        .display(ctx)
        .await?;

    Ok(())
}

#[poise::command(
    prefix_command,
    slash_command,
    rename = "add",
    aliases("p"),
    check = "can_play_music",
    required_permissions = "SPEAK"
)]
/// Add a song to the queue.
pub(crate) async fn add_song(
    ctx: Context<'_>,
    #[description = "The song name or url you'd like to enqueue."] song: String,
) -> anyhow::Result<()> {
    add(ctx, song, false).await
}

#[poise::command(
    prefix_command,
    slash_command,
    rename = "top",
    aliases("t"),
    check = "can_play_music",
    required_permissions = "SPEAK"
)]
/// Add a song to the top of the queue.
pub(crate) async fn add_to_top(
    ctx: Context<'_>,
    #[description = "The song name or url you'd like to enqueue."] song: String,
) -> anyhow::Result<()> {
    add(ctx, song, true).await
}

#[poise::command(
    prefix_command,
    slash_command,
    reuse_response,
    aliases("pl"),
    check = "can_play_music",
    required_permissions = "SPEAK"
)]
/// Add all the songs on a playlist to the queue.
pub(crate) async fn add_playlist(
    ctx: Context<'_>,
    #[description = "The playlist url."] playlist: String,
) -> anyhow::Result<()> {
    ctx.defer().await?;

    let queue = get_queue(&ctx).await?;

    // Thanks youtube-dl for being open-source <3
    let playlist_rgx: &'static Regex =
        regex!(r"(?:(?:PL|LL|EC|UU|FL|RD|UL|TL|PU|OLAK5uy_)[0-9A-Za-z-_]{10,}|RDMM)");

    let playlist_id = match playlist_rgx.find(&playlist) {
        Some(m) => &playlist[m.start()..m.end()],
        None => {
            ctx.say("URL does not contain a playlist ID.").await?;
            return Ok(());
        }
    };

    let mut collector = queue
        .enqueue(
            ctx.author().id,
            EnqueueType::Playlist(EnqueuedItem {
                item: playlist_id.to_string(),
                metadata: TrackMetaData {
                    added_by: ctx.author().id,
                    added_at: Utc::now(),
                },
                extracted_metadata: None,
            }),
        )
        .await?;

    let mut playlist_info = None;
    let mut playlist_length = 0;

    while let Some(evt) = collector.recv().await {
        match evt {
            QueueEnqueueEvent::TrackEnqueued(_track, _remaining_time) => {}

            QueueEnqueueEvent::PlaylistProcessingStart(playlist) => {
                playlist_info = Some(playlist.clone());
                playlist_length = playlist.video_count;

                let mut embed = CreateEmbed::default();
                embed
                    .title("Playlist loading...")
                    .footer(|f| f.text(format!("Added by {}", ctx.author().tag())))
                    .field("Name", playlist.title, true)
                    .field("Size", format!("{} tracks", playlist.video_count), true);

                if let Some(desc) = playlist.description {
                    embed.field("Description", desc, true);
                }

                embed.field("Uploader", playlist.uploader, true);

                ctx.send(|m| {
                    m.embeds = vec![embed];
                    m
                })
                .await?;
            }

            QueueEnqueueEvent::PlaylistProcessingProgress(track) => {
                let mut embed = CreateEmbed::default();
                embed
                    .title("Playlist loading...")
                    .footer(|f| {
                        f.text(format!(
                            "Loaded {} out of {}.",
                            track.index + 1,
                            playlist_length
                        ))
                    })
                    .fields([
                        ("Track", track.title, true),
                        ("Artist", track.artist, true),
                        (
                            "Duration",
                            format!(
                                "{:02}:{:02}",
                                track.length.as_secs() / 60,
                                track.length.as_secs() % 60
                            ),
                            true,
                        ),
                    ]);

                if let Some(thumbnail) = track.thumbnail {
                    embed.thumbnail(thumbnail);
                }

                ctx.send(|m| {
                    m.embeds = vec![embed];
                    m
                })
                .await?;
            }

            QueueEnqueueEvent::PlaylistProcessingEnd => {
                let playlist = playlist_info.take().unwrap();

                let mut embed = CreateEmbed::default();
                embed
                    .title("Playlist added!.")
                    .footer(|f| f.text(format!("Added by {}", ctx.author().tag())))
                    .field("Name", playlist.title, true)
                    .field("Size", format!("{} tracks", playlist.video_count), true);

                if let Some(desc) = playlist.description {
                    embed.field("Description", desc, true);
                }

                embed.field("Uploader", playlist.uploader, true);

                ctx.send(|m| {
                    m.embeds = vec![embed];
                    m
                })
                .await?;
            }

            QueueEnqueueEvent::Error(e) => {
                return notify_error(&ctx, format!("Failed to enqueue playlist: {e:?}")).await;
            }

            _ => continue,
        }
    }

    Ok(())
}

#[poise::command(
    prefix_command,
    slash_command,
    aliases("r"),
    check = "can_play_music",
    required_permissions = "SPEAK"
)]
/// Remove songs from the queue.
pub(crate) async fn remove(
    ctx: Context<'_>,
    #[description = "A position or list of positions, separated by spaces."] positions: String,
) -> anyhow::Result<()> {
    remove_from_queue(ctx, QueueRemovalCondition::Indices(positions)).await
}

#[poise::command(
    prefix_command,
    slash_command,
    aliases("rd"),
    check = "can_play_music",
    required_permissions = "SPEAK"
)]
/// Remove duplicate songs from the queue.
pub(crate) async fn remove_dupes(ctx: Context<'_>) -> anyhow::Result<()> {
    remove_from_queue(ctx, QueueRemovalCondition::Duplicates).await
}

#[poise::command(
    prefix_command,
    slash_command,
    check = "can_play_music",
    required_permissions = "SPEAK"
)]
/// Clear the queue.
pub(crate) async fn clear(
    ctx: Context<'_>,
    #[description = "Specify a user to remove all songs enqueued by them."] user: Option<User>,
) -> anyhow::Result<()> {
    match user {
        Some(user) => remove_from_queue(ctx, QueueRemovalCondition::FromUser(user.id)).await,
        None => remove_from_queue(ctx, QueueRemovalCondition::All).await,
    }
}

#[poise::command(
    prefix_command,
    slash_command,
    check = "can_play_music",
    required_permissions = "SPEAK"
)]
/// Shuffle the queue.
pub(crate) async fn shuffle(ctx: Context<'_>) -> anyhow::Result<()> {
    ctx.defer().await?;

    let queue = get_queue(&ctx).await?;
    let mut collector = queue.shuffle(ctx.author().id).await?;

    match collector.recv().await {
        Some(QueueShuffleEvent::QueueShuffled) => {
            ctx.say("Queue shuffled!").await?;
        }
        Some(QueueShuffleEvent::Error(e)) => {
            return notify_error(&ctx, format!("Failed to shuffle queue: {e:?}")).await;
        }
        None => (),
    }

    Ok(())
}

async fn get_queue(ctx: &Context<'_>) -> anyhow::Result<Queue> {
    let guild_id = ctx
        .guild_id()
        .ok_or_else(|| anyhow!("Guild ID is not available."))?;

    let data = ctx.data();
    let read_lock = data.data.read().await;

    let queue = match read_lock.music_data.as_ref() {
        Some(d) => d.get_queue(&guild_id),
        None => {
            return notify_error(ctx, "Failed to get access to music data!").await;
        }
    };

    drop(read_lock);

    match queue {
        Some(q) => Ok(q),
        None => {
            ctx.say("I'm not in a voice channel, peko.").await?;
            Err(anyhow!("Failed to get queue."))
        }
    }
}

async fn set_play_state(ctx: Context<'_>, state: PlayStateChange) -> anyhow::Result<()> {
    ctx.defer_ephemeral().await?;

    let queue = get_queue(&ctx).await?;

    let mut collector = queue.set_play_state(ctx.author().id, state).await?;

    if let Some(evt) = collector.recv().await {
        match evt {
            QueuePlayStateEvent::Playing => {
                ctx.say("Resuming song!").await?;
            }
            QueuePlayStateEvent::Paused => {
                ctx.say("Pausing song!").await?;
            }
            QueuePlayStateEvent::StartedLooping => {
                ctx.say("Looping song!").await?;
            }
            QueuePlayStateEvent::StoppedLooping => {
                ctx.say("No longer looping song!").await?;
            }
            QueuePlayStateEvent::StateAlreadySet => {
                ctx.say("State already set!").await?;
            }
            QueuePlayStateEvent::Error(e) => {
                return notify_error(&ctx, format!("Failed to set state: {e:?}")).await;
            }
        }
    }

    Ok(())
}

async fn add(ctx: Context<'_>, song: String, add_to_top: bool) -> anyhow::Result<()> {
    ctx.defer().await?;

    let queue = get_queue(&ctx).await?;
    let video_id_rgx = regex!(r"[0-9A-Za-z_-]{10}[048AEIMQUYcgkosw]");

    let url = video_id_rgx
        .find(&song)
        .map(|u| u.as_str().to_owned())
        .unwrap_or_else(|| format!("ytsearch1:{}", song.trim()));

    let enqueued_item = EnqueuedItem {
        item: url,
        metadata: TrackMetaData {
            added_by: ctx.author().id,
            added_at: Utc::now(),
        },
        extracted_metadata: None,
    };

    let mut collector = if add_to_top {
        queue.enqueue_top(ctx.author().id, enqueued_item).await?
    } else {
        queue
            .enqueue(ctx.author().id, EnqueueType::Track(enqueued_item))
            .await?
    };

    if let Some(evt) = collector.recv().await {
        match evt {
            QueueEnqueueEvent::TrackEnqueued(track, remaining_time) => {
                let user = ctx.author().tag();

                ctx.send(|m| {
                    m.embed(|e| {
                        e.author(|a| a.name("Queue Update"))
                            .title("Track added to queue!")
                            .fields([
                                ("Pos", format!("#{}", track.index + 1), true),
                                ("Track", track.title, true),
                                ("Artist", track.artist, true),
                                (
                                    "Duration",
                                    format!(
                                        "{:02}:{:02}",
                                        track.length.as_secs() / 60,
                                        track.length.as_secs() % 60
                                    ),
                                    true,
                                ),
                            ])
                            .footer(|f| f.text(format!("Added by {}", user)));

                        if remaining_time > std::time::Duration::ZERO {
                            let formatted_time = if remaining_time.as_secs() > 3600 {
                                format!(
                                    "{:02}:{:02}:{:02}",
                                    remaining_time.as_secs() / 3600,
                                    (remaining_time.as_secs() % 3600) / 60,
                                    remaining_time.as_secs() % 60
                                )
                            } else {
                                format!(
                                    "{:02}:{:02}",
                                    remaining_time.as_secs() / 60,
                                    remaining_time.as_secs() % 60
                                )
                            };

                            e.field("Remaining (approx)", formatted_time, true);
                        }

                        if let Some(thumbnail) = track.thumbnail {
                            e.thumbnail(thumbnail);
                        }

                        e
                    })
                })
                .await?;
            }

            QueueEnqueueEvent::TrackEnqueuedTop(track) => {
                let user = ctx.author().tag();

                ctx.send(|m| {
                    m.embed(|e| {
                        e.author(|a| a.name("Queue Update"))
                            .title("Track added to top of queue!")
                            .fields([
                                ("Track", track.title, true),
                                ("Artist", track.artist, true),
                                (
                                    "Duration",
                                    format!(
                                        "{:02}:{:02}",
                                        track.length.as_secs() / 60,
                                        track.length.as_secs() % 60
                                    ),
                                    true,
                                ),
                            ])
                            .footer(|f| f.text(format!("Added by {}", user)));

                        if let Some(thumbnail) = track.thumbnail {
                            e.thumbnail(thumbnail);
                        }

                        e
                    })
                })
                .await?;
            }

            QueueEnqueueEvent::TrackEnqueuedBacklog(track) => {
                let user = ctx.author().tag();

                ctx.send(|m| {
                    m.embed(|e| {
                        e.author(|a| a.name("Queue Update"))
                            .title("Item added to queue!")
                            .fields([("Item", track, true)])
                            .footer(|f| f.text(format!("Added by {}", user)));

                        e
                    })
                })
                .await?;
            }

            QueueEnqueueEvent::Error(e) => {
                return notify_error(&ctx, format!("Enqueue Error: {e:?}")).await;
            }

            _ => {
                return notify_error(
                    &ctx,
                    "I somehow received a playlist event despite queueing a song peko? pardun!?",
                )
                .await;
            }
        };
    }

    Ok(())
}

async fn remove_from_queue(
    ctx: Context<'_>,
    removal_condition: QueueRemovalCondition,
) -> anyhow::Result<()> {
    ctx.defer().await?;

    let user_id = ctx.author().id;
    let queue = get_queue(&ctx).await?;

    let removal_condition = match removal_condition {
        QueueRemovalCondition::All => ProcessedQueueRemovalCondition::All,
        QueueRemovalCondition::Duplicates => ProcessedQueueRemovalCondition::Duplicates,
        QueueRemovalCondition::FromUser(uid) => ProcessedQueueRemovalCondition::FromUser(uid),
        QueueRemovalCondition::Indices(indices) => {
            let indices = indices
                .split(' ')
                .map(|n| n.parse::<usize>().context(here!()))
                .collect::<anyhow::Result<Vec<_>, _>>();

            match indices {
                Ok(idx) => ProcessedQueueRemovalCondition::Indices(idx),
                Err(e) => {
                    return notify_error(&ctx, format!("Invalid indices: {e:?}")).await;
                }
            }
        }
    };

    let mut collector = queue.remove(user_id, removal_condition).await?;

    if let Some(evt) = collector.recv().await {
        match evt {
            QueueRemovalEvent::TracksRemoved { count } => {
                ctx.say(format!("Removed {count} songs!")).await?;
            }
            QueueRemovalEvent::DuplicatesRemoved { count } => {
                ctx.say(format!("Removed {count} duplicates!")).await?;
            }
            QueueRemovalEvent::UserPurged { user_id: _, count } => {
                ctx.say(format!(
                    "Removed {count} songs from {}!",
                    ctx.author().tag()
                ))
                .await?;
            }
            QueueRemovalEvent::QueueCleared { count } => {
                ctx.say(format!("Queue cleared! {count} songs removed."))
                    .await?;
            }
            QueueRemovalEvent::Error(e) => {
                return notify_error(&ctx, format!("Removal Error: {e:?}")).await;
            }
        }
    }

    Ok(())
}

#[derive(Debug)]
enum QueueRemovalCondition {
    All,
    Duplicates,
    Indices(String),
    FromUser(UserId),
}

#[instrument(skip(ctx))]
async fn notify_error<D, T>(ctx: &Context<'_>, error_msg: D) -> anyhow::Result<T>
where
    D: std::fmt::Debug + std::fmt::Display,
{
    error!("{error_msg:?}");

    ctx.say(format!("Error: {error_msg:?}"))
        .await
        .map_err::<anyhow::Error, _>(|e| e.into())
        .map(|_msg| ())?;

    Err(anyhow!("{error_msg:?}"))
}

async fn can_play_music(ctx: Context<'_>) -> anyhow::Result<bool> {
    Ok(ctx.data().config.music_bot.enabled && ctx.guild_id().is_some())
}
