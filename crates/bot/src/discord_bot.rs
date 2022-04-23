use std::{cell::RefCell, collections::HashMap, sync::Arc};

use anyhow::{anyhow, Context as _};
use futures::future::BoxFuture;
use holodex::model::id::VideoId;
use macros::clone_variables;
use music_queue::{MusicData, Queue};
use poise::{serenity_prelude::GatewayIntents, Context, Event, Framework};
use serenity::{
    client::Context as Ctx,
    model::{
        id::{EmojiId, GuildId, StickerId, UserId},
        prelude::{Mention, ReactionType},
    },
};
use songbird::SerenityInit;
use tokio::{
    select,
    sync::{broadcast, mpsc, oneshot, watch, Mutex, RwLock},
    task::JoinHandle,
};
use tracing::{debug, error, info};

use apis::meme_api::MemeApi;
use utility::{
    config::{
        Config, ContentFilterAction, DatabaseHandle, DatabaseOperations, EmojiStats,
        EmojiUsageSource, EntryEvent, Quote, Reminder, SavedMusicQueue,
    },
    discord::*,
    extensions::MessageExt,
    here,
    streams::*,
};

use crate::{commands as cmds, resource_tracking, temp_mute_react};

pub struct DataWrapper {
    pub config: Arc<Config>,
    pub data: RwLock<DiscordData>,
}

pub struct DiscordData {
    pub database: Mutex<DatabaseHandle>,

    pub stream_index: Option<watch::Receiver<HashMap<VideoId, Livestream>>>,
    pub stream_updates: Option<broadcast::Sender<StreamUpdate>>,

    pub meme_creator: Option<MemeApi>,
    pub reminder_sender: Option<mpsc::Sender<EntryEvent<u32, Reminder>>>,
    pub quotes: Option<Vec<Quote>>,
    pub music_data: Option<MusicData>,

    pub emoji_usage_counter:
        Option<mpsc::Sender<ResourceUsageEvent<EmojiId, EmojiUsageSource, EmojiStats>>>,
    pub sticker_usage_counter: Option<mpsc::Sender<ResourceUsageEvent<StickerId, (), u64>>>,

    pub guild_notifier: Mutex<RefCell<Option<oneshot::Sender<()>>>>,
}

impl DiscordData {
    pub fn load(
        ctx: &Ctx,
        config: &Config,
        stream_index: Option<watch::Receiver<HashMap<VideoId, Livestream>>>,
        stream_updates: broadcast::Sender<StreamUpdate>,
        reminder_sender: mpsc::Sender<EntryEvent<u32, Reminder>>,
        guild_notifier: oneshot::Sender<()>,
    ) -> anyhow::Result<Self> {
        let database = config.database.get_handle()?;

        let (stream_index, stream_updates) = if config.stream_tracking.enabled {
            (stream_index, Some(stream_updates))
        } else {
            (None, None)
        };

        let quotes = if config.quotes.enabled {
            Vec::<Quote>::create_table(&database)?;

            Some(Vec::<Quote>::load_from_database(&database)?)
        } else {
            None
        };

        let meme_creator = config
            .meme_creation
            .enabled
            .then(|| MemeApi::new(&config.meme_creation))
            .transpose()?;

        let reminder_sender = config.reminders.enabled.then(|| reminder_sender);

        let (emoji_usage_counter, sticker_usage_counter) = if config.emoji_tracking.enabled {
            let (emoji_usage_counter, emoji_usage_recv) = mpsc::channel(64);
            let (sticker_usage_counter, sticker_usage_recv) = mpsc::channel(64);

            let database = &config.database;

            tokio::spawn(clone_variables!(database; {
                if let Err(e) = resource_tracking::emoji_tracker(&database, emoji_usage_recv).await.context(here!()) {
                    error!("{:?}", e);
                }
            }));

            tokio::spawn(clone_variables!(database; {
                if let Err(e) = resource_tracking::sticker_tracker(&database, sticker_usage_recv).await.context(here!()) {
                    error!("{:?}", e);
                }
            }));

            (Some(emoji_usage_counter), Some(sticker_usage_counter))
        } else {
            (None, None)
        };

        if config.react_temp_mute.enabled {
            let ctx = ctx.clone();

            tokio::spawn(clone_variables!(config; {
                if let Err(e) = temp_mute_react::handler(ctx, &config.react_temp_mute).await.context(here!()) {
                    error!("{:?}", e);
                }
            }));
        }

        Ok(Self {
            database: Mutex::new(database),

            meme_creator,
            reminder_sender,
            quotes,
            music_data: None,

            stream_index,
            stream_updates,

            emoji_usage_counter,
            sticker_usage_counter,

            guild_notifier: Mutex::new(RefCell::new(Some(guild_notifier))),
        })
    }
}

