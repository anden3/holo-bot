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
        //! Set the volume.
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

        /* //! Shows the current queue.
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
        //! Adds a song to the top of the queue.
        top | t: SubCommand = [
            //! The song name or url you'd like to play.
            req song: String,
        ],
        //! Shuffles the queue.
        shuffle: SubCommand,
        //! Removes songs from the queue.
        remove | r: SubCommand = [
            //! A position or list of positions, separated by spaces.
            req positions: String,
        ],
        //! Removes duplicate songs from the queue.
        remove_dupes: SubCommand,

        //! Clears the queue.
        clear: SubCommand = [
            //! Specify a user to remove all songs enqueued by them.
            user: User,
        ],

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

            //! Removes queued songs from users that have left the voice channel.
            leave_cleanup: SubCommand,
        ] */
    ]
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
                join_channel(ctx, interaction, guild_id, &manager).await?
            },
            "leave" | "l" => {
                leave_channel(ctx, guild_id, &manager).await?
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
                add_to_queue(ctx, interaction, queue, song, false).await?
            },
            "play_now" => |song: req String| {
                play_now(ctx, interaction, queue, song).await?
            },
            "add_playlist" | "pl" => |playlist: req String| {
                add_playlist(ctx, interaction, queue, playlist).await?
            }
            "top" | "t" => |song: req String| {
                add_to_queue(ctx, interaction, queue, song, true).await?
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
                if let Some(user_str) = user.and_then(|u| u.as_str().map(|s| s.to_owned())) {
                    let user_id = user_str.parse::<u64>()?.into();

                    remove_from_queue(
                        queue,
                        QueueRemovalCondition::FromUser(user_id),
                    ).await?
                }
                else {
                    remove_from_queue(queue, QueueRemovalCondition::All).await?
                }
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

#[instrument(skip(ctx, interaction, guild_id, manager))]
async fn join_channel(
    ctx: &Ctx,
    interaction: &ApplicationCommandInteraction,
    guild_id: GuildId,
    manager: &Arc<Songbird>,
) -> anyhow::Result<SubCommandReturnValue> {
    let mut data = ctx.data.write().await;

    let music_data = match data.get_mut::<MusicData>() {
        Some(d) => d,
        None => {
            return Ok(SubCommandReturnValue::EditInteraction(
                "Failed to get access to music data!".to_string(),
            ))
        }
    };

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
        (_, Err(e)) => {
            return Ok(SubCommandReturnValue::EditInteraction(format!(
                "Failed to join channel: {:?}",
                e
            )))
        }
    }

    music_data.register_guild(Arc::clone(manager), &guild_id);
    Ok(SubCommandReturnValue::DeleteInteraction)
}

