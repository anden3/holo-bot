use std::sync::Arc;

use anyhow::anyhow;
use chrono::Utc;
use regex::Regex;
use serde_json::Value;
use serenity::model::id::{GuildId, UserId};
use songbird::Songbird;

use super::prelude::*;

interaction_setup! {
    name = "music",
    group = "fun",
    description = "Play music from YouTube.",
    options = [
        //! Join your voice channel.
        join | j: SubCommand,
        //! Leaves your voice channel.
        leave | l: SubCommand,
        /* //! Set the volume.
        volume | vol: SubCommand = [
            //! The volume you'd like, between 0 and 100.
            req volume: Integer,
        ],

        //! Plays a song immediately.
        play_now: SubCommand = [
            //! The song name or url you'd like to play.
            req song: String,
        ],

        //! Pauses the current song.
        pause: SubCommand,
        //! Resumes the current song.
        resume: SubCommand,
        //! Skip current song.
        skip | s: SubCommand = [
            //! How many songs to skip.
            amount: Integer,
        ],
        //! Toggle looping the current song.
        r#loop: SubCommand,

        //! Shows the current queue.
        queue | q: SubCommand, */
        //! Adds a song to the queue.
        add | p: SubCommand = [
            //! The song name or url you'd like to play.
            req song: String,
        ],
        //! Adds all the songs on a playlist to the queue.
        add_playlist | pl: SubCommand = [
            //! The playlist url.
            req playlist: String,
        ],
        /* //! Adds a song to the top of the queue.
        top | t: SubCommand = [
            //! The song name or url you'd like to play.
            req song: String,
        ]
        //! Shuffles the queue.
        shuffle: SubCommand,
        //! Removes songs from the queue.
        remove | r: SubCommand = [
            //! A position or list of positions, separated by spaces.
            req positions: String,
        ] */

        /* //! Commands related to current song.
        song: SubCommandGroup = [
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
            replay: SubCommand,

            //! Shows the current song.
            now_playing: SubCommand,

            //! Get info about current song sent as a DM.
            grab: SubCommand,
        ], */

        /* //! Queue-related commands.
        queue: SubCommandGroup = [
            //! Removes duplicate songs from the queue.
            remove_dupes: SubCommand,

            //! Clears the queue.
            clear: SubCommand = [
                //! Specify a user to remove all songs enqueued by them.
                user: User,
            ],

            //! Moves a song to either the top of the queue or to a specified position.
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
            leave_cleanup: SubCommand,
        ], */
    ],
}

#[derive(Debug)]
enum SubCommandReturnValue {
    None,
    DeleteInteraction,
    EditInteraction(String),
}
#[derive(Debug)]
enum QueueRemovalCondition {
    All,
    Duplicates,
    Indices(String),
    FromUser(UserId),
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

    let queue = match ctx.data.read().await.get::<MusicData>() {
        Some(d) => d.get_queue(&guild_id),
        None => {
            return notify_error(ctx, interaction, "Failed to get access to music data!").await;
        }
    };