pub struct DiscordBot;

impl DiscordBot {
    pub async fn start(
        config: Arc<Config>,
        stream_update: broadcast::Sender<StreamUpdate>,
        reminder_sender: mpsc::Sender<EntryEvent<u32, Reminder>>,
        index_receiver: Option<watch::Receiver<HashMap<VideoId, Livestream>>>,
        guild_ready: oneshot::Sender<()>,
    ) -> anyhow::Result<(JoinHandle<()>, Ctx)> {
        let owner = UserId(113_654_526_589_796_356);

        let (ctx_tx, ctx_rx) = oneshot::channel();

        let client = poise::Framework::build()
            .token(&config.discord_token)
            .user_data_setup(move |ctx, _ready, _fw| {
                Box::pin(async move {
                    ctx_tx.send(ctx.clone()).map_err(|_| ()).unwrap();

                    let discord_data = DiscordData::load(
                        ctx,
                        &config,
                        index_receiver,
                        stream_update,
                        reminder_sender,
                        guild_ready,
                    )?;

                    Ok(DataWrapper {
                        config: Arc::clone(&config),
                        data: RwLock::new(discord_data),
                    })
                })
            })
            .client_settings(|c| {
                c.register_songbird()
                    .application_id(812833473370390578u64)
                    .intents(
                        GatewayIntents::GUILDS
                            | GatewayIntents::GUILD_EMOJIS_AND_STICKERS
                            | GatewayIntents::GUILD_MESSAGES
                            | GatewayIntents::GUILD_MESSAGE_REACTIONS
                            | GatewayIntents::GUILD_VOICE_STATES
                            | GatewayIntents::MESSAGE_CONTENT,
                    )
            })
            .options(poise::FrameworkOptions {
                prefix_options: poise::PrefixFrameworkOptions {
                    prefix: Some("-".into()),
                    case_insensitive_commands: true,
                    edit_tracker: Some(poise::EditTracker::for_timespan(
                        std::time::Duration::from_secs(3600),
                    )),
                    mention_as_prefix: true,
                    ..Default::default()
                },
                owners: vec![owner].into_iter().collect(),
                listener: Self::handle_discord_event,
                command_check: Some(Self::should_fail),
                commands: vec![
                    cmds::birthdays(),
                    poise::Command {
                        subcommands: vec![cmds::config::remove_command()],
                        ..cmds::config()
                    },
                    cmds::donate(),
                    cmds::eightball(),
                    cmds::emoji_usage(),
                    cmds::help(),
                    cmds::live(),
                    cmds::meme(),
                    cmds::move_conversation(),
                    poise::Command {
                        subcommands: vec![
                            cmds::music::join(),
                            cmds::music::leave(),
                            cmds::music::volume(),
                            cmds::music::play_now(),
                            cmds::music::pause(),
                            cmds::music::resume(),
                            cmds::music::loop_song(),
                            cmds::music::skip(),
                            cmds::music::now_playing(),
                            cmds::music::queue(),
                            cmds::music::add_song(),
                            cmds::music::add_to_top(),
                            cmds::music::add_playlist(),
                            cmds::music::remove(),
                            cmds::music::remove_dupes(),
                            cmds::music::clear(),
                            cmds::music::shuffle(),
                        ],
                        ..cmds::music()
                    },
                    cmds::ogey(),
                    cmds::pekofy(),
                    cmds::pekofy_message(),
                    cmds::sticker_usage(),
                    cmds::timestamp(),
                    cmds::tsfmt(),
                    cmds::upcoming(),
                    cmds::uwuify(),
                    cmds::uwuify_message(),
                ],
                ..Default::default()
            })
            .build()
            .await?;

        let task = tokio::spawn(async move {
            let client_clone = Arc::clone(&client);

            let status = select! {
                e = client.start() => {
                    e.context(here!())
                }
                e = tokio::signal::ctrl_c() => {
                    e.context(here!())
                }
            };

            if let Err(e) = Self::save_client_data(client_clone).await {
                error!("{:?}", e);
            }

            if let Err(e) = status {
                error!("{:?}", e);
            }

            info!(task = "Discord bot", "Shutting down.");
        });

        let cache = ctx_rx.await.context(here!())?;

        Ok((task, cache))
    }

