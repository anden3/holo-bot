use std::{num::ParseIntError, sync::Arc};

use anyhow::anyhow;
use chrono::Utc;
use futures::{stream::FuturesOrdered, StreamExt};
use itertools::Itertools;
use rand::{seq::SliceRandom, thread_rng};
use serde_json::Value;
use serenity::{async_trait, model::id::GuildId, prelude::TypeMap};
use songbird::{
    create_player,
    input::{self, Input},
    tracks::{LoopState, PlayMode},
    Event, EventContext, EventHandler, Songbird, TrackEvent,
};
use tokio::sync::RwLock;

use super::prelude::*;

interaction_setup! {
    name = "music",
    group = "fun",
    description = "Play music from YouTube.",
    options = [
        //! Join your voice channel.
        join: SubCommand,
        //! Leaves your voice channel.
        leave: SubCommand,
        //! Set the volume.
        volume: SubCommand = [
            //! The volume you'd like, between 0 and 100.
            req volume: Integer,
        ],

        //! Plays a song immediately.
        play_now: SubCommand = [
            //! The song name or url you'd like to play.
            req song: String,
        ],

        //! Commands related to current song.
        song: SubCommandGroup = [
            /* //! Skip current song.
            skip: SubCommand = [
                //! How many songs to skip.
                amount: Integer,
            ],

            //! Seeks forward by a certain amount of seconds.
            forward: SubCommand = [
                //! How many seconds to seek forward.
                req seconds: Integer,
            ],

            //! Rewinds by a certain amount of seconds.
            rewind: SubCommand = [
                //! How many seconds to rewind.
                req seconds: Integer,
            ]

            //! Seeks to a certain position in the current song.
            seek: SubCommand = [
                //! The timestamp in the song to seek to. Example: 1:25.
                req position: String,
            ],

            //! Replays the current song from the beginning.
            replay: SubCommand, */

            //! Pauses the current song.
            pause: SubCommand,

            //! Resumes the current song.
            resume: SubCommand,

            /* //! Toggle looping the current song.
            r#loop: SubCommand,

            //! Shows the current song.
            now_playing: SubCommand,

            //! Get info about current song sent as a DM.
            grab: SubCommand, */
        ],

        //! Queue-related commands.
        queue: SubCommandGroup = [
            /* //! Shows the current queue.
            show: SubCommand, */

            //! Adds a song to the queue.
            add: SubCommand = [
                //! The song name or url you'd like to play.
                req song: String,
            ],

            //! Adds a song to the top of the queue.
            add_top: SubCommand = [
                //! The song name or url you'd like to play.
                req song: String,
            ]

            //! Removes songs from the queue.
            remove: SubCommand = [
                //! A position or list of positions, separated by spaces.
                req positions: String,
            ]

            /* //! Removes duplicate songs from the queue.
            remove_dupes: SubCommand,

            //! Clears the queue.
            clear: SubCommand = [
                //! Specify a user to remove all songs enqueued by them.
                user: User,
            ], */

            //! Shuffles the queue.
            shuffle: SubCommand,

            /* //! Moves a song to either the top of the queue or to a specified position.
            r#move: SubCommand = [
                //! The position of the song you'd like to move.
                req from: Integer,
                //! The new position of the song.
                to: Integer,
            ],

            //! Skips to a certain position in the queue.
            skip_to: SubCommand = [
                //! The position of the song you'd like to skip to.
                req position: Integer,
            ],

            //! Toggle looping the queue.
            r#loop: SubCommand,

            //! Removes queued songs from users that have left the voice channel.
            leave_cleanup: SubCommand, */
        ],
    ],
}

enum SongType<'a> {
    Url(&'a str),
    Query(&'a str),
}

struct ResumeQueueAfterForcedSong {
    data: Arc<RwLock<TypeMap>>,
    guild_id: GuildId,
}

#[async_trait]
impl EventHandler for ResumeQueueAfterForcedSong {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        let mut data = self.data.write().await;
        let music_data = data.get_mut::<MusicData>().unwrap();

        if !music_data.is_guild_registered(&self.guild_id) {
            return None;
        }

        if let EventContext::Track(&[(_, track)]) = ctx {
            if let Some(track_option) = music_data.forced_songs.get_mut(&self.guild_id) {
                if let Some(stored_track) = track_option {
                    if stored_track.uuid() == track.uuid() {
                        *track_option = None;
                    } else {
                        // The forced song has changed, so queue should not be resumed.
                        return None;
                    }
                }
            }
        }

        if let Err(e) = music_data
            .queues
            .get_mut(&self.guild_id)
            .unwrap()
            .resume()
            .context(here!())
        {
            error!("{:?}", e);
        }

        None
    }
}

