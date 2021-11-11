use std::sync::Arc;

use anyhow::anyhow;
use chrono::Utc;
use music_queue::{
    events::*, metadata::*, EnqueueType, EnqueuedItem, MusicData, PlayStateChange,
    ProcessedQueueRemovalCondition, Queue, QueueItem, QueueItemData,
};
use regex::Regex;
use serde_json::Value;
use serenity::{
    builder::CreateEmbed,
    model::id::{GuildId, UserId},
};
use songbird::Songbird;

use super::prelude::*;

interaction_setup! {
    name = "music",
    group = "fun",
    description = "Play music from YouTube.",
    enabled_if = |config| config.music_bot.enabled,
    options = [
        //! Join your voice channel.
        join | j: SubCommand,
        //! Leaves your voice channel.
        leave | l: SubCommand,
        //! Set the volume.
        volume/*  | vol */: SubCommand = [
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

        //! Get the currently playing song, if any.
        now_playing: SubCommand,
        //! Shows the current queue.
        queue | q: SubCommand,
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

#[allow(dead_code)]
enum SubCommandReturnValue {
    None,
    Error(QueueError),
    DeleteInteraction,
    EditInteraction(String),
    EditEmbed(Box<dyn FnOnce(&mut CreateEmbed) -> &mut CreateEmbed + Send + Sync>),
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

    let user_id = interaction.user.id;

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
                set_volume(user_id, queue, volume).await?
            },
            "pause" => {
                set_play_state(user_id, queue, PlayStateChange::Pause).await?
            },
            "resume" => {
                set_play_state(user_id, queue, PlayStateChange::Resume).await?
            },
            "loop" => {
                set_play_state(user_id, queue, PlayStateChange::ToggleLoop).await?
            },
            "now_playing" => {
                now_playing(user_id, queue).await?
            },
            "queue" | "q" => {
                show_queue(ctx, interaction, guild_id, queue).await?
            },
            "add" | "p" => |song: req String| {
                add_to_queue(interaction, queue, song, false).await?
            },
            "play_now" => |song: req String| {
                play_now(interaction, queue, song).await?
            },
            "add_playlist" | "pl" => |playlist: req String| {
                add_playlist(ctx, interaction, queue, playlist).await?
            }
            "top" | "t" => |song: req String| {
                add_to_queue(interaction, queue, song, true).await?
            },
            "skip" | "s" => |amount: i32| {
                skip_songs(user_id, queue, amount.unwrap_or(1)).await?
            },
            "remove" | "r" => |positions: req String| {
                remove_from_queue(ctx, user_id, queue, QueueRemovalCondition::Indices(positions)).await?
            },
            "remove_dupes" | "rd" => {
                remove_from_queue(ctx, user_id, queue, QueueRemovalCondition::Duplicates).await?
            },
            "shuffle" => {
                shuffle_queue(user_id, queue).await?
            }
            "clear" => |user: Value| {
                if let Some(user_str) = user.and_then(|u| u.as_str().map(|s| s.to_owned())) {
                    let user_id = user_str.parse::<u64>()?.into();

                    remove_from_queue(
                        ctx,
                        user_id,
                        queue,
                        QueueRemovalCondition::FromUser(user_id),
                    ).await?
                }
                else {
                    remove_from_queue(ctx, user_id, queue, QueueRemovalCondition::All).await?
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
            debug!("Music response: {}", message);
            interaction
                .edit_original_interaction_response(&ctx.http, |r| r.content(message))
                .await?;
        }
        Some(SubCommandReturnValue::EditEmbed(f)) => {
            let mut embed = CreateEmbed::default();
            f(&mut embed);

            interaction
                .edit_original_interaction_response(&ctx.http, |r| r.set_embeds(vec![embed]))
                .await?;
        }
        Some(SubCommandReturnValue::Error(e)) => match e {
            QueueError::AccessDenied => {
                interaction
                    .edit_original_interaction_response(&ctx.http, |r| {
                        r.content("You don't have permission to do that, peko.")
                    })
                    .await?;
            }
            QueueError::NotInVoiceChannel => {
                interaction
                    .edit_original_interaction_response(&ctx.http, |r| {
                        r.content("You're not in a voice channel, peko.")
                    })
                    .await?;
            }
            QueueError::Other(e) => {
                debug!("Music error: {}", e);
                interaction
                    .edit_original_interaction_response(&ctx.http, |r| r.content(e))
                    .await?;
            }
        },
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
    {
        let data = ctx.data.read().await;

        if !data.contains_key::<MusicData>() {
            return Ok(SubCommandReturnValue::EditInteraction(
                "Failed to get access to music data!".to_string(),
            ));
        }
    }

    let channel_id = ctx
        .cache
        .guild_field(guild_id, |g| {
            g.voice_states
                .get(&interaction.user.id)
                .and_then(|vs| vs.channel_id)
        })
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

    {
        let mut data = ctx.data.write().await;
        let music_data = data.get_mut::<MusicData>().unwrap();

        music_data.register_guild(
            Arc::clone(manager),
            &guild_id,
            Arc::clone(&ctx.http),
            Arc::clone(&ctx.cache),
        );
    }

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

    match manager.leave(guild_id).await {
        Ok(()) => debug!("Joined voice channel!"),
        Err(e) => {
            return Ok(SubCommandReturnValue::EditInteraction(format!(
                "Failed to leave channel: {:?}",
                e
            )))
        }
    }

    {
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
    }

    Ok(SubCommandReturnValue::DeleteInteraction)
}

#[instrument(skip(queue))]
async fn set_volume(
    user_id: UserId,
    queue: Option<Queue>,
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
    let mut collector = queue.set_volume(user_id, volume).await?;

    if let Some(evt) = collector.recv().await {
        return Ok(match evt {
            QueueVolumeEvent::VolumeChanged(vol) => SubCommandReturnValue::EditInteraction(
                format!("Volume set to {}!", (vol * 100.0) as i32),
            ),
            QueueVolumeEvent::Error(e) => SubCommandReturnValue::Error(e),
        });
    }

    Ok(SubCommandReturnValue::DeleteInteraction)
}

#[instrument(skip(interaction, queue))]
async fn play_now(
    interaction: &ApplicationCommandInteraction,
    queue: Option<Queue>,
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

    let video_id_rgx = regex!(r"[0-9A-Za-z_-]{10}[048AEIMQUYcgkosw]");

    let url = video_id_rgx
        .find(&song)
        .map(|u| u.as_str().to_owned())
        .unwrap_or_else(|| format!("ytsearch1:{}", song.trim()));

    let mut collector = queue
        .play_now(
            interaction.user.id,
            EnqueuedItem {
                item: url,
                metadata: TrackMetaData {
                    added_by: interaction.user.id,
                    added_at: Utc::now(),
                },
                extracted_metadata: None,
            },
        )
        .await?;

    if let Some(evt) = collector.recv().await {
        return Ok(match evt {
            QueuePlayNowEvent::Playing(track) => {
                let user = interaction.user.tag();

                SubCommandReturnValue::EditEmbed(Box::new(move |e| {
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
                }))
            }
            QueuePlayNowEvent::Error(e) => SubCommandReturnValue::Error(e),
        });
    }

    Ok(SubCommandReturnValue::None)
}

#[instrument(skip(queue))]
async fn set_play_state(
    user_id: UserId,
    queue: Option<Queue>,
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

    let mut collector = queue.set_play_state(user_id, state).await?;

    if let Some(evt) = collector.recv().await {
        return Ok(match evt {
            QueuePlayStateEvent::Playing => {
                SubCommandReturnValue::EditInteraction("Resuming song!".to_string())
            }
            QueuePlayStateEvent::Paused => {
                SubCommandReturnValue::EditInteraction("Pausing song!".to_string())
            }
            QueuePlayStateEvent::StartedLooping => {
                SubCommandReturnValue::EditInteraction("Looping song!".to_string())
            }
            QueuePlayStateEvent::StoppedLooping => {
                SubCommandReturnValue::EditInteraction("No longer looping song!".to_string())
            }
            QueuePlayStateEvent::StateAlreadySet => {
                SubCommandReturnValue::EditInteraction("State already set!".to_string())
            }
            QueuePlayStateEvent::Error(e) => SubCommandReturnValue::Error(e),
        });
    }

    Ok(SubCommandReturnValue::DeleteInteraction)
}

#[instrument(skip(queue))]
async fn skip_songs(
    user_id: UserId,
    queue: Option<Queue>,
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

    let mut collector = queue.skip(user_id, amount as usize).await?;

    if let Some(evt) = collector.recv().await {
        return Ok(match evt {
            QueueSkipEvent::TracksSkipped { count } => {
                SubCommandReturnValue::EditInteraction(format!(
                    "Skipped {} {}!",
                    count,
                    if count > 1 { "tracks" } else { "track" }
                ))
            }
            QueueSkipEvent::Error(e) => SubCommandReturnValue::Error(e),
        });
    }

    Ok(SubCommandReturnValue::DeleteInteraction)
}

async fn now_playing(
    user_id: UserId,
    queue: Option<Queue>,
) -> anyhow::Result<SubCommandReturnValue> {
    let queue = match queue {
        Some(q) => q,
        None => {
            return Ok(SubCommandReturnValue::EditInteraction(
                "I'm not in a voice channel peko.".to_string(),
            ));
        }
    };

    let mut collector = queue.now_playing(user_id).await?;

    if let Some(evt) = collector.recv().await {
        return Ok(match evt {
            QueueNowPlayingEvent::NowPlaying(track) => {
                let track = match track {
                    Some(t) => t,
                    None => return Ok(SubCommandReturnValue::DeleteInteraction),
                };

                SubCommandReturnValue::EditEmbed(Box::new(move |e| {
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
                }))
            }
            QueueNowPlayingEvent::Error(e) => SubCommandReturnValue::Error(e),
        });
    }

    Ok(SubCommandReturnValue::DeleteInteraction)
}

#[instrument(skip(ctx, queue))]
async fn show_queue(
    ctx: &Ctx,
    interaction: &ApplicationCommandInteraction,
    guild_id: GuildId,
    queue: Option<Queue>,
) -> anyhow::Result<SubCommandReturnValue> {
    let queue = match queue {
        Some(q) => q,
        None => {
            return Ok(SubCommandReturnValue::EditInteraction(
                "I'm not in a voice channel peko.".to_string(),
            ));
        }
    };

    let mut collector = queue.show(interaction.user.id).await?;

    let queue_data = match collector.recv().await {
        Some(QueueShowEvent::CurrentQueue(queue)) => queue,
        Some(QueueShowEvent::Error(e)) => {
            return Ok(SubCommandReturnValue::Error(e));
        }
        None => return Ok(SubCommandReturnValue::DeleteInteraction),
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
        .display(ctx, interaction)
        .await?;

    Ok(SubCommandReturnValue::None)
}

#[instrument(skip(interaction, queue))]
async fn add_to_queue(
    interaction: &ApplicationCommandInteraction,
    queue: Option<Queue>,
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

    let video_id_rgx = regex!(r"[0-9A-Za-z_-]{10}[048AEIMQUYcgkosw]");

    let url = video_id_rgx
        .find(&song)
        .map(|u| u.as_str().to_owned())
        .unwrap_or_else(|| format!("ytsearch1:{}", song.trim()));

    let enqueued_item = EnqueuedItem {
        item: url,
        metadata: TrackMetaData {
            added_by: interaction.user.id,
            added_at: Utc::now(),
        },
        extracted_metadata: None,
    };

    let mut collector = if add_to_top {
        queue
            .enqueue_top(interaction.user.id, enqueued_item)
            .await?
    } else {
        queue
            .enqueue(interaction.user.id, EnqueueType::Track(enqueued_item))
            .await?
    };

    if let Some(evt) = collector.recv().await {
        return match evt {
            QueueEnqueueEvent::TrackEnqueued(track, remaining_time) => {
                let user = interaction.user.tag();

                Ok(SubCommandReturnValue::EditEmbed(Box::new(move |e| {
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
                })))
            }

            QueueEnqueueEvent::TrackEnqueuedTop(track) => {
                let user = interaction.user.tag();

                Ok(SubCommandReturnValue::EditEmbed(Box::new(move |e| {
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
                })))
            }

            QueueEnqueueEvent::TrackEnqueuedBacklog(track) => {
                let user = interaction.user.tag();

                Ok(SubCommandReturnValue::EditEmbed(Box::new(move |e| {
                    e.author(|a| a.name("Queue Update"))
                        .title("Item added to queue!")
                        .fields([("Item", track, true)])
                        .footer(|f| f.text(format!("Added by {}", user)));

                    e
                })))
            }

            QueueEnqueueEvent::Error(e) => Ok(SubCommandReturnValue::Error(e)),

            _ => Ok(SubCommandReturnValue::EditInteraction(
                "I somehow received a playlist event despite queueing a song peko? pardun!?"
                    .to_string(),
            )),
        };
    }

    Ok(SubCommandReturnValue::DeleteInteraction)
}

#[instrument(skip(ctx, interaction, queue))]
async fn add_playlist(
    ctx: &Ctx,
    interaction: &ApplicationCommandInteraction,
    queue: Option<Queue>,
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
        .enqueue(
            interaction.user.id,
            EnqueueType::Playlist(EnqueuedItem {
                item: playlist_id.to_string(),
                metadata: TrackMetaData {
                    added_by: interaction.user.id,
                    added_at: Utc::now(),
                },
                extracted_metadata: None,
            }),
        )
        .await?;

    let mut playlist_processor_id = None;
    let mut playlist_length = 0;

    while let Some(evt) = collector.recv().await {
        match evt {
            QueueEnqueueEvent::TrackEnqueued(_track, _remaining_time) => {}

            QueueEnqueueEvent::PlaylistProcessingStart(playlist) => {
                playlist_length = playlist.video_count;

                let mut embed = CreateEmbed::default();
                embed
                    .author(|a| a.name("Playlist Processing"))
                    .title("Playlist found, starting processing...")
                    .footer(|f| f.text(format!("Added by {}", interaction.user.tag())))
                    .field("Name", playlist.title, true);

                if let Some(desc) = playlist.description {
                    embed.field("Description", desc, true);
                }

                embed.field("Uploader", playlist.uploader, true);

                let _ = interaction
                    .edit_original_interaction_response(ctx, |e| e.set_embeds(vec![embed]))
                    .await
                    .context(here!())?;

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

                let mut embed = CreateEmbed::default();
                embed
                    .author(|a| a.name("Queue Update"))
                    .title("Playlist entry loaded!")
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

                let _ = interaction
                    .edit_followup_message(ctx, followup_id, |e| e.embeds(vec![embed]))
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
                return Ok(SubCommandReturnValue::Error(e));
            }

            _ => continue,
        }
    }

    Ok(SubCommandReturnValue::DeleteInteraction)
}

#[instrument(skip(ctx, queue))]
async fn remove_from_queue(
    ctx: &Ctx,
    user_id: UserId,
    queue: Option<Queue>,
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

    let mut collector = queue.remove(user_id, removal_condition).await?;

    if let Some(evt) = collector.recv().await {
        return Ok(match evt {
            QueueRemovalEvent::TracksRemoved { count } => {
                SubCommandReturnValue::EditInteraction(format!("Removed {} songs!", count))
            }
            QueueRemovalEvent::DuplicatesRemoved { count } => {
                SubCommandReturnValue::EditInteraction(format!("Removed {} duplicates!", count))
            }
            QueueRemovalEvent::UserPurged { user_id, count } => {
                SubCommandReturnValue::EditInteraction(format!(
                    "Removed {} songs from {}!",
                    count,
                    user_id
                        .to_user(&ctx.http)
                        .await
                        .map(|u| u.tag())
                        .unwrap_or_else(|_| "unknown user".to_string())
                ))
            }
            QueueRemovalEvent::QueueCleared { count } => SubCommandReturnValue::EditInteraction(
                format!("Queue cleared! {} songs removed.", count),
            ),
            QueueRemovalEvent::Error(e) => SubCommandReturnValue::Error(e),
        });
    }

    Ok(SubCommandReturnValue::DeleteInteraction)
}

#[instrument(skip(queue))]
async fn shuffle_queue(
    user_id: UserId,
    queue: Option<Queue>,
) -> anyhow::Result<SubCommandReturnValue> {
    let queue = match queue {
        Some(q) => q,
        None => {
            return Ok(SubCommandReturnValue::EditInteraction(
                "I'm not in a voice channel peko.".to_string(),
            ));
        }
    };

    let mut collector = queue.shuffle(user_id).await?;

    match collector.recv().await {
        Some(QueueShuffleEvent::QueueShuffled) => {
            return Ok(SubCommandReturnValue::EditInteraction(
                "Queue shuffled!".to_string(),
            ));
        }
        Some(QueueShuffleEvent::Error(e)) => {
            return Ok(SubCommandReturnValue::Error(e));
        }
        None => (),
    }

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
