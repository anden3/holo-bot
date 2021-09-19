use std::{cell::RefCell, collections::HashMap, sync::Arc};

use anyhow::{anyhow, Context};
use commands::prelude::ApplicationCommandInteraction;
use once_cell::sync::OnceCell;
use serenity::{
    framework::{
        standard::{macros::hook, Configuration, DispatchError},
        StandardFramework,
    },
    model::{interactions::Interaction, prelude::*},
    prelude::*,
    CacheAndHttp, Client,
};
use songbird::{SerenityInit, SongbirdKey};
use tokio::{
    select,
    sync::{broadcast, mpsc, oneshot, watch, RwLockReadGuard},
    task::JoinHandle,
};
use tracing::{debug, error, info, instrument, warn};

use apis::meme_api::MemeApi;
use utility::{
    config::{Config, EmojiStats, EntryEvent, LoadFromDatabase, Reminder, SaveToDatabase},
    discord::*,
    extensions::MessageExt,
    here, setup_interaction_groups,
    streams::*,
};

type Ctx = serenity::prelude::Context;

static CONFIGURATION: OnceCell<Configuration> = OnceCell::new();
static EMOJI_CACHE: OnceCell<Arc<RwLock<HashMap<EmojiId, EmojiStats>>>> = OnceCell::new();

pub struct DiscordBot;

impl DiscordBot {
    #[instrument(skip(config, exit_receiver))]
    pub async fn start(
        config: Config,
        stream_update: broadcast::Sender<StreamUpdate>,
        reminder_sender: mpsc::Receiver<EntryEvent<u64, Reminder>>,
        index_receiver: watch::Receiver<HashMap<String, Livestream>>,
        guild_ready: oneshot::Sender<()>,
        exit_receiver: watch::Receiver<bool>,
    ) -> anyhow::Result<(JoinHandle<()>, Arc<CacheAndHttp>)> {
        let owner = UserId(113_654_526_589_796_356);

        let mut conf = Configuration::default();
        conf.owners.insert(owner);

        if CONFIGURATION.set(conf).is_err() {
            return Err(anyhow!("Couldn't save static framework configurations!"));
        }

        let framework = StandardFramework::new()
            .help(&commands::HELP_CMD)
            .configure(|c| {
                c.prefixes(vec!["草", "-"])
                    .owners(vec![owner].into_iter().collect())
                    .blocked_guilds(config.blocked_servers.iter().map(|i| GuildId(*i)).collect())
            })
            .group(&commands::FUN_GROUP)
            .group(&commands::UTILITY_GROUP);

        let handler = Handler {
            config: config.clone(),
            guild_notifier: Mutex::new(RefCell::new(Some(guild_ready))),
        };

        let client = Client::builder(&config.discord_token)
            .framework(framework)
            .event_handler(handler)
            .register_songbird()
            .application_id(812833473370390578u64)
            .await
            .context(here!())?;

        let cache = Arc::<CacheAndHttp>::clone(&client.cache_and_http);

        let task = tokio::spawn(async move {
            match Self::run(
                client,
                config,
                stream_update,
                reminder_sender,
                index_receiver,
                exit_receiver,
            )
            .await
            {
                Ok(()) => (),
                Err(e) => {
                    error!("{:?}", e);
                }
            }

            info!(task = "Discord bot", "Shutting down.");
        });

        Ok((task, cache))
    }

    #[instrument(skip(client, config, stream_update, index_receiver, exit_receiver))]
    async fn run(
        mut client: Client,
        config: Config,
        stream_update: broadcast::Sender<StreamUpdate>,
        reminder_sender: mpsc::Receiver<EntryEvent<u64, Reminder>>,
        index_receiver: watch::Receiver<HashMap<String, Livestream>>,
        mut exit_receiver: watch::Receiver<bool>,
    ) -> anyhow::Result<()> {
        {
            let mut data = client.data.write().await;

            let db_handle = config.get_database_handle()?;

            data.insert::<MemeApi>(MemeApi::new(&config)?);
            data.insert::<Quotes>(Quotes::load_from_database(&db_handle)?.into());
            data.insert::<EmojiUsage>(EmojiUsage::load_from_database(&db_handle)?.into());

            data.insert::<DbHandle>(DbHandle(Mutex::new(db_handle)));
            data.insert::<RegisteredInteractions>(RegisteredInteractions::default());

            data.insert::<StreamIndex>(StreamIndex(index_receiver));

            let (message_send, message_recv) = broadcast::channel::<MessageUpdate>(64);
            std::mem::drop(message_recv);

            data.insert::<MessageSender>(MessageSender(message_send));
            data.insert::<ReminderSender>(ReminderSender(reminder_sender));
            data.insert::<StreamUpdateTx>(StreamUpdateTx(stream_update));

            data.insert::<MusicData>(MusicData::default());
        }

        select! {
            e = client.start() => {
                e.context(here!())
            }
            e = exit_receiver.changed() => {
                let data = client.data.read().await;

                let result = tokio::try_join!(
                    Self::save_data(&data),
                    Self::disconnect_music(&data),
                );

                if let Err(err) = result {
                    error!(%err, "Saving error!");
                }

                e.context(here!())
            }
        }
    }