    let result = match_sub_commands! {
        type SubCommandReturnValue,
        [
            "join" | "j" => {
                let mut data = ctx.data.write().await;

                let music_data = match data.get_mut::<MusicData>() {
                    Some(d) => d,
                    None => return notify_error(ctx, interaction, "Failed to get access to music data!").await,
                };

                join_channel(ctx, interaction, guild_id, &manager, music_data).await?
            },
            "leave" | "l" => {
                let mut data = ctx.data.write().await;

                let music_data = match data.get_mut::<MusicData>() {
                    Some(d) => d,
                    None => return notify_error(ctx, interaction, "Failed to get access to music data!").await,
                };

                leave_channel(guild_id, &manager, music_data).await?
            },
            "volume" | "vol" => |volume: req i32| {
                set_volume(queue, volume).await?
            },
            "pause" => {
                set_play_state(queue, PlayStateChange::Pause).await?
            },
            "resume" => {
                set_play_state(queue, PlayStateChange::Resume).await?
            },
            "loop" => {
                set_play_state(queue, PlayStateChange::ToggleLoop).await?
            },

            /* "queue" | "q" => {
                show_queue(ctx, interaction, guild_id, &manager, music_data).await?
            }, */
            "play" | "p" => |song: req String| {
                add_to_queue(interaction, queue, song, false).await?
            },
            "play_now" => |song: req String| {
                play_now(interaction, queue, song).await?
            },
            "add_playlist" | "pl" => |playlist: req String| {
                add_playlist(interaction, queue, playlist).await?
            }
            "top" | "t" => |song: req String| {
                add_to_queue(interaction, queue, song, true).await?
            },
            "skip" | "s" => |amount: i32| {
                skip_songs(queue, amount.unwrap_or(1)).await?
            },
            "remove" | "r" => |positions: req String| {
                remove_from_queue(queue, QueueRemovalCondition::Indices(positions)).await?
            },
            "remove_dupes" | "rd" => {
                remove_from_queue(queue, QueueRemovalCondition::Duplicates).await?
            },
            "shuffle" => {
                shuffle_queue(queue).await?
            }
            "clear" => |user: Value| {
                warn!("{:#?}", user);
                send_response(ctx, interaction, format!("{:#?}", user)).await?;
                SubCommandReturnValue::None
            },
        ]
    };

    match result {
        Some(SubCommandReturnValue::DeleteInteraction) => {
            interaction
                .delete_original_interaction_response(&ctx.http)
                .await?;
        }
        Some(SubCommandReturnValue::EditInteraction(message)) => {
            interaction
                .edit_original_interaction_response(&ctx.http, |r| r.content(message))
                .await?;
        }
        Some(SubCommandReturnValue::None) => (),
        None => (),
    }

    Ok(())
}

#[instrument(skip(ctx, interaction, guild_id, manager, music_data))]
async fn join_channel(
    ctx: &Ctx,
    interaction: &ApplicationCommandInteraction,
    guild_id: GuildId,
    manager: &Arc<Songbird>,
    music_data: &mut MusicData,
) -> anyhow::Result<SubCommandReturnValue> {
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
            return Ok(SubCommandReturnValue::EditInteraction(
                "Could not find your voice channel, make sure you are in one peko.".to_string(),
            ));
        }
    };

    match manager.join(guild_id, connect_to).await {
        (_, Ok(())) => debug!("Joined voice channel!"),
        (_, Err(e)) => return notify_error(ctx, interaction, e).await,
    }

    music_data.register_guild(Arc::clone(manager), &guild_id);

    Ok(SubCommandReturnValue::DeleteInteraction)
}

#[instrument(skip(guild_id, manager, music_data))]
async fn leave_channel(
    guild_id: GuildId,
    manager: &Arc<Songbird>,
    music_data: &mut MusicData,
) -> anyhow::Result<SubCommandReturnValue> {
    if manager.get(guild_id).is_none() {
        return Ok(SubCommandReturnValue::EditInteraction(
            "I'm not in a voice channel peko.".to_string(),
        ));
    }

    music_data.deregister_guild(&guild_id);
    Ok(SubCommandReturnValue::DeleteInteraction)
}

#[instrument(skip(queue))]
async fn set_volume(
    queue: Option<BufferedQueue>,
    volume: i32,
) -> anyhow::Result<SubCommandReturnValue> {
    let queue = match queue {
        Some(q) => q,
        None => {
            return Ok(SubCommandReturnValue::EditInteraction(
                "I'm not in a voice channel peko.".to_string(),
            ));
        }
    };

    let volume = (volume.clamp(0, 100) as f32) / 100.0;
    queue.set_volume(volume).await?;

    Ok(SubCommandReturnValue::DeleteInteraction)
}

#[instrument(skip(interaction, queue))]
async fn play_now(
    interaction: &ApplicationCommandInteraction,
    queue: Option<BufferedQueue>,
    song: String,
) -> anyhow::Result<SubCommandReturnValue> {
    let queue = match queue {
        Some(q) => q,
        None => {
            return Ok(SubCommandReturnValue::EditInteraction(
                "I'm not in a voice channel peko.".to_string(),
            ));
        }
    };

    queue
        .play_now(EnqueuedItem {
            item: song,
            metadata: TrackMetaData {
                added_by: interaction.user.id,
                added_at: Utc::now(),
            },
        })
        .await?;

    Ok(SubCommandReturnValue::DeleteInteraction)
}

