use std::{
    cell::RefCell,
    collections::{hash_map::Entry, HashMap},
    sync::Arc,
};

use anyhow::{anyhow, Context as _};
use chrono::{Datelike, Duration, NaiveDate, TimeZone, Utc};
use futures::future::BoxFuture;
use holodex::model::id::VideoId;
use macros::clone_variables;
// use music_queue::{MusicData, Queue};
use poise::{
    serenity_prelude::{
        AttachmentType, ChannelId, ExecuteWebhook, GatewayIntents, Mentionable, User, Webhook,
    },
    Context, Event, Framework, FrameworkContext,
};
use serenity::{
    client::Context as Ctx,
    model::{
        id::{EmojiId, StickerId},
        prelude::{Mention, ReactionType},
    },
};
// use songbird::SerenityInit;
use tokio::{
    select,
    sync::{broadcast, mpsc, oneshot, watch, Mutex, RwLock},
    task::JoinHandle,
};
use tracing::{debug, error, info};

use apis::meme_api::MemeApi;
use url::Url;
use utility::{
    config::{
        Config, ContentFilterAction, DatabaseHandle, EmojiStats,
        EmojiUsageSource, /* SavedMusicQueue */
    },
    discord::*,
    extensions::MessageExt,
    here,
    streams::*,
    types::Service,
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
    // pub music_data: Option<MusicData>,
    pub emoji_usage_counter:
        Option<mpsc::Sender<ResourceUsageEvent<EmojiId, EmojiUsageSource, EmojiStats>>>,
    pub sticker_usage_counter: Option<mpsc::Sender<ResourceUsageEvent<StickerId, (), u64>>>,

    pub guild_notifier: Mutex<RefCell<Option<oneshot::Sender<()>>>>,
    pub service_restarter: broadcast::Sender<Service>,

    pub webhook_cache: HashMap<ChannelId, Webhook>,
}

impl DiscordData {
    pub fn load(
        ctx: &Ctx,
        config: &Config,
        stream_index: Option<watch::Receiver<HashMap<VideoId, Livestream>>>,
        stream_updates: broadcast::Sender<StreamUpdate>,
        guild_notifier: oneshot::Sender<()>,
        service_restarter: broadcast::Sender<Service>,
    ) -> anyhow::Result<Self> {
        let database = config.database.get_handle()?;

        let (stream_index, stream_updates) = if config.stream_tracking.enabled {
            (stream_index, Some(stream_updates))
        } else {
            (None, None)
        };

        let meme_creator = config
            .meme_creation
            .enabled
            .then(|| MemeApi::new(&config.meme_creation))
            .transpose()?;

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
            // music_data: None,
            stream_index,
            stream_updates,

            emoji_usage_counter,
            sticker_usage_counter,

            guild_notifier: Mutex::new(RefCell::new(Some(guild_notifier))),
            service_restarter,

            webhook_cache: HashMap::new(),
        })
    }
}

pub struct DiscordBot;