#[instrument(skip(ctx, guild_id, manager))]
async fn leave_channel(
    ctx: &Ctx,
    guild_id: GuildId,
    manager: &Arc<Songbird>,
) -> anyhow::Result<SubCommandReturnValue> {
    if manager.get(guild_id).is_none() {
        return Ok(SubCommandReturnValue::EditInteraction(
            "I'm not in a voice channel peko.".to_string(),
        ));
    }

    let mut data = ctx.data.write().await;

    let music_data = match data.get_mut::<MusicData>() {
        Some(d) => d,
        None => {
            return Ok(SubCommandReturnValue::EditInteraction(
                "Failed to get access to music data!".to_string(),
            ))
        }
    };

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

#[instrument(skip(ctx, interaction, queue))]
async fn play_now(
    ctx: &Ctx,
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

    let mut collector = queue
        .play_now(EnqueuedItem {
            item: song,
            metadata: TrackMetaData {
                added_by: interaction.user.id,
                added_at: Utc::now(),
            },
        })
        .await?;

    while let Some(evt) = collector.recv().await {
        match evt {
            QueuePlayNowEvent::Playing(track) => {
                let _ = interaction
                    .edit_original_interaction_response(ctx, |e| {
                        e.create_embed(|e| {
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
                                .footer(|f| f.text(format!("Added by {}", interaction.user.tag())))
                        })
                    })
                    .await?;
            }
            QueuePlayNowEvent::Error(e) => {
                return Ok(SubCommandReturnValue::EditInteraction(e));
            }
        }
    }

    Ok(SubCommandReturnValue::None)
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

    queue.skip(amount as usize).await?;
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

#[instrument(skip(ctx, interaction, queue))]
async fn add_to_queue(
    ctx: &Ctx,
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

    let enqueued_item = EnqueuedItem {
        item: url,
        metadata: TrackMetaData {
            added_by: interaction.user.id,
            added_at: Utc::now(),
        },
    };

    let mut collector = if add_to_top {
        queue.enqueue_top(enqueued_item).await?
    } else {
        queue.enqueue(EnqueueType::Track(enqueued_item)).await?
    };

    while let Some(evt) = collector.recv().await {
        match evt {
            QueueEnqueueEvent::TrackEnqueued(track) => {
                let _ = interaction
                    .edit_original_interaction_response(ctx, |e| {
                        e.create_embed(|e| {
                            e.author(|a| a.name("Queue Update"))
                                .title("Track added to queue!")
                                .fields([
                                    ("Position", (track.index + 1).to_string(), true),
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
                                .footer(|f| f.text(format!("Added by {}", interaction.user.tag())));

                            if let Some(thumbnail) = track.thumbnail {
                                e.thumbnail(thumbnail)
                            } else {
                                e
                            }
                        })
                    })
                    .await?;
            }

            QueueEnqueueEvent::TrackEnqueuedTop(track) => {
                let _ = interaction
                    .edit_original_interaction_response(ctx, |e| {
                        e.create_embed(|e| {
                            e.author(|a| a.name("Queue Update"))
                                .title("Track added to top of queue!")
                                .fields([
                                    ("Position", (track.index + 1).to_string(), true),
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
                                .footer(|f| f.text(format!("Added by {}", interaction.user.tag())));

                            if let Some(thumbnail) = track.thumbnail {
                                e.thumbnail(thumbnail)
                            } else {
                                e
                            }
                        })
                    })
                    .await?;
            }

            QueueEnqueueEvent::Error(e) => {
                return Ok(SubCommandReturnValue::EditInteraction(e));
            }

            _ => {
                return Ok(SubCommandReturnValue::EditInteraction(
                    "I somehow received a playlist event despite queueing a song peko? pardun!?"
                        .to_string(),
                ))
            }
        }
    }

    Ok(SubCommandReturnValue::DeleteInteraction)
}

#[instrument(skip(ctx, interaction, queue))]
async fn add_playlist(
    ctx: &Ctx,
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

    let mut collector = queue
        .enqueue(EnqueueType::Playlist(EnqueuedItem {
            item: playlist_id.to_string(),
            metadata: TrackMetaData {
                added_by: interaction.user.id,
                added_at: Utc::now(),
            },
        }))
        .await?;

    let mut playlist_processor_id = None;
    let mut playlist_length = 0;

    while let Some(evt) = collector.recv().await {
        match evt {
            QueueEnqueueEvent::TrackEnqueued(_track) => {}

            QueueEnqueueEvent::PlaylistProcessingStart(playlist) => {
                playlist_length = playlist.video_count;

                // TODO: Handle error.
                let _ = interaction
                    .edit_original_interaction_response(ctx, |e| {
                        e.create_embed(|e| {
                            e.author(|a| a.name("Playlist Processing"))
                                .title("Playlist found, starting processing...")
                                .description("Does this have to be here??")
                                .fields([
                                    ("Name", playlist.title, true),
                                    ("Description", playlist.description, true),
                                    ("Uploader", playlist.uploader, true),
                                ])
                                .footer(|f| f.text(format!("Added by {}", interaction.user.tag())))
                        })
                    })
                    .await
                    .context(here!());

                let followup = interaction
                    .create_followup_message(ctx, |e| {
                        e.username("Playlist Loader").content("Loading playlist...")
                    })
                    .await
                    .context(here!())?;

                playlist_processor_id = Some(followup.id);
            }

            QueueEnqueueEvent::PlaylistProcessingProgress(track) => {
                let followup_id = match playlist_processor_id {
                    Some(id) => id,
                    None => continue,
                };

                let _ = interaction
                    .edit_followup_message(ctx, followup_id, |e| {
                        e.create_embed(|e| {
                            e.author(|a| a.name("Queue Update"))
                                .title("Track added to top of queue!")
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
                                e.thumbnail(thumbnail)
                            } else {
                                e
                            }
                        })
                    })
                    .await
                    .context(here!())?;
            }

            QueueEnqueueEvent::PlaylistProcessingEnd => {
                let followup_id = match playlist_processor_id {
                    Some(id) => id,
                    None => continue,
                };

                interaction
                    .delete_followup_message(ctx, followup_id)
                    .await
                    .context(here!())?;
            }

            QueueEnqueueEvent::Error(e) => {
                return Ok(SubCommandReturnValue::EditInteraction(e));
            }

            _ => return Ok(SubCommandReturnValue::EditInteraction(
                "I somehow received a queue top event despite queueing a playlist peko? pardun!?"
                    .to_string(),
            )),
        }
    }

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