#[instrument(skip(queue))]
async fn set_play_state(
    queue: Option<BufferedQueue>,
    state: PlayStateChange,
) -> anyhow::Result<SubCommandReturnValue> {
    let queue = match queue {
        Some(q) => q,
        None => {
            return Ok(SubCommandReturnValue::EditInteraction(
                "I'm not in a voice channel peko.".to_string(),
            ));
        }
    };

    queue.set_play_state(state).await?;

    Ok(SubCommandReturnValue::DeleteInteraction)
}

#[instrument(skip(queue))]
async fn skip_songs(
    queue: Option<BufferedQueue>,
    amount: i32,
) -> anyhow::Result<SubCommandReturnValue> {
    let queue = match queue {
        Some(q) => q,
        None => {
            return Ok(SubCommandReturnValue::EditInteraction(
                "I'm not in a voice channel peko.".to_string(),
            ));
        }
    };

    if amount <= 0 {
        return Ok(SubCommandReturnValue::EditInteraction(
            "I can't skip 0 or fewer songs peko.".to_string(),
        ));
    }

    queue.skip(amount as u32).await?;
    Ok(SubCommandReturnValue::DeleteInteraction)
}

/* #[instrument(skip(ctx, interaction, guild_id, manager, music_data))]
async fn show_queue(
    ctx: &Ctx,
    interaction: &ApplicationCommandInteraction,
    guild_id: GuildId,
    manager: &Arc<Songbird>,
    music_data: &mut MusicData,
) -> anyhow::Result<SubCommandReturnValue> {
    if manager.get(guild_id).is_none() {
        return Ok(SubCommandReturnValue::EditInteraction(
            "Not in a voice channel peko.".to_string(),
        ));
    }

    let queue = music_data.queues.get(&guild_id).unwrap().current_queue();

    let track_metadata = queue.iter().map(|t| t.metadata()).cloned();

    let track_extra_metadata = queue
        .iter()
        .map(|t| t.typemap().read())
        .collect::<FuturesOrdered<_>>()
        .filter_map(|f| async move {
            match f.get::<TrackMetaData>() {
                Some(metadata) => {
                    match metadata.fetch_data(ctx, &guild_id).await.context(here!()) {
                        Ok(data) => Some(Some(data)),
                        Err(e) => {
                            error!("{:?}", e);
                            Some(None)
                        }
                    }
                }
                None => Some(None),
            }
        })
        .collect::<Vec<_>>()
        .await;

    let track_data = track_metadata
        .zip(track_extra_metadata)
        .enumerate()
        .collect::<Vec<_>>();

    PaginatedList::new()
        .title("Queue")
        .data(&track_data)
        .embed(Box::new(|(i, (meta, extra)), _| {
            let mut embed = CreateEmbed::default();

            if let Some(thumbnail) = &meta.thumbnail {
                embed.thumbnail(thumbnail.to_owned());
            }

            if let Some(title) = &meta.title {
                embed.description(format!("{} - {}", i, title.to_owned()));
            }

            if let Some(extra_data) = extra {
                let member = &extra_data.added_by;

                if let Some(colour) = extra_data.member_colour {
                    embed.colour(colour);
                }

                embed.footer(|f| f.text(format!("Added by: {}", member.user.tag())));
                embed.timestamp(&extra_data.added_at);
            }

            embed
        }))
        .display(interaction, ctx)
        .await?;

    Ok(SubCommandReturnValue::None)
} */