#[interaction_cmd]
async fn music(
    ctx: &Ctx,
    interaction: &ApplicationCommandInteraction,
    config: &Config,
) -> anyhow::Result<()> {
    show_deferred_response(interaction, ctx, false).await?;

    let guild_id = interaction
        .guild_id
        .ok_or_else(|| anyhow!("Could not get guild ID."))?;

    let manager = songbird::get(ctx)
        .await
        .ok_or_else(|| anyhow!("Songbird manager not available."))?;

    let mut data = ctx.data.write().await;

    let music_data = match data.get_mut::<MusicData>() {
        Some(d) => d,
        None => return notify_error(ctx, interaction, "Failed to get access to music data!").await,
    };

    match_sub_commands! {
        "join" => {
            join_channel(ctx, interaction, guild_id, manager, music_data).await?;
        },
        "leave" => {
            leave_channel(ctx, interaction, guild_id, manager, music_data).await?;
        },
        "volume" => |volume: req i32| {
            set_volume(ctx, interaction, guild_id, manager, music_data, volume).await?;
        },

        "play_now" => |song: req String| {
            play_now(ctx, interaction, guild_id, manager, music_data, song).await?;
        },

        "song pause" => {
            set_play_state(ctx, interaction, guild_id, manager, music_data, PlayMode::Pause).await?;
        },
        "song resume" => {
            set_play_state(ctx, interaction, guild_id, manager, music_data, PlayMode::Play).await?;
        },
        "song loop" => {
            toggle_song_loop(ctx, interaction, guild_id, manager, music_data).await?;
        },

        "queue add" => |song: req String| {
            add_to_queue(ctx, interaction, guild_id, manager, music_data, song, false).await?;
        },
        "queue add_top" => |song: req String| {
            add_to_queue(ctx, interaction, guild_id, manager, music_data, song, true).await?;
        },
        "queue remove" => |positions: req String| {
            remove_from_queue(ctx, interaction, guild_id, manager, music_data, QueueRemovalCondition::Indices(positions)).await?;
        },
        "queue remove_dupes" => {
            remove_from_queue(ctx, interaction, guild_id, manager, music_data, QueueRemovalCondition::Duplicates).await?;
        },
        "queue clear" => |user: Value| {
            warn!("{:#?}", user);
            send_response(ctx, interaction, format!("{:#?}", user)).await?;
        },
        "queue shuffle" => {
            shuffle_queue(ctx, interaction, guild_id, manager, music_data).await?;
        }
    }

    Ok(())
}

#[instrument(skip(ctx, interaction, guild_id, manager, music_data))]
async fn join_channel(
    ctx: &Ctx,
    interaction: &ApplicationCommandInteraction,
    guild_id: GuildId,
    manager: Arc<Songbird>,
    music_data: &mut MusicData,
) -> anyhow::Result<()> {
    let channel_id = ctx
        .cache
        .guild_field(guild_id, |g| {
            g.voice_states
                .get(&interaction.user.id)
                .and_then(|vs| vs.channel_id)
        })
        .await
        .flatten();

    let connect_to = match channel_id {
        Some(channel_id) => channel_id,
        None => {
            return send_response(
                ctx,
                interaction,
                "Could not find your voice channel, make sure you are in one.",
            )
            .await;
        }
    };

    match manager.join(guild_id, connect_to).await {
        (_, Ok(())) => debug!("Joined voice channel!"),
        (_, Err(e)) => return notify_error(ctx, interaction, e).await,
    }

    music_data.register_guild(guild_id);

    interaction
        .delete_original_interaction_response(&ctx.http)
        .await?;

    Ok(())
}