impl DiscordBot {
    pub async fn start(
        config: Arc<Config>,
        stream_update: broadcast::Sender<StreamUpdate>,
        index_receiver: Option<watch::Receiver<HashMap<VideoId, Livestream>>>,
        guild_ready: oneshot::Sender<()>,
        service_restarter: broadcast::Sender<Service>,
    ) -> anyhow::Result<(JoinHandle<()>, Ctx)> {
        let (ctx_tx, ctx_rx) = oneshot::channel();

        let client_builder = poise::Framework::builder()
            .token(&config.discord_token)
            .initialize_owners(true)
            .setup(move |ctx, _ready, _fw| {
                Box::pin(async move {
                    ctx_tx.send(ctx.clone()).map_err(|_| ()).unwrap();

                    let discord_data = DiscordData::load(
                        ctx,
                        &config,
                        index_receiver,
                        stream_update,
                        guild_ready,
                        service_restarter,
                    )?;

                    Ok(DataWrapper {
                        config: Arc::clone(&config),
                        data: RwLock::new(discord_data),
                    })
                })
            })
            .intents(
                GatewayIntents::GUILDS
                    | GatewayIntents::GUILD_EMOJIS_AND_STICKERS
                    | GatewayIntents::GUILD_MESSAGES
                    | GatewayIntents::GUILD_MESSAGE_REACTIONS
                    | GatewayIntents::GUILD_VOICE_STATES
                    | GatewayIntents::MESSAGE_CONTENT,
            )
            // .client_settings(|c| c.register_songbird())
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
                event_handler: Self::handle_discord_event,
                on_error: |error| Box::pin(Self::on_error(error)),
                command_check: Some(Self::should_fail),
                commands: cmds::get_commands(),
                ..Default::default()
            });

        let client = client_builder.build().await?;

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
        framework: FrameworkContext<'a, DataWrapper, anyhow::Error>,
        data: &'a DataWrapper,
    ) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async move {
            match event {
                Event::CacheReady { guilds } => {
                    info!("Cache ready. Guild count: {}", guilds.len());

                    for guild_id in guilds {
                        debug!(
                            "Guild ready: {}",
                            guild_id.name(ctx).unwrap_or_else(|| "<unknown>".to_owned())
                        );
                    }
                }

                Event::GuildCreate {
                    guild,
                    is_new: _is_new,
                } => {
                    if data.config.blocked.servers.contains(&guild.id) {
                        return Ok(());
                    }

                    info!(name = %guild.name, "Guild initialized!");

                    let commands_builder =
                        poise::builtins::create_application_commands(&framework.options().commands);

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

                    /* if data.config.music_bot.enabled {
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
                    } */
                }
                Event::Message { new_message: msg } => {
                    if msg.author.bot {
                        return Ok(());
                    }

                    let is_april_fools = {
                        let now = Utc::now();

                        let start = NaiveDate::from_ymd_opt(now.date_naive().year(), 4, 1)
                            .unwrap()
                            .and_hms_opt(0, 0, 0)
                            .unwrap();

                        let start = Utc.from_local_datetime(&start).unwrap();
                        let earlier_start = start - Duration::hours(4);

                        let end = start + Duration::days(1);

                        now >= earlier_start && now <= end
                    };

                    if is_april_fools || msg.channel_id == ChannelId(824333250104787004) {
                        let Some(webhook) = Self::get_channel_webhook(ctx, data, &msg.author, msg.channel_id).await else {
                            return Ok(())
                        };

                        let has_links = Url::parse(msg.content.trim()).is_ok();

                        enum Prank {
                            Pekofy,
                            Uwuify,
                            None,
                        }

                        let prank = match msg.author.id.0 % 2 {
                            _ if has_links => Prank::None,
                            0 => Prank::Pekofy,
                            1 => Prank::Uwuify,
                            _ => unreachable!(),
                        };

                        let username = msg
                            .author_nick(&ctx)
                            .await
                            .unwrap_or(msg.author.name.clone());

                        let mut webhook_builder = ExecuteWebhook::default();
                        webhook_builder.username(username);

                        if let Some(avatar) = msg.author.avatar_url() {
                            webhook_builder.avatar_url(avatar);
                        }

                        /* webhook_builder.add_files(
                            msg.attachments
                                .iter()
                                .filter_map(|a| Url::parse(&a.url).ok())
                                .map(AttachmentType::Image),
                        );

                        let mention = if let Some(replied_to) = &msg.referenced_message {
                            webhook_builder.allowed_mentions(|m| {
                                m.users(msg.mentions.iter().chain(Some(&replied_to.author)))
                            });

                            format!("{} ", replied_to.author.mention())
                        } else {
                            webhook_builder.allowed_mentions(|m| m.users(&msg.mentions));
                            String::new()
                        }; */

                        let message = match prank {
                            Prank::Pekofy => {
                                match super::commands::pekofy::pekofy_text(&msg.content) {
                                    Ok(text) => text,
                                    Err(e) => {
                                        error!(err = ?e, "Failed to pekofy text!");
                                        msg.content.clone()
                                    }
                                }
                            }
                            Prank::Uwuify => {
                                match super::commands::uwuify::uwuify_str(&msg.content) {
                                    Some(text) => text,
                                    None => msg.content.clone(),
                                }
                            }
                            Prank::None => msg.content.clone(),
                        };

                        if
                        /* !mention.is_empty() || */
                        !message.is_empty() {
                            webhook_builder
                                .content(/* format!("{mention}{message}") */ message);
                        } else {
                            webhook_builder.content("placeholder");
                        }

                        match webhook.execute(&ctx, true, |m| m).await {
                            Ok(Some(_msg)) => {
                                // Message succeeded to be sent.
                                if let Err(e) = msg.delete(&ctx).await {
                                    error!(err = ?e, "Failed to delete original message!");
                                }
                            }
                            Ok(None) => {
                                error!("Webhook message was sent but wasn't processed properly!");
                            }
                            Err(e) => {
                                error!(err = ?e, "Failed to send webhook message!");
                            }
                        }

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

    async fn on_error(error: poise::FrameworkError<'_, DataWrapper, anyhow::Error>) {
        // This is our custom error handler
        // They are many errors that can occur, so we only handle the ones we want to customize
        // and forward the rest to the default handler
        match error {
            poise::FrameworkError::Setup { error, .. } => panic!("Failed to start bot: {error:?}"),
            poise::FrameworkError::Command { error, ctx } => {
                error!(command = %ctx.command().name, "Command error: {:?}", error,);
            }
            error => {
                if let Err(e) = poise::builtins::on_error(error).await {
                    error!("Error while handling error: {}", e)
                }
            }
        }
    }

    async fn get_channel_webhook(
        ctx: &Ctx,
        data: &DataWrapper,
        user: &User,
        channel: ChannelId,
    ) -> Option<Webhook> {
        match data.data.write().await.webhook_cache.entry(channel) {
            Entry::Occupied(slot) => Some(slot.get().clone()),
            Entry::Vacant(slot) => {
                if let Ok(existing_hooks) = channel.webhooks(&ctx).await {
                    for hook in existing_hooks {
                        if let Err(e) = hook.delete(&ctx).await {
                            error!(err = ?e, "Failed to delete existing webhook!");
                        }
                    }
                }

                let webhook = match channel.create_webhook(&ctx, &user.name).await {
                    Ok(hook) => hook,
                    Err(e) => {
                        error!(err = ?e, "Failed to create webhook!");
                        return None;
                    }
                };

                slot.insert(webhook.clone());
                Some(webhook)
            }
        }
    }

    async fn save_client_data(
        client: Arc<Framework<DataWrapper, anyhow::Error>>,
    ) -> anyhow::Result<()> {
        let user_data = client.user_data().await;
        let _connection = user_data.config.database.get_handle()?;

        let data = user_data.data.read().await;

        if let Some(s) = &data.emoji_usage_counter {
            if let Err(e) = s.send(EmojiUsageEvent::Terminate).await {
                error!(?e, "Saving error!");
            }
        }

        if let Some(s) = &data.sticker_usage_counter {
            if let Err(e) = s.send(StickerUsageEvent::Terminate).await {
                error!(?e, "Saving error!");
            }
        }

        /* if let Some(s) = &data.music_data {
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
        } */

        Ok(())
    }
}