#[instrument(skip(interaction, queue))]
async fn add_to_queue(
    interaction: &ApplicationCommandInteraction,
    queue: Option<BufferedQueue>,
    song: String,
    add_to_top: bool,
) -> anyhow::Result<SubCommandReturnValue> {
    let queue = match queue {
        Some(q) => q,
        None => {
            return Ok(SubCommandReturnValue::EditInteraction(
                "I'm not in a voice channel peko.".to_string(),
            ));
        }
    };

    let url = match song.trim().to_lowercase().starts_with("http") {
        true => song,
        false => format!("ytsearch1:{}", song),
    };

    if add_to_top {
        queue
            .enqueue_top(EnqueuedItem {
                item: url,
                metadata: TrackMetaData {
                    added_by: interaction.user.id,
                    added_at: Utc::now(),
                },
            })
            .await?;
    } else {
        queue
            .enqueue(EnqueueType::Track(EnqueuedItem {
                item: url,
                metadata: TrackMetaData {
                    added_by: interaction.user.id,
                    added_at: Utc::now(),
                },
            }))
            .await?;
    }

    /* let metadata = track_handle.metadata();
    let track_name = metadata.title.clone().unwrap_or_else(|| {
        metadata
            .track
            .clone()
            .unwrap_or_else(|| "UNKNOWN".to_string())
    });

    Ok(SubCommandReturnValue::EditInteraction(format!(
        "Queued {} by {}.",
        track_name,
        metadata
            .artist
            .clone()
            .unwrap_or_else(|| "UNKNOWN".to_string())
    ))) */

    Ok(SubCommandReturnValue::DeleteInteraction)
}

#[instrument(skip(interaction, queue))]
async fn add_playlist(
    interaction: &ApplicationCommandInteraction,
    queue: Option<BufferedQueue>,
    playlist: String,
) -> anyhow::Result<SubCommandReturnValue> {
    let queue = match queue {
        Some(q) => q,
        None => {
            return Ok(SubCommandReturnValue::EditInteraction(
                "I'm not in a voice channel peko.".to_string(),
            ));
        }
    };

    // Thanks youtube-dl for being open-source <3
    let playlist_rgx: &'static Regex =
        regex!(r"(?:(?:PL|LL|EC|UU|FL|RD|UL|TL|PU|OLAK5uy_)[0-9A-Za-z-_]{10,}|RDMM)");

    let playlist_id = match playlist_rgx.find(&playlist) {
        Some(m) => &playlist[m.start()..m.end()],
        None => {
            return Ok(SubCommandReturnValue::EditInteraction(
                "URL does not contain a playlist ID.".to_string(),
            ));
        }
    };

    queue
        .enqueue(EnqueueType::Playlist(EnqueuedItem {
            item: playlist_id.to_string(),
            metadata: TrackMetaData {
                added_by: interaction.user.id,
                added_at: Utc::now(),
            },
        }))
        .await?;

    Ok(SubCommandReturnValue::DeleteInteraction)
}

#[instrument(skip(queue))]
async fn remove_from_queue(
    queue: Option<BufferedQueue>,
    removal_condition: QueueRemovalCondition,
) -> anyhow::Result<SubCommandReturnValue> {
    let queue = match queue {
        Some(q) => q,
        None => {
            return Ok(SubCommandReturnValue::EditInteraction(
                "I'm not in a voice channel peko.".to_string(),
            ));
        }
    };

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
                    error!("{:?}", e);
                    return Ok(SubCommandReturnValue::EditInteraction(
                        "Failed to parse index.".to_string(),
                    ));
                }
            }
        }
    };

    queue.remove(removal_condition).await?;
    Ok(SubCommandReturnValue::DeleteInteraction)
}

#[instrument(skip(queue))]
async fn shuffle_queue(queue: Option<BufferedQueue>) -> anyhow::Result<SubCommandReturnValue> {
    let queue = match queue {
        Some(q) => q,
        None => {
            return Ok(SubCommandReturnValue::EditInteraction(
                "I'm not in a voice channel peko.".to_string(),
            ));
        }
    };

    queue.shuffle().await?;
    Ok(SubCommandReturnValue::DeleteInteraction)
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
async fn notify_error<D, T>(
    ctx: &Ctx,
    interaction: &ApplicationCommandInteraction,
    error_msg: D,
) -> anyhow::Result<T>
where
    D: std::fmt::Debug + std::fmt::Display,
{
    send_response(ctx, interaction, format!("Error: {:?}", error_msg)).await?;
    Err(anyhow!("{:?}", error_msg))
}