#[instrument(skip(ctx, interaction, guild_id, manager, music_data))]
async fn leave_channel(
    ctx: &Ctx,
    interaction: &ApplicationCommandInteraction,
    guild_id: GuildId,
    manager: Arc<Songbird>,
    music_data: &mut MusicData,
) -> anyhow::Result<()> {
    if manager.get(guild_id).is_none() {
        return send_response(ctx, interaction, "Not in a voice channel.").await;
    }

    music_data.deregister_guild(&guild_id)?;

    match manager.remove(guild_id).await.context(here!()) {
        Err(e) => {
            return notify_error(ctx, interaction, e).await;
        }
        Ok(()) => debug!("Left voice channel!"),
    }

    interaction
        .delete_original_interaction_response(&ctx.http)
        .await?;

    Ok(())
}

#[instrument(skip(ctx, interaction, guild_id, manager, music_data))]
async fn set_volume(
    ctx: &Ctx,
    interaction: &ApplicationCommandInteraction,
    guild_id: GuildId,
    manager: Arc<Songbird>,
    music_data: &mut MusicData,
    volume: i32,
) -> anyhow::Result<()> {
    if manager.get(guild_id).is_none() {
        return send_response(ctx, interaction, "Not in a voice channel.").await;
    }

    let volume = (volume.clamp(0, 100) as f32) / 100.0;
    music_data.set_volume(&guild_id, volume)?;

    interaction
        .delete_original_interaction_response(&ctx.http)
        .await?;

    Ok(())
}

#[instrument(skip(ctx, interaction, guild_id, manager, music_data))]
async fn play_now(
    ctx: &Ctx,
    interaction: &ApplicationCommandInteraction,
    guild_id: GuildId,
    manager: Arc<Songbird>,
    music_data: &mut MusicData,
    song: String,
) -> anyhow::Result<()> {
    let handler_lock = match manager.get(guild_id) {
        Some(handler_lock) => handler_lock,
        _ => {
            return send_response(ctx, interaction, "Not in a voice channel to play in").await;
        }
    };

    let mut handler = handler_lock.lock().await;

    let source = match parse_input(&song).await {
        Ok(source) => source,
        Err(e) => {
            return notify_error(ctx, interaction, e).await;
        }
    };

    let queue = music_data.queues.get(&guild_id).unwrap();
    queue.pause()?;

    let forced_song = music_data.forced_songs.get_mut(&guild_id).unwrap();

    debug!(song = ?source.metadata.title, "Playing song!");
    let new_song = handler.play_source(source);

    new_song.add_event(
        Event::Track(TrackEvent::End),
        ResumeQueueAfterForcedSong {
            data: ctx.data.clone(),
            guild_id,
        },
    )?;

    if let Some(misc_data) = music_data.misc_data.get(&guild_id) {
        new_song.set_volume(misc_data.volume)?;
    }

    new_song
        .typemap()
        .write()
        .await
        .insert::<TrackMetaData>(TrackMetaData {
            added_by: interaction.user.id,
            added_at: Utc::now(),
        });

    if let Some(current_forced_song) = forced_song.replace(new_song) {
        current_forced_song.stop()?;
    }

    send_response(ctx, interaction, "Playing song").await?;

    Ok(())
}

#[instrument(skip(ctx, interaction, guild_id, manager, music_data))]
async fn set_play_state(
    ctx: &Ctx,
    interaction: &ApplicationCommandInteraction,
    guild_id: GuildId,
    manager: Arc<Songbird>,
    music_data: &mut MusicData,
    state: PlayMode,
) -> anyhow::Result<()> {
    if manager.get(guild_id).is_none() {
        return send_response(ctx, interaction, "Not in a voice channel.").await;
    }

    match music_data.get_current(&guild_id) {
        CurrentTrack::Forced(track) | CurrentTrack::InQueue(track) => match state {
            PlayMode::Play => track.play()?,
            PlayMode::Pause => track.pause()?,
            PlayMode::Stop => track.stop()?,
            _ => (),
        },
        CurrentTrack::None => {
            return send_response(ctx, interaction, "No song is playing.").await;
        }
    }

    interaction
        .delete_original_interaction_response(&ctx.http)
        .await?;

    Ok(())
}