    fn should_fail(
        ctx: Context<'_, DataWrapper, anyhow::Error>,
    ) -> BoxFuture<'_, anyhow::Result<bool>> {
        Box::pin(async move {
            let config = &ctx.data().config;

            if config.blocked.users.contains(&ctx.author().id) {
                return Ok(false);
            }

            if config.blocked.servers.contains(&ctx.guild_id().unwrap()) {
                return Ok(false);
            }

            if config.blocked.channels.contains(&ctx.channel_id()) {
                return Ok(false);
            }

            Ok(true)
        })
    }

    fn handle_discord_event<'a>(
        ctx: &'a Ctx,
        event: &'a Event<'_>,
        framework: &'a Framework<DataWrapper, anyhow::Error>,
        data: &'a DataWrapper,
    ) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async move {
            match event {
                Event::GuildCreate {
                    guild,
                    is_new: _is_new,
                } => {
                    if data.config.blocked.servers.contains(&guild.id) {
                        return Ok(());
                    }

                    info!(name = %guild.name, "Guild initialized!");

                    let mut commands_builder =
                        poise::serenity_prelude::CreateApplicationCommands::default();
                    let commands = &framework.options().commands;

                    for command in commands {
                        if let Some(slash_command) = command.create_as_slash_command() {
                            commands_builder.add_application_command(slash_command);
                        }
                        if let Some(context_menu_command) = command.create_as_context_menu_command()
                        {
                            commands_builder.add_application_command(context_menu_command);
                        }
                    }

                    let commands_builder = serenity::json::Value::Array(commands_builder.0);

                    ctx.http
                        .create_guild_application_commands(guild.id.0, &commands_builder)
                        .await?;

                    {
                        let read_lock = data.data.read().await;
                        let sender_lock = read_lock.guild_notifier.lock().await;
                        let sender = sender_lock.replace(None);

                        if let Some(sender) = sender {
                            sender
                                .send(())
                                .map_err(|_| anyhow!("Failed to send notification!"))
                        } else {
                            Err(anyhow!("Failed to get notification sender!"))
                        }?;
                    }

                    if data.config.music_bot.enabled {
                        let db_handle = match data.config.database.get_handle() {
                            Ok(h) => h,
                            Err(e) => {
                                return Err(anyhow!("Failed to get database handle! {e:?}"));
                            }
                        };

                        let mut music_data = MusicData::default();

                        if let Err(e) =
                            HashMap::<GuildId, SavedMusicQueue>::create_table(&db_handle)
                        {
                            return Err(anyhow!("Failed to create table: {e:?}"));
                        }

                        let queues = match HashMap::<GuildId, SavedMusicQueue>::load_from_database(
                            &db_handle,
                        ) {
                            Ok(q) => q,
                            Err(e) => {
                                return Err(anyhow!("Failed to load music queues! {e:?}"));
                            }
                        };

                        for (guild_id, queue) in queues {
                            if guild_id != guild.id {
                                continue;
                            }

                            let manager = songbird::serenity::get(ctx).await.unwrap().clone();

                            match manager.join(guild.id, queue.channel_id).await {
                                (_, Ok(())) => debug!("Joined voice channel!"),
                                (_, Err(e)) => {
                                    error!("{:?}", e);
                                    continue;
                                }
                            }

                            let queue = Queue::load(
                                manager,
                                &guild.id,
                                ctx.http.clone(),
                                ctx.cache.clone(),
                                queue.state,
                                &queue.tracks,
                            );

                            music_data.insert(guild.id, queue);
                        }

                        {
                            let mut write_lock = data.data.write().await;
                            write_lock.music_data = Some(music_data);
                        }
                    }
                }
                Event::Message { new_message: msg } => {
                    if msg.author.bot {
                        return Ok(());
                    }

                    if data.config.content_filtering.enabled {
                        let filter_config = &data.config.content_filtering;
                        let filter_actions = filter_config.filter(msg).into_actions();

                        for action in filter_actions {
                            match action {
                                ContentFilterAction::DeleteMsg => {
                                    if let Err(e) = msg.delete(&ctx.http).await {
                                        error!(err = %e, "Failed to delete message.");
                                    }
                                }

                                ContentFilterAction::Log(embed) => {
                                    if let Err(e) = msg
                                        .channel_id
                                        .send_message(&ctx.http, |m| m.set_embed(embed))
                                        .await
                                    {
                                        error!(err = %e, "Failed to log action.");
                                    }
                                }

                                ContentFilterAction::LogStaff(embed) => {
                                    if let Err(e) = filter_config
                                        .logging_channel
                                        .send_message(&ctx.http, |m| m.set_embed(embed))
                                        .await
                                    {
                                        error!(err = %e, "Failed to log action.");
                                    }
                                }

                                ContentFilterAction::LogStaffNotify(embed) => {
                                    if let Err(e) = filter_config
                                        .logging_channel
                                        .send_message(&ctx.http, |m| {
                                            m.content(
                                                &filter_config
                                                    .staff_role
                                                    .map(|r| Mention::from(r).to_string())
                                                    .unwrap_or_else(|| "@here".to_owned()),
                                            )
                                            .set_embed(embed)
                                        })
                                        .await
                                    {
                                        error!(err = %e, "Failed to log action.");
                                    }
                                }

                                ContentFilterAction::Mute(_duration) => {}

                                ContentFilterAction::Ban(reason) => {
                                    if let Some(guild_id) = &msg.guild_id {
                                        if let Err(e) = guild_id
                                            .ban_with_reason(&ctx.http, msg.author.id, 2, &reason)
                                            .await
                                        {
                                            error!(err = %e, "Failed to ban user.");
                                        }
                                    }
                                }
                            }
                        }
                    }

                    if data.config.emoji_tracking.enabled {
                        // Send emoji tracking update.
                        let read_lock = data.data.read().await;
                        let emoji_usage = &read_lock.emoji_usage_counter.as_ref().unwrap();

                        if let Err(e) = emoji_usage
                            .send(EmojiUsageEvent::Used {
                                resources: msg.get_emojis(),
                                usage: EmojiUsageSource::InText,
                            })
                            .await
                            .context(here!())
                        {
                            error!(?e, "Failed to update emoji usage!");
                        }

                        // Send sticker tracking update.
                        let sticker_usage = read_lock.sticker_usage_counter.as_ref().unwrap();

                        if let Err(e) = sticker_usage
                            .send(StickerUsageEvent::Used {
                                resources: msg.sticker_items.iter().map(|s| s.id).collect(),
                                usage: (),
                            })
                            .await
                            .context(here!())
                        {
                            error!(?e, "Failed to update sticker usage!");
                        }
                    }

                    if data.config.embed_compressor.enabled {}
                }
                Event::ReactionAdd { add_reaction } => {
                    if data.config.emoji_tracking.enabled {
                        if let ReactionType::Custom {
                            animated: _,
                            id,
                            name: _,
                        } = &add_reaction.emoji
                        {
                            // Send emoji tracking update.
                            let read_lock = data.data.read().await;
                            let emoji_usage = read_lock.emoji_usage_counter.as_ref().unwrap();

                            if let Err(e) = emoji_usage
                                .send(EmojiUsageEvent::Used {
                                    resources: vec![*id],
                                    usage: EmojiUsageSource::AsReaction,
                                })
                                .await
                                .context(here!())
                            {
                                return Err(anyhow!("Failed to update emoji usage: {e:?}"));
                            }
                        }
                    }
                }

                _ => (),
            }

            Ok(())
        })
    }

    async fn save_client_data(
        client: Arc<Framework<DataWrapper, anyhow::Error>>,
    ) -> anyhow::Result<()> {
        let user_data = client.user_data().await;
        let connection = user_data.config.database.get_handle()?;

        let data = user_data.data.read().await;

        if let Some(s) = &data.emoji_usage_counter {
            if let Err(e) = s.send(EmojiUsageEvent::Terminate).await {
                error!(?e, "Saving error!");
            }
        }

        if let Some(s) = &data.music_data {
            let mut queues = HashMap::with_capacity(s.0.len());

            for (&guild_id, queue) in s.0.iter() {
                if let Some((ch, state, tracks)) = queue.save_and_exit().await {
                    queues.insert(
                        guild_id,
                        SavedMusicQueue {
                            channel_id: ch,
                            state,
                            tracks,
                        },
                    );
                }
            }

            if let Err(e) = queues.save_to_database(&connection) {
                error!(?e, "Saving error!");
            }
        }

        if let Some(quotes) = data.quotes.clone() {
            if let Err(e) = quotes.save_to_database(&connection) {
                error!(?e, "Saving error!");
            }
        }

        Ok(())
    }
}
