use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    sync::Arc,
};

use anyhow::{anyhow, Context};
use chrono::Utc;
use commands::prelude::ApplicationCommandInteraction;
use futures::stream::{FuturesUnordered, StreamExt};
use holo_bot_macros::clone_variables;
use once_cell::sync::OnceCell;
use serenity::{
    framework::{
        standard::{macros::hook, Configuration, DispatchError},
        StandardFramework,
    },
    model::{interactions::Interaction, prelude::*},
    prelude::*,
    utils::Color,
    CacheAndHttp, Client,
};
use songbird::{SerenityInit, SongbirdKey};
use tokio::{
    select,
    sync::{broadcast, mpsc, oneshot, watch, RwLockWriteGuard},
    task::JoinHandle,
    time::sleep,
};
use tracing::{debug, error, info, instrument, warn};

use apis::meme_api::MemeApi;
use unicode_truncate::UnicodeTruncateStr;
use utility::{
    config::{
        Config, Database, EmojiStats, EmojiUsageSource, EntryEvent, LoadFromDatabase,
        ReactTempMuteConfig, Reminder, SaveToDatabase,
    },
    discord::*,
    extensions::MessageExt,
    here,
    streams::*,
};

type Ctx = serenity::prelude::Context;

static CONFIGURATION: OnceCell<Configuration> = OnceCell::new();

pub struct DiscordBot;