#[instrument(skip(ctx, interaction, guild_id, manager, music_data))]
async fn toggle_song_loop(
    ctx: &Ctx,
    interaction: &ApplicationCommandInteraction,
    guild_id: GuildId,
    manager: Arc<Songbird>,
    music_data: &mut MusicData,
) -> anyhow::Result<()> {
    if manager.get(guild_id).is_none() {
        return send_response(ctx, interaction, "Not in a voice channel.").await;
    }

    match music_data.get_current(&guild_id) {
        CurrentTrack::Forced(track) | CurrentTrack::InQueue(track) => {
            match track.get_info().await?.loops {
                LoopState::Finite(0) => track.enable_loop()?,
                LoopState::Infinite | LoopState::Finite(_) => track.disable_loop()?,
            }
        }
        CurrentTrack::None => {
            return send_response(ctx, interaction, "No song is playing.").await;
        }
    }

    interaction
        .delete_original_interaction_response(&ctx.http)
        .await?;

    Ok(())
}

#[instrument(skip(ctx, interaction, guild_id, manager, music_data))]
async fn add_to_queue(
    ctx: &Ctx,
    interaction: &ApplicationCommandInteraction,
    guild_id: GuildId,
    manager: Arc<Songbird>,
    music_data: &mut MusicData,
    song: String,
    add_to_top: bool,
) -> anyhow::Result<()> {
    let handler = match manager.get(guild_id) {
        Some(handler) => handler,
        None => return send_response(ctx, interaction, "Not in a voice channel.").await,
    };

    let mut handler = handler.lock().await;

    let queue = music_data.queues.get(&guild_id).unwrap();
    let current_song = music_data.get_current(&guild_id);
    let queue_was_empty = queue.is_empty();

    // Prepare track.
    let input = parse_input(&song).await?;
    let (track, track_handle) = create_player(input);

    if let Some(misc_data) = music_data.misc_data.get(&guild_id) {
        track_handle.set_volume(misc_data.volume)?;
    }

    track_handle
        .typemap()
        .write()
        .await
        .insert::<TrackMetaData>(TrackMetaData {
            added_by: interaction.user.id,
            added_at: Utc::now(),
        });

    queue.add(track, &mut handler);

    // Check if queue should be played or paused, depending on if a forced track is active or the queue was empty.
    let first_unplayed_track_position = match current_song {
        CurrentTrack::Forced(_) => {
            track_handle.pause()?;
            0
        }
        CurrentTrack::InQueue(_) => 1,
        CurrentTrack::None => {
            queue.resume()?;
            0
        }
    };

    // Make sure to not insert at the very front if the queue is being played right now.
    if add_to_top && !queue_was_empty {
        queue.modify_queue(|q| {
            let track = q.pop_back().unwrap();
            q.insert(first_unplayed_track_position, track);
        })
    }

    send_response(
        ctx,
        interaction,
        format!(
            "Playing {} by {}.",
            track_handle
                .metadata()
                .track
                .clone()
                .unwrap_or_else(|| "UNKNOWN".to_string()),
            track_handle
                .metadata()
                .artist
                .clone()
                .unwrap_or_else(|| "UNKNOWN".to_string())
        ),
    )
    .await?;

    Ok(())
}

