use std::{collections::HashMap, sync::Arc};

use anyhow::{anyhow, Context};
use once_cell::sync::OnceCell;
use rand::prelude::SliceRandom;
use serenity::{
    framework::{
        standard::{macros::hook, Args, Configuration, Delimiter, DispatchError},
        StandardFramework,
    },
    model::{interactions::Interaction, prelude::*},
    prelude::*,
    CacheAndHttp, Client,
};
use tokio::{
    select,
    sync::{broadcast, watch},
};
use tracing::{debug, error, info, instrument, warn};

use apis::{holo_api::HoloApi, meme_api::MemeApi};
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
    #[instrument(skip(config))]
    pub async fn start(
        config: Config,
        exit_receiver: watch::Receiver<bool>,
    ) -> anyhow::Result<(tokio::task::JoinHandle<()>, Arc<CacheAndHttp>)> {
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
            .group(&commands::UTILITY_GROUP)
            .group(&commands::FUN_GROUP);

        let client = Client::builder(&config.discord_token)
            .framework(framework)
            .event_handler(Handler)
            .await
            .context(here!())?;

        let cache = Arc::<CacheAndHttp>::clone(&client.cache_and_http);

        let task = tokio::spawn(async move {
            match Self::run(client, config, exit_receiver).await {
                Ok(()) => (),
                Err(e) => {
                    error!("{:?}", e);
                }
            }

            info!(task = "Discord bot", "Shutting down.");
        });

        Ok((task, cache))
    }

    #[instrument(skip(client, config))]
    async fn run(
        mut client: Client,
        config: Config,
        mut exit_receiver: watch::Receiver<bool>,
    ) -> anyhow::Result<()> {
        {
            let mut data = client.data.write().await;

            let db_handle = config.get_database_handle()?;

            data.insert::<MemeApi>(MemeApi::new(&config)?);
            data.insert::<Config>(config);
            data.insert::<EmojiUsage>(EmojiUsage(Config::get_emoji_usage(&db_handle)?));

            data.insert::<DbHandle>(DbHandle(Mutex::new(db_handle)));
            data.insert::<RegisteredInteractions>(RegisteredInteractions::default());

            let stream_index_lock =
                backoff::future::retry(backoff::ExponentialBackoff::default(), || async {
                    HoloApi::get_stream_index_lock()
                        .ok_or_else(|| backoff::Error::Transient(anyhow!("Failed to get lock")))
                })
                .await
                .context(here!())?;

            data.insert::<StreamIndex>(StreamIndex(stream_index_lock));

            let (reaction_send, reaction_recv) = broadcast::channel::<ReactionUpdate>(16);
            let (message_send, message_recv) = broadcast::channel::<MessageUpdate>(64);

            std::mem::drop(reaction_recv);
            std::mem::drop(message_recv);

            data.insert::<ReactionSender>(ReactionSender(reaction_send));
            data.insert::<MessageSender>(MessageSender(message_send));
        }

        select! {
           e = client.start() => {
                e.context(here!())
            }
            e = exit_receiver.changed() => {
                let data = client.data.read().await;

                let connection = data.get::<DbHandle>().unwrap().lock().await;
                Config::save_emoji_usage(&connection, &data.get::<EmojiUsage>().unwrap().0)?;

                e.context(here!())
            }
        }
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
struct Handler;

#[serenity::async_trait]
impl EventHandler for Handler {
    #[instrument(skip(ctx, guild))]
    async fn guild_create(&self, ctx: Ctx, guild: Guild, _is_new: bool) {
        let mut data = ctx.data.write().await;
        let config = data.get::<Config>().unwrap();

        if config.blocked_servers.contains(guild.id.as_u64()) {
            return;
        }

        let app_id = *ctx.cache.current_user_id().await.as_u64();

        let mut commands = setup_interaction_groups!(guild, [Fun, Utility]);

        let upload =
            RegisteredInteraction::register(&mut commands, &config.discord_token, app_id, &guild)
                .await;

        if let Err(e) = upload {
            error!("{}", e);
            return;
        }

        let commands = commands
            .into_iter()
            .map(|r| (r.command.as_ref().unwrap().id, r))
            .collect::<HashMap<_, _>>();

        let command_map = data.get_mut::<RegisteredInteractions>().unwrap();
        command_map.insert(guild.id, commands);
    }

    #[instrument(skip(ctx))]
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
                let request_data = match request.data {
                    Some(ref d) => d,
                    None => {
                        warn!("Interaction has no data!");
                        return;
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
                        warn!("Unknown interaction found: '{}'", request_data.name);
                        return;
                    }
                };

                match interaction.check_rate_limit(&ctx, &request).await {
                    Ok(false) => return,
                    Err(err) => {
                        error!("{:?}", err);
                        return;
                    }
                    _ => (),
                }

                let conf = CONFIGURATION.get().unwrap();

                match commands::util::should_fail(conf, &ctx, &request, &interaction).await {
                    Some(err) => {
                        debug!("{:?}", err);
                        return;
                    }
                    None => {
                        let func = interaction.func;
                        std::mem::drop(data);

                        tokio::spawn(async move {
                            if let Err(err) = (func)(&ctx, &request).await {
                                error!("{:?}", err);
                                return;
                            }
                        });
                    }
                }
            }

            _ => warn!("Unknown interaction type: {:#?}!", request.kind),
        }
    }

    async fn message(&self, ctx: Ctx, msg: Message) {
        if msg.author.bot {
            return;
        }

        let emoji_regex: &'static regex::Regex = utility::regex!(r#"<a?:(\w+):(\d+)>"#);
        let found_emoji = emoji_regex.captures_iter(&msg.content).collect::<Vec<_>>();

        if !found_emoji.is_empty() {
            if let Ok(mut data) = ctx.data.try_write() {
                let emoji_usage = data.get_mut::<EmojiUsage>().unwrap();

                for cap in found_emoji {
                    let count = emoji_usage
                        .entry(EmojiId(cap[2].parse().unwrap()))
                        .or_insert_with(EmojiStats::default);
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

                for cap in found_emoji {
                    let count = cache
                        .entry(EmojiId(cap[2].parse().unwrap()))
                        .or_insert_with(EmojiStats::default);
                    (*count).text_count += 1;
                }
            }
        }

        let data = ctx.data.read().await;
        let sender = data.get::<MessageSender>().unwrap();

        if sender.receiver_count() > 0 {
            if let Err(err) = sender.send(MessageUpdate::Sent(msg.clone())) {
                error!("{:?}", err);
                return;
            }
        }

        if let Ok(mentions_me) = msg.mentions_me(&ctx.http).await {
            if !mentions_me {
                return;
            }

            let mut args = Args::new(&msg.content, &[Delimiter::Single(' ')]);

            args.trimmed();
            args.advance();

            if args.is_empty() {
                let res = match &msg.referenced_message {
                    Some(msg) if !msg.is_own(&ctx.cache).await => {
                        msg.reply_ping(&ctx.http, "parduuun?").await.err()
                    }
                    Some(_) => None,
                    None => msg.reply_ping(&ctx.http, "parduuun?").await.err(),
                };

                if let Some(err) = res {
                    error!("{:?}", err);
                }
                return;
            }

            let response_vec = match args.remains() {
                Some(msg) => match msg {
                    "marry me" | "will you be my wife?" | "will you be my waifu?" => {
                        vec!["AH↓HA↑HA↑HA↑HA↑ no peko"]
                    }
                    _ => return,
                },
                None => return,
            };

            let response = if let Some(response) = response_vec.choose(&mut rand::thread_rng()) {
                response
            } else {
                error!("Couldn't pick a response!");
                return;
            };

            if let Some(err) = msg.reply_ping(&ctx.http, response).await.err() {
                error!("{:?}", err)
            }

            return;
        }
    }

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