    async fn save_data(data: &RwLockReadGuard<'_, TypeMap>) -> anyhow::Result<()> {
        let connection = data.get::<DbHandle>().unwrap().lock().await;

        data.get::<EmojiUsage>()
            .and_then(|d| d.save_to_database(&connection).ok());

        data.get::<Quotes>()
            .and_then(|d| d.save_to_database(&connection).ok());

        Ok(())
    }

    async fn disconnect_music(data: &RwLockReadGuard<'_, TypeMap>) -> anyhow::Result<()> {
        let manager = data
            .get::<SongbirdKey>()
            .ok_or_else(|| anyhow!("Songbird manager not available."))?;

        if let Some(music_data) = data.get::<MusicData>() {
            for id in music_data.queues.keys() {
                music_data.stop(id)?;
                manager.remove(*id).await.context(here!())?;
            }
        }

        Ok(())
    }
}

#[hook]
#[allow(clippy::wildcard_enum_match_arm)]
#[instrument(skip(ctx))]
async fn dispatch_error_hook(ctx: &Ctx, msg: &Message, error: DispatchError) {
    match error {
        DispatchError::NotEnoughArguments { min, given } => {
            let res = msg
                .channel_id
                .say(
                    &ctx,
                    &format!("Need {} arguments, but only got {}.", min, given),
                )
                .await;

            if let Err(e) = res {
                error!("{:?}", e);
            }
        }
        DispatchError::TooManyArguments { max, given } => {
            let res = msg
                .channel_id
                .say(
                    &ctx,
                    &format!("Max arguments allowed is {}, but got {}.", max, given),
                )
                .await;

            if let Err(e) = res {
                error!("{:?}", e);
            }
        }
        DispatchError::CheckFailed(..)
        | DispatchError::Ratelimited(..)
        | DispatchError::CommandDisabled(..)
        | DispatchError::BlockedUser
        | DispatchError::BlockedGuild
        | DispatchError::BlockedChannel
        | DispatchError::OnlyForDM
        | DispatchError::OnlyForGuilds
        | DispatchError::OnlyForOwners
        | DispatchError::LackingRole
        | DispatchError::LackingPermissions(..) => error!("Unhandled dispatch error."),

        _ => error!("Unknown dispatch error!"),
    }
}

#[derive(Debug)]
struct Handler {
    config: Config,
    guild_notifier: Mutex<RefCell<Option<oneshot::Sender<()>>>>,
}

impl Handler {
    #[instrument(skip(ctx))]
    async fn interaction_requested(
        &self,
        ctx: Ctx,
        request: ApplicationCommandInteraction,
    ) -> anyhow::Result<()> {
        let request_data = &request.data;

        let data = ctx.data.read().await;

        let interaction = data
            .get::<RegisteredInteractions>()
            .unwrap()
            .get(&request.guild_id.unwrap())
            .and_then(|h| h.get(&request_data.id));

        let interaction = match interaction {
            Some(i) => i,
            None => {
                anyhow::bail!("Unknown interaction found: '{}'", request_data.name);
            }
        };

        match interaction.check_rate_limit(&ctx, &request).await {
            Ok(false) => anyhow::bail!("Rate limit hit!"),
            Err(err) => {
                anyhow::bail!(err);
            }
            _ => (),
        }

        let conf = CONFIGURATION.get().unwrap();

        match commands::util::should_fail(conf, &ctx, &request, interaction).await {
            Some(err) => {
                debug!("{:?}", err);
                return Ok(());
            }
            None => {
                let func = interaction.func;
                std::mem::drop(data);

                let config = self.config.clone();

                tokio::spawn(async move {
                    if let Err(err) = (func)(&ctx, &request, &config).await {
                        error!("{:?}", err);
                        return;
                    }
                });
            }
        }

        Ok(())
    }

    async fn update_emoji_usage(ctx: &Ctx, emoji: Vec<EmojiId>) -> anyhow::Result<()> {
        if !emoji.is_empty() {
            if let Ok(mut data) = ctx.data.try_write() {
                let emoji_usage = data.get_mut::<EmojiUsage>().unwrap();

                for id in emoji {
                    let count = emoji_usage.entry(id).or_insert_with(EmojiStats::default);
                    (*count).text_count += 1;
                }

                if let Some(cache) = EMOJI_CACHE.get() {
                    let mut cache = cache.write().await;

                    if !cache.is_empty() {
                        for (id, count) in cache.iter() {
                            let c = emoji_usage.entry(*id).or_insert_with(EmojiStats::default);
                            *c += *count;
                        }

                        cache.clear();
                    }
                }
            } else {
                let mut cache = EMOJI_CACHE
                    .get_or_init(|| Arc::new(RwLock::new(HashMap::new())))
                    .write()
                    .await;

                for id in emoji {
                    let count = cache.entry(id).or_insert_with(EmojiStats::default);
                    (*count).text_count += 1;
                }
            }
        }

        Ok(())
    }
}

