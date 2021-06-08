use std::{collections::HashMap, sync::Arc};

use anyhow::{anyhow, Context};
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
use tokio::{
    select,
    sync::{broadcast, watch, RwLockReadGuard},
    task::JoinHandle,
};
use tracing::{debug, error, info, instrument, warn};

use apis::{
    holo_api::{Livestream, StreamUpdate},
    meme_api::MemeApi,
};
use commands::util::*;
use utility::{
    config::{Config, EmojiStats},
    here, setup_interaction_groups,
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
        index_receiver: watch::Receiver<HashMap<u32, Livestream>>,
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
                c.prefixes(vec!["草", "-"]);
                c.owners(vec![owner].into_iter().collect());
                c.blocked_guilds(config.blocked_servers.iter().map(|i| GuildId(*i)).collect());

                c
            })
            .group(&commands::FUN_GROUP);

        let handler = Handler {
            config: config.clone(),
        };

        let client = Client::builder(&config.discord_token)
            .framework(framework)
            .event_handler(handler)
            .await
            .context(here!())?;

        let cache = Arc::<CacheAndHttp>::clone(&client.cache_and_http);

        let task = tokio::spawn(async move {
            match Self::run(client, config, stream_update, index_receiver, exit_receiver).await {
                Ok(()) => (),
                Err(e) => {
                    error!("{:?}", e);
                }
            }

            info!(task = "Discord bot", "Shutting down.");
        });

        Ok((task, cache))
    }

    #[instrument(skip(client, config, exit_receiver))]
    async fn run(
        mut client: Client,
        config: Config,
        stream_update: broadcast::Sender<StreamUpdate>,
        index_receiver: watch::Receiver<HashMap<u32, Livestream>>,
        mut exit_receiver: watch::Receiver<bool>,
    ) -> anyhow::Result<()> {
        {
            let mut data = client.data.write().await;

            let db_handle = config.get_database_handle()?;

            data.insert::<MemeApi>(MemeApi::new(&config)?);
            data.insert::<Quotes>(Quotes(Config::get_quotes(&db_handle)?));
            data.insert::<EmojiUsage>(EmojiUsage(Config::get_emoji_usage(&db_handle)?));

            data.insert::<DbHandle>(DbHandle(Mutex::new(db_handle)));
            data.insert::<RegisteredInteractions>(RegisteredInteractions::default());

            data.insert::<StreamIndex>(StreamIndex(index_receiver));
            data.insert::<ClaimedChannels>(ClaimedChannels::default());

            let (reaction_send, reaction_recv) = broadcast::channel::<ReactionUpdate>(16);
            let (message_send, message_recv) = broadcast::channel::<MessageUpdate>(64);

            std::mem::drop(reaction_recv);
            std::mem::drop(message_recv);

            data.insert::<ReactionSender>(ReactionSender(reaction_send));
            data.insert::<MessageSender>(MessageSender(message_send));
            data.insert::<StreamUpdateTx>(StreamUpdateTx(stream_update));
        }

        select! {
            e = client.start() => {
                e.context(here!())
            }
            e = exit_receiver.changed() => {
                let data = client.data.read().await;

                let (save_result, restore_result) = tokio::join!(
                    Self::save_data(&data),
                    Self::restore_claimed_channels(&data)
                );

                if let Err(err) = save_result.or(restore_result) {
                    error!(%err, "Saving error!");
                }

                e.context(here!())
            }
        }
    }

    async fn save_data(data: &RwLockReadGuard<'_, TypeMap>) -> anyhow::Result<()> {
        let connection = data.get::<DbHandle>().unwrap().lock().await;
        Config::save_emoji_usage(&connection, &data.get::<EmojiUsage>().unwrap().0)?;
        Config::save_quotes(&connection, &data.get::<Quotes>().unwrap().0)?;

        Ok(())
    }

    async fn restore_claimed_channels(data: &RwLockReadGuard<'_, TypeMap>) -> anyhow::Result<()> {
        let claimed_channels = data.get::<ClaimedChannels>().unwrap();

        for (_, token) in claimed_channels.values() {
            token.cancel();
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
}

impl Handler {
    #[instrument(skip(ctx, guild))]
    async fn initialize_stream_chat_pool(
        ctx: &Ctx,
        guild: &Guild,
        pool_category: ChannelId,
    ) -> anyhow::Result<Vec<ChannelId>> {
        let mut pooled_channel_ids = Vec::new();

        if let Some((_, _category)) = guild
            .channels
            .iter()
            .find(|(id, c)| **id == pool_category && c.kind == ChannelType::Category)
        {
            let pooled_channels = guild
                .channels
                .iter()
                .filter(|(_, c)| {
                    if let Some(category) = c.category_id {
                        category == pool_category
                    } else {
                        false
                    }
                })
                .map(|(_, c)| c)
                .collect::<Vec<_>>();

            let needed_channels = 10 - (pooled_channels.len() as i32);
            pooled_channel_ids = pooled_channels
                .into_iter()
                .map(|c| c.id)
                .collect::<Vec<_>>();

            if needed_channels > 0 {
                for _ in 0..needed_channels {
                    let ch = guild
                        .create_channel(&ctx.http, |c| {
                            c.category(pool_category)
                                .name("pooled-stream-chat")
                                .kind(ChannelType::Text)
                        })
                        .await;

                    let ch = match ch {
                        Ok(ch) => ch,
                        Err(err) => {
                            error!(%err, "Couldn't create channel pool!");
                            break;
                        }
                    };

                    pooled_channel_ids.push(ch.id);
                }
            }
        }

        Ok(pooled_channel_ids)
    }

    #[instrument(skip(ctx))]
    async fn interaction_requested(&self, ctx: Ctx, request: Interaction) -> anyhow::Result<()> {
        let request_data = match request.data {
            Some(ref d) => d,
            None => {
                anyhow::bail!("Interaction has no data!");
            }
        };

        let data = ctx.data.read().await;

        let interaction = data
            .get::<RegisteredInteractions>()
            .unwrap()
            .get(&request.guild_id)
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

        match commands::util::should_fail(conf, &ctx, &request, &interaction).await {
            Some(err) => {
                debug!("{:?}", err);
                return Ok(());
            }
            None => {
                let func = interaction.func;
                std::mem::drop(data);

                let app_id = *ctx.cache.current_user_id().await.as_u64();
                let config = self.config.clone();

                tokio::spawn(async move {
                    if let Err(err) = (func)(&ctx, &request, &config, app_id).await {
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

    fn get_emojis_in_message(msg: &Message) -> Vec<EmojiId> {
        let emoji_regex: &'static regex::Regex = utility::regex!(r#"<a?:(\w+):(\d+)>"#);

        emoji_regex
            .captures_iter(&msg.content)
            .map(|caps| EmojiId(caps[2].parse().unwrap()))
            .collect()
    }
}

#[serenity::async_trait]
impl EventHandler for Handler {
    #[instrument(skip(self, ctx, guild))]
    async fn guild_create(&self, ctx: Ctx, guild: Guild, _is_new: bool) {
        info!(name = %guild.name, "Guild initialized!");

        if self.config.blocked_servers.contains(guild.id.as_u64()) {
            return;
        }

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

        /* match Self::initialize_stream_chat_pool(
            &ctx,
            &guild,
            ChannelId(self.config.stream_chat_pool),
        )
        .await
        {
            Ok(pool) if !pool.is_empty() => {
                info!(?pool, "Pool ready to send.");

                let sender_lock = self.stream_pool_ready.lock().await;
                let sender = sender_lock.replace(None);

                if let Some(sender) = sender {
                    if sender.send(pool).is_err() {
                        error!("Failed to send stream pool!");
                    } else {
                        info!("Pool sent!");
                    }
                } else {
                    error!("Failed to get pool channel!");
                }
            }
            Ok(_) => {
                debug!("Empty pool.");
            }
            Err(e) => {
                error!("{}", e);
                return;
            }
        } */
    }

    #[instrument(skip(self, ctx))]
    async fn interaction_create(&self, ctx: Ctx, request: Interaction) {
        match &request.kind {
            InteractionType::Ping => {
                let res = Interaction::create_interaction_response(&request, &ctx.http, |r| {
                    r.kind(InteractionResponseType::Pong)
                })
                .await;

                if let Err(e) = res {
                    error!("{:?}", e);
                }
            }

            InteractionType::ApplicationCommand => {
                if let Err(e) = self.interaction_requested(ctx, request).await {
                    warn!(err = %e, "Interaction failed.");
                    return;
                }
            }

            _ => warn!("Unknown interaction type: {:#?}!", request.kind),
        }
    }

    #[instrument(skip(self, ctx))]
    async fn message(&self, ctx: Ctx, msg: Message) {
        if msg.author.bot {
            return;
        }

        if let Err(err) = Self::update_emoji_usage(&ctx, Self::get_emojis_in_message(&msg)).await {
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

    #[instrument(skip(self, ctx))]
    async fn reaction_add(&self, ctx: Ctx, reaction: Reaction) {
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

        let data = ctx.data.read().await;
        let sender = data.get::<ReactionSender>().unwrap();

        if sender.receiver_count() > 0 {
            if let Err(err) = sender.send(ReactionUpdate::Added(reaction)) {
                error!("{:?}", err);
                return;
            }
        }
    }

    #[instrument(skip(self, ctx))]
    async fn reaction_remove(&self, ctx: Ctx, reaction: Reaction) {
        let data = ctx.data.read().await;
        let sender = data.get::<ReactionSender>().unwrap();

        if sender.receiver_count() == 0 {
            return;
        }

        if let Err(err) = sender.send(ReactionUpdate::Removed(reaction)) {
            error!("{:?}", err);
            return;
        }
    }

    #[instrument(skip(self, ctx))]
    async fn reaction_remove_all(&self, ctx: Ctx, channel: ChannelId, message: MessageId) {
        let data = ctx.data.read().await;
        let sender = data.get::<ReactionSender>().unwrap();

        if sender.receiver_count() == 0 {
            return;
        }

        if let Err(err) = sender.send(ReactionUpdate::Wiped(channel, message)) {
            error!("{:?}", err);
            return;
        }
    }
}