#[instrument(skip(ctx, interaction, guild_id, manager, music_data))]
async fn remove_from_queue(
    ctx: &Ctx,
    interaction: &ApplicationCommandInteraction,
    guild_id: GuildId,
    manager: Arc<Songbird>,
    music_data: &mut MusicData,
    removal_condition: QueueRemovalCondition,
) -> anyhow::Result<()> {
    if manager.get(guild_id).is_none() {
        return send_response(ctx, interaction, "Not in a voice channel.").await;
    }

    let queue = music_data.queues.get(&guild_id).unwrap();
    let queue_in_use = matches!(music_data.get_current(&guild_id), CurrentTrack::InQueue(_));

    if queue.is_empty() || (queue_in_use && queue.len() == 1) {
        interaction
            .delete_original_interaction_response(&ctx.http)
            .await?;

        return Ok(());
    }

    match removal_condition {
        QueueRemovalCondition::All => {
            if queue_in_use {
                queue.modify_queue(|q| q.truncate(1));
            } else {
                queue.stop();
            }
        }
        QueueRemovalCondition::Duplicates => {
            queue.modify_queue(|q| {
                let duplicates = q
                    .iter()
                    .enumerate()
                    .duplicates_by(|(_, t)| t.uuid())
                    .map(|(i, _)| i)
                    .collect::<HashSet<_>>();

                let mut is_unique = (0..q.len()).map(|i| !duplicates.contains(&i));
                q.retain(|_| is_unique.next().unwrap());
            });
        }
        QueueRemovalCondition::Indices(indices) => {
            queue.modify_queue::<_, Result<(), ParseIntError>>(|q| {
                let mut indices = indices
                    .split(' ')
                    .map(|n| n.parse::<usize>())
                    .collect::<Result<HashSet<_>, _>>()?;

                if queue_in_use {
                    indices.remove(&0);
                }

                let mut is_retained = (0..q.len()).map(|i| !indices.contains(&i));
                q.retain(|_| is_retained.next().unwrap());

                Ok(())
            })?;
        }
        QueueRemovalCondition::FromUser(user_id) => {
            let current_queue = queue.current_queue();
            let mut is_retained = current_queue
                .iter()
                .map(|t| t.typemap().read())
                .collect::<FuturesOrdered<_>>()
                .collect::<Vec<_>>()
                .await
                .into_iter()
                .map(|t| {
                    t.get::<TrackMetaData>()
                        .map(|d| d.added_by != user_id)
                        .unwrap_or(true)
                })
                .collect::<Vec<_>>();

            if queue_in_use {
                is_retained[0] = true;
            }

            let mut is_retained = is_retained.into_iter();
            queue.modify_queue(|q| q.retain(|_| is_retained.next().unwrap()));
        }
    }

    interaction
        .delete_original_interaction_response(&ctx.http)
        .await?;

    Ok(())
}

#[instrument(skip(ctx, interaction, guild_id, manager, music_data))]
async fn shuffle_queue(
    ctx: &Ctx,
    interaction: &ApplicationCommandInteraction,
    guild_id: GuildId,
    manager: Arc<Songbird>,
    music_data: &mut MusicData,
) -> anyhow::Result<()> {
    if manager.get(guild_id).is_none() {
        return send_response(ctx, interaction, "Not in a voice channel.").await;
    }

    let queue = music_data.queues.get(&guild_id).unwrap();
    let queue_in_use = matches!(music_data.get_current(&guild_id), CurrentTrack::InQueue(_));

    if queue.is_empty() || (queue_in_use && queue.len() <= 2) {
        interaction
            .delete_original_interaction_response(&ctx.http)
            .await?;

        return Ok(());
    }

    queue.modify_queue(|q| {
        let slice = q.make_contiguous();

        let slice = if queue_in_use {
            let (_, slice) = slice.split_at_mut(1);
            slice
        } else {
            slice
        };

        slice.shuffle(&mut thread_rng());
    });

    interaction
        .delete_original_interaction_response(&ctx.http)
        .await?;

    Ok(())
}

#[instrument]
async fn parse_input(song: &str) -> anyhow::Result<Input> {
    let song = match song.trim().to_lowercase().starts_with("http") {
        true => SongType::Url(song),
        false => SongType::Query(song),
    };

    match song {
        SongType::Url(url) => input::ytdl(url).await.context(here!()),
        SongType::Query(query) => input::ytdl_search(query).await.context(here!()),
    }
}

#[instrument(skip(ctx, interaction))]
async fn send_response<D: ToString + std::fmt::Debug>(
    ctx: &Ctx,
    interaction: &ApplicationCommandInteraction,
    response: D,
) -> anyhow::Result<()> {
    interaction
        .edit_original_interaction_response(&ctx.http, |r| r.content(response))
        .await
        .map_err(|e| e.into())
        .map(|_msg| ())
}

#[instrument(skip(ctx, interaction))]
async fn notify_error<D>(
    ctx: &Ctx,
    interaction: &ApplicationCommandInteraction,
    error_msg: D,
) -> anyhow::Result<()>
where
    D: std::fmt::Debug + std::fmt::Display,
{
    send_response(ctx, interaction, format!("Error: {:?}", error_msg)).await?;
    Err(anyhow!("{:?}", error_msg))
}