#[serenity::async_trait]
impl EventHandler for Handler {
    #[instrument(skip(self, ctx, guild))]
    async fn guild_create(&self, ctx: Ctx, guild: Guild, _is_new: bool) {
        if self.config.blocked_servers.contains(guild.id.as_u64()) {
            return;
        }

        info!(name = %guild.name, "Guild initialized!");

        let token = self.config.discord_token.clone();

        // Upload interactions to Discord.
        let app_id = *ctx.cache.current_user_id().await.as_u64();

        let mut commands = setup_interaction_groups!(guild, [Fun, Utility]);

        if let Err(e) = RegisteredInteraction::register(&mut commands, &token, app_id, &guild).await
        {
            error!("{}", e);
            return;
        }

        let commands = commands
            .into_iter()
            .map(|r| (r.command.as_ref().unwrap().id, r))
            .collect::<HashMap<_, _>>();

        let mut data = ctx.data.write().await;

        let command_map = data.get_mut::<RegisteredInteractions>().unwrap();
        command_map.insert(guild.id, commands);

        let sender_lock = self.guild_notifier.lock().await;
        let sender = sender_lock.replace(None);

        if let Some(sender) = sender {
            if sender.send(()).is_err() {
                error!("Failed to send notification!");
            }
        } else {
            error!("Failed to get notification sender!");
        }
    }

    #[instrument(skip(self, ctx))]
    #[allow(unreachable_patterns)]
    async fn interaction_create(&self, ctx: Ctx, request: Interaction) {
        match request {
            Interaction::Ping(_ping) => (),
            Interaction::MessageComponent(_cmp) => (),

            Interaction::ApplicationCommand(cmd) => {
                if cmd.guild_id.is_none() {
                    return;
                }

                if self
                    .config
                    .blocked_servers
                    .contains(cmd.guild_id.unwrap().as_u64())
                {
                    return;
                }

                if let Err(e) = self.interaction_requested(ctx, cmd).await {
                    warn!(err = %e, "Interaction failed.");
                    return;
                }
            }

            _ => warn!("Unknown interaction type: {:#?}!", request.kind()),
        }
    }

    #[instrument(skip(self, ctx))]
    async fn message(&self, ctx: Ctx, msg: Message) {
        if msg.author.bot {
            return;
        }

        if let Err(err) = Self::update_emoji_usage(&ctx, msg.get_emojis()).await {
            error!(%err, "Failed to update emoji usage!");
        }

        // Send new message update.
        let data = ctx.data.read().await;
        let sender = data.get::<MessageSender>().unwrap();

        if sender.receiver_count() > 0 {
            if let Err(err) = sender.send(MessageUpdate::Sent(msg.clone())) {
                error!("{:?}", err);
                return;
            }
        }
    }

    #[instrument(skip(self, ctx))]
    async fn message_update(
        &self,
        ctx: Ctx,
        _old_if_available: Option<Message>,
        new: Option<Message>,
        _event: MessageUpdateEvent,
    ) {
        if let Some(new) = new {
            let data = ctx.data.read().await;
            let sender = data.get::<MessageSender>().unwrap();

            if sender.receiver_count() > 0 {
                if let Err(err) = sender.send(MessageUpdate::Edited(new)) {
                    error!("{:?}", err);
                    return;
                }
            }
        }
    }

    #[instrument(skip(self, ctx, _channel_id, _guild_id))]
    async fn message_delete(
        &self,
        ctx: Ctx,
        _channel_id: ChannelId,
        deleted_message: MessageId,
        _guild_id: Option<GuildId>,
    ) {
        let data = ctx.data.read().await;
        let sender = data.get::<MessageSender>().unwrap();

        if sender.receiver_count() > 0 {
            if let Err(err) = sender.send(MessageUpdate::Deleted(deleted_message)) {
                error!("{:?}", err);
                return;
            }
        }
    }

    #[instrument(skip(self, ctx, _channel_id, deleted_messages, _guild_id))]
    async fn message_delete_bulk(
        &self,
        ctx: Ctx,
        _channel_id: ChannelId,
        deleted_messages: Vec<MessageId>,
        _guild_id: Option<GuildId>,
    ) {
        let data = ctx.data.read().await;
        let sender = data.get::<MessageSender>().unwrap();

        if sender.receiver_count() > 0 {
            for id in deleted_messages {
                if let Err(err) = sender.send(MessageUpdate::Deleted(id)) {
                    error!("{:?}", err);
                    return;
                }
            }
        }
    }

    #[instrument(skip(self, _ctx))]
    async fn reaction_add(&self, _ctx: Ctx, reaction: Reaction) {
        let mut cache = EMOJI_CACHE
            .get_or_init(|| Arc::new(RwLock::new(HashMap::new())))
            .write()
            .await;

        if let ReactionType::Custom {
            animated: _,
            id,
            name: _,
        } = &reaction.emoji
        {
            let count = cache.entry(*id).or_insert_with(EmojiStats::default);
            (*count).reaction_count += 1;
        }
    }
}