impl DiscordBot {
    #[instrument(skip(
        config,
        stream_update,
        reminder_sender,
        index_receiver,
        guild_ready,
        exit_receiver
    ))]
    pub async fn start(
        config: Arc<Config>,
        stream_update: broadcast::Sender<StreamUpdate>,
        reminder_sender: mpsc::Sender<EntryEvent<u64, Reminder>>,
        index_receiver: Option<watch::Receiver<HashMap<String, Livestream>>>,
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
                    .blocked_guilds(config.blocked.servers.clone())
                    .blocked_users(config.blocked.users.clone())
                    .allowed_channels(config.blocked.channels.clone())
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

    #[instrument(skip(
        client,
        config,
        stream_update,
        reminder_sender,
        index_receiver,
        exit_receiver
    ))]
    async fn run(
        mut client: Client,
        config: Arc<Config>,
        stream_update: broadcast::Sender<StreamUpdate>,
        reminder_sender: mpsc::Sender<EntryEvent<u64, Reminder>>,
        index_receiver: Option<watch::Receiver<HashMap<String, Livestream>>>,
        mut exit_receiver: watch::Receiver<bool>,
    ) -> anyhow::Result<()> {
        {
            let mut data = client.data.write().await;

            let db_handle = config.database.get_handle()?;

            data.insert::<RegisteredInteractions>(RegisteredInteractions::default());

            {
                let (message_send, _) = broadcast::channel::<MessageUpdate>(64);
                data.insert::<MessageSender>(MessageSender(message_send));

                let (reaction_send, _) = broadcast::channel::<ReactionUpdate>(64);
                data.insert::<ReactionSender>(ReactionSender(reaction_send));
            }

            if config.stream_tracking.enabled {
                if let Some(index) = index_receiver {
                    data.insert::<StreamIndex>(StreamIndex(index));
                }

                data.insert::<StreamUpdateTx>(StreamUpdateTx(stream_update));
            }

            if config.quotes.enabled {
                data.insert::<Quotes>(Quotes::load_from_database(&db_handle)?.into());
            }

            if config.meme_creation.enabled {
                data.insert::<MemeApi>(MemeApi::new(&config.meme_creation)?);
            }

            if config.music_bot.enabled {
                data.insert::<MusicData>(MusicData::default());
            }

            if config.reminders.enabled {
                data.insert::<ReminderSender>(ReminderSender(reminder_sender));
            }

            if config.emoji_tracking.enabled {
                let (emoji_usage_send, emoji_usage_recv) = mpsc::channel(64);
                data.insert::<EmojiUsageSender>(EmojiUsageSender(emoji_usage_send));

                tokio::spawn(clone_variables!(config; {
                    if let Err(e) = Self::track_emoji_usage(&config.database, emoji_usage_recv).await.context(here!()) {
                        error!("{:?}", e);
                    }
                }));
            }

            if config.react_temp_mute.enabled {
                let reaction_receiver = data.get::<ReactionSender>().unwrap().subscribe();
                let ctx = client.cache_and_http.clone();

                tokio::spawn(clone_variables!(config; {
                    if let Err(e) = Self::react_temp_mute_handler(ctx, &config.react_temp_mute, reaction_receiver).await.context(here!()) {
                        error!("{:?}", e);
                    }
                }));
            }

            data.insert::<DbHandle>(DbHandle(Mutex::new(db_handle)));
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

    #[instrument(skip(database, emojis))]
    async fn track_emoji_usage(
        database: &Database,
        mut emojis: mpsc::Receiver<EmojiUsageEvent>,
    ) -> anyhow::Result<()> {
        let mut emoji_usage: EmojiUsage = {
            let db_handle = database.get_handle()?;
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
                    let db_handle = database.get_handle()?;
                    emoji_usage.save_to_database(&db_handle)?;
                    break;
                }
            }
        }

        Ok(())
    }

    #[instrument(skip(ctx, config, reactions))]
    async fn react_temp_mute_handler(
        ctx: Arc<CacheAndHttp>,
        config: &ReactTempMuteConfig,
        mut reactions: broadcast::Receiver<ReactionUpdate>,
    ) -> anyhow::Result<()> {
        struct ReactedMessage {
            count: usize,
            reacters: HashSet<UserId>,
        }

        let mut mute_participants: lru::LruCache<UserId, usize> = lru::LruCache::new(16);
        let mut cache = lru::LruCache::new(8);
        let mut muted_users = FuturesUnordered::new();

        loop {
            let reaction;

            select! {
                _ = muted_users.select_next_some(), if !muted_users.is_empty() => {
                    debug!("User unmuted!");
                    continue;
                }

                r = reactions.recv() => {
                    reaction = match r {
                        Ok(r) => r,
                        Err(broadcast::error::RecvError::Closed) => break,
                        Err(e) => {
                            error!(?e, "Failed to receive reaction!");
                            continue;
                        }
                    }
                }
            }

            let (r, was_removed) = match reaction {
                ReactionUpdate::Added(r) => (r, false),
                ReactionUpdate::Removed(r) => (r, true),
            };

            match r.emoji {
                ReactionType::Unicode(_) => continue,
                ReactionType::Custom { id, .. } if !config.reactions.contains(&id) => continue,
                _ => (),
            }

            if Utc::now() - r.message_id.created_at() > config.eligibility_duration {
                cache.pop(&r.message_id);
                continue;
            }

            let message = match r
                .channel_id
                .message(&ctx.http, &r.message_id)
                .await
                .context(here!())
            {
                Ok(m) => m,
                Err(e) => {
                    error!(?e, "Failed to get message!");
                    continue;
                }
            };

            if message.author.bot {
                continue;
            }

            let mut msg_data = match cache.get_mut(&r.message_id) {
                Some(m) => m,
                None if was_removed => continue,
                None => {
                    cache.put(
                        r.message_id,
                        ReactedMessage {
                            count: 0,
                            reacters: HashSet::new(),
                        },
                    );

                    cache.get_mut(&r.message_id).unwrap()
                }
            };

            if was_removed {
                if let Some(uid) = r.user_id {
                    if !msg_data.reacters.remove(&uid) {
                        continue;
                    }
                }

                msg_data.count -= 1;

                if msg_data.count == 0 {
                    cache.pop(&r.message_id);
                }

                continue;
            } else {
                if let Some(uid) = r.user_id {
                    if !msg_data.reacters.insert(uid) {
                        continue;
                    }
                }

                msg_data.count += 1;
            }

            if msg_data.count >= config.required_reaction_count {
                let msg_data = cache.pop(&r.message_id).unwrap();

                if let Err(e) = message.delete(&ctx.http).await.context(here!()) {
                    error!(?e, "Failed to delete message!");
                }

                let guild_id = r.guild_id.unwrap();
                let author_id = message.author.id;
                let http = ctx.http.clone();

                let mute_future = async move {
                    let mut member = match guild_id.member(&http, author_id).await.context(here!())
                    {
                        Ok(m) => m,
                        Err(e) => {
                            error!(?e, "Failed to get member!");
                            return;
                        }
                    };

                    if let Err(e) = member
                        .add_role(&http, config.mute_role)
                        .await
                        .context(here!())
                    {
                        error!(?e, "Failed to mute member!");
                        return;
                    }

                    sleep(config.mute_duration.to_std().unwrap()).await;

                    if let Err(e) = member
                        .remove_role(&http, config.mute_role)
                        .await
                        .context(here!())
                    {
                        error!(?e, "Failed to unmute member!");

                        if let Some(log_ch) = config.logging_channel {
                            let _ = log_ch
                                .say(
                                    &http,
                                    format!(
                                        "@here {} failed to be unmuted, do it manually!",
                                        Mention::from(author_id)
                                    ),
                                )
                                .await;
                        }

                        return;
                    }
                };

                muted_users.push(mute_future);

                if let Some(log_ch) = &config.logging_channel {
                    let mut content = message.content.clone();

                    if content.len() >= 1024 {
                        let (truncated_data, _len) = content.unicode_truncate(1021);
                        content = format!("{}...", truncated_data);
                    }

                    let mut voters = Vec::with_capacity(msg_data.reacters.len());

                    for voter_id in &msg_data.reacters {
                        let voter = match voter_id
                            .to_user(&ctx.http)
                            .await
                            .context(here!())
                            .map(|u| u.tag())
                        {
                            Ok(v) => v,
                            Err(e) => {
                                error!(?e, "Failed to get user!");
                                Mention::from(*voter_id).to_string()
                            }
                        };

                        voters.push(voter)
                    }

                    let res = log_ch
                        .send_message(&ctx.http, |m| {
                            m.embed(|e| {
                                e.title("User got temp bonked");
                                e.author(|a| {
                                    a.name(message.author.tag()).icon_url(
                                        message
                                            .author
                                            .avatar_url()
                                            .unwrap_or_else(|| message.author.default_avatar_url()),
                                    )
                                });
                                e.colour(Color::RED);
                                e.fields([
                                    ("Channel", message.channel_id.mention().to_string(), true),
                                    ("Message", content, true),
                                    (
                                        "Voters",
                                        voters.iter().fold(String::new(), |acc, u| {
                                            format!("{}\n{}", acc, u)
                                        }),
                                        true,
                                    ),
                                ]);

                                if !message.attachments.is_empty() {
                                    e.image(message.attachments[0].url.clone());

                                    if message.attachments.len() > 1 {
                                        e.field(
                                            "Additional Images",
                                            message
                                                .attachments
                                                .iter()
                                                .skip(1)
                                                .fold(String::new(), |s, i| {
                                                    format!("{}\n{}", s, i.url)
                                                }),
                                            true,
                                        );
                                    }
                                }

                                e
                            });

                            m
                        })
                        .await;

                    if let Err(e) = res.context(here!()) {
                        error!(?e, "Failed to send logging message!");
                    }
                }

                // Keep track of users muting others often.
                for user in msg_data.reacters {
                    if let Some(mute_count) = mute_participants.get_mut(&user) {
                        *mute_count += 1;

                        if *mute_count >= config.excessive_mute_threshold {
                            if let Some(log_ch) = &config.logging_channel {
                                let _ = log_ch
                                    .say(
                                        &ctx.http,
                                        format!(
                                            "{} has helped mute people {} times as of late.",
                                            Mention::from(user),
                                            mute_count
                                        ),
                                    )
                                    .await;
                            }
                        }
                    } else {
                        mute_participants.put(user, 1);
                    }
                }
            }
        }

        Ok(())
    }

    #[instrument(skip(data))]
    async fn save_data(data: &RwLockWriteGuard<'_, TypeMap>) -> anyhow::Result<()> {
        let connection = data.get::<DbHandle>().unwrap().lock().await;

        data.get::<Quotes>()
            .and_then(|d| d.save_to_database(&connection).ok());

        Ok(())
    }

    #[instrument(skip(data))]
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
    config: Arc<Config>,
    guild_notifier: Mutex<RefCell<Option<oneshot::Sender<()>>>>,
}

