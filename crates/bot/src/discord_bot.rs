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
    sync::{broadcast, mpsc, oneshot, watch, RwLockWriteGuard},
    task::JoinHandle,
};
use tracing::{debug, error, info, instrument, warn};

use apis::meme_api::MemeApi;
use utility::{
    config::{
        Config, EmojiStats, EmojiUsageSource, EntryEvent, LoadFromDatabase, Reminder,
        SaveToDatabase,
    },
    discord::*,
    extensions::MessageExt,
    here, setup_interaction_groups,
    streams::*,
};

type Ctx = serenity::prelude::Context;

static CONFIGURATION: OnceCell<Configuration> = OnceCell::new();

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

            data.insert::<DbHandle>(DbHandle(Mutex::new(db_handle)));
            data.insert::<RegisteredInteractions>(RegisteredInteractions::default());

            data.insert::<StreamIndex>(StreamIndex(index_receiver));

            let (message_send, message_recv) = broadcast::channel::<MessageUpdate>(64);
            std::mem::drop(message_recv);

            data.insert::<MessageSender>(MessageSender(message_send));
            data.insert::<ReminderSender>(ReminderSender(reminder_sender));
            data.insert::<StreamUpdateTx>(StreamUpdateTx(stream_update));

            data.insert::<MusicData>(MusicData::default());

            let (emoji_usage_send, emoji_usage_recv) = mpsc::channel(64);
            data.insert::<EmojiUsageSender>(EmojiUsageSender(emoji_usage_send));

            tokio::spawn(
                async move { Self::track_emoji_usage(config.clone(), emoji_usage_recv).await },
            );
        }

        select! {
            e = client.start() => {
                e.context(here!())
            }
            e = exit_receiver.changed() => {
                let mut data = client.data.write().await;

                if let Some(s) = data.get::<EmojiUsageSender>() {
                    if let Err(e) = s.send(EmojiUsageEvent::Terminate).await {
                        error!(?e, "Saving error!");
                    }
                }

                if let Err(e) = Self::save_data(&data).await {
                    error!(?e, "Saving error!");
                }

                if let Err(e) = Self::disconnect_music(&mut data).await {
                    error!(?e, "Saving error!");
                }

                e.context(here!())
            }
        }
    }

    async fn track_emoji_usage(
        config: Config,
        mut emojis: mpsc::Receiver<EmojiUsageEvent>,
    ) -> anyhow::Result<()> {
        let mut emoji_usage: EmojiUsage = {
            let db_handle = config.get_database_handle()?;
            EmojiUsage::load_from_database(&db_handle)?.into()
        };

        while let Some(event) = emojis.recv().await {
            match event {
                EmojiUsageEvent::Used { emojis, usage } => {
                    for id in emojis {
                        let count = emoji_usage.entry(id).or_insert_with(EmojiStats::default);
                        count.add(usage);
                    }
                }
                EmojiUsageEvent::GetUsage(sender) => {
                    if sender.send(emoji_usage.clone()).is_err() {
                        error!("Failed to send emoji usage!");
                        continue;
                    }
                }
                EmojiUsageEvent::Terminate => {
                    let db_handle = config.get_database_handle()?;
                    emoji_usage.save_to_database(&db_handle)?;
                    break;
                }
            }
        }

        Ok(())
    }

    async fn save_data(data: &RwLockWriteGuard<'_, TypeMap>) -> anyhow::Result<()> {
        let connection = data.get::<DbHandle>().unwrap().lock().await;

        data.get::<Quotes>()
            .and_then(|d| d.save_to_database(&connection).ok());

        Ok(())
    }

    async fn disconnect_music(data: &mut RwLockWriteGuard<'_, TypeMap>) -> anyhow::Result<()> {
        let manager = data
            .get::<SongbirdKey>()
            .ok_or_else(|| anyhow!("Songbird manager not available."))?
            .clone();

        if let Some(music_data) = data.get_mut::<MusicData>() {
            for id in music_data.keys().copied().collect::<Vec<_>>() {
                music_data.remove(&id);
                manager.remove(id).await.context(here!())?;
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
                    }
                });
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

        // Send new message update.
        let data = ctx.data.read().await;
        let sender = data.get::<MessageSender>().unwrap();

        if sender.receiver_count() > 0 {
            if let Err(err) = sender.send(MessageUpdate::Sent(msg.clone())) {
                error!("{:?}", err);
                return;
            }
        }

        // Send emoji tracking update.
        let emoji_usage = data.get::<EmojiUsageSender>().unwrap();

        if let Err(e) = emoji_usage
            .send(EmojiUsageEvent::Used {
                emojis: msg.get_emojis(),
                usage: EmojiUsageSource::InText,
            })
            .await
            .context(here!())
        {
            error!(?e, "Failed to update emoji usage!");
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

    #[instrument(skip(self, ctx))]
    async fn reaction_add(&self, ctx: Ctx, reaction: Reaction) {
        let data = ctx.data.read().await;

        if let ReactionType::Custom {
            animated: _,
            id,
            name: _,
        } = &reaction.emoji
        {
            // Send emoji tracking update.
            let emoji_usage = data.get::<EmojiUsageSender>().unwrap();

            if let Err(e) = emoji_usage
                .send(EmojiUsageEvent::Used {
                    emojis: vec![*id],
                    usage: EmojiUsageSource::AsReaction,
                })
                .await
                .context(here!())
            {
                error!(?e, "Failed to update emoji usage!");
            }
        }
    }
}