impl Handler {
    #[instrument(skip(self, guild))]
    async fn register_interaction_group(
        &self,
        guild: &Guild,
        group: &[DeclaredInteraction],
    ) -> Vec<RegisteredInteraction> {
        let mut cmds = Vec::with_capacity(group.len());

        for interaction in group {
            if let Some(enable_check) = interaction.enabled {
                if !(enable_check)(&self.config) {
                    continue;
                }
            }

            match (interaction.setup)(guild).await {
                Ok((c, o)) => cmds.push(RegisteredInteraction {
                    name: interaction.name,
                    command: None,
                    func: interaction.func,
                    options: o,
                    config_json: c,
                    global_rate_limits: RwLock::new((0, Utc::now())),
                    user_rate_limits: RwLock::new(HashMap::new()),
                }),
                Err(e) => ::log::error!("{:?} {}", e, here!()),
            }
        }

        cmds
    }

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
        if self.config.blocked.servers.contains(&guild.id) {
            return;
        }

        info!(name = %guild.name, "Guild initialized!");

        let token = self.config.discord_token.clone();

        // Upload interactions to Discord.
        let app_id = *ctx.cache.current_user_id().await.as_u64();

        let groups = [commands::FUN_COMMANDS, commands::UTILITY_COMMANDS];
        let command_count = groups.iter().map(|g| g.len()).sum();

        let mut commands = Vec::with_capacity(command_count);

        for group in groups {
            commands.extend(self.register_interaction_group(&guild, &group).await);
        }

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

                if self.config.blocked.servers.contains(&cmd.guild_id.unwrap()) {
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

        if self.config.emoji_tracking.enabled {
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
        let sender = data.get::<ReactionSender>().unwrap();

        let emoji = reaction.emoji.clone();

        if sender.receiver_count() > 0 {
            if let Err(err) = sender.send(ReactionUpdate::Added(reaction)) {
                error!("{:?}", err);
                return;
            }
        }

        if self.config.emoji_tracking.enabled {
            if let ReactionType::Custom {
                animated: _,
                id,
                name: _,
            } = &emoji
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

    #[instrument(skip(self, ctx))]
    async fn reaction_remove(&self, ctx: Ctx, reaction: Reaction) {
        let data = ctx.data.read().await;
        let sender = data.get::<ReactionSender>().unwrap();

        if sender.receiver_count() > 0 {
            if let Err(err) = sender.send(ReactionUpdate::Removed(reaction)) {
                error!("{:?}", err);
                return;
            }
        }
    }
}
