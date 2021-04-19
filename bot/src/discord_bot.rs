use std::sync::Arc;

use anyhow::{anyhow, Context};
use commands::util::{MessageSender, MessageUpdate, ReactionSender, ReactionUpdate, StreamIndex};
use log::{debug, error, warn};
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

use apis::{holo_api::HoloApi, meme_api::MemeApi};
use tokio::sync::broadcast;
use utility::{config::Config, get_slash_commands, here, setup_slash_commands};

type Ctx = serenity::prelude::Context;

static CONFIGURATION: OnceCell<Configuration> = OnceCell::new();

pub struct DiscordBot;

impl DiscordBot {
    pub async fn start(config: Config) -> anyhow::Result<Arc<CacheAndHttp>> {
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
                c.blocked_guilds(vec![GuildId(755_302_276_176_019_557)].into_iter().collect());

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

        tokio::spawn(async move {
            match Self::run(client, config).await {
                Ok(()) => (),
                Err(e) => {
                    error!("{:?}", e);
                }
            }
        });

        return Ok(cache);
    }

    async fn run(mut client: Client, config: Config) -> anyhow::Result<()> {
        {
            let mut data = client.data.write().await;

            data.insert::<MemeApi>(MemeApi::new(&config)?);
            data.insert::<Config>(config);

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

        client.start().await.context(here!())?;

        Ok(())
    }
}

#[hook]
#[allow(clippy::wildcard_enum_match_arm)]
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

struct Handler;

#[serenity::async_trait]
impl EventHandler for Handler {
    async fn guild_create(&self, ctx: Ctx, guild: Guild, _is_new: bool) {
        let app_id = *ctx.cache.current_user_id().await.as_u64();

        setup_slash_commands!(
            ctx,
            guild,
            app_id,
            [live, upcoming, eightball, meme, birthdays, ogey]
        );

        /* get_slash_commands!(cmds, FunS, UtilityS);

        for (cmd, _) in cmds {
            if let Err(err) = (cmd.setup)(&ctx, &guild, app_id).await {
                error!("{:?}", err);
                return;
            }
        } */
    }

    async fn interaction_create(&self, ctx: Ctx, interaction: Interaction) {
        match &interaction.kind {
            InteractionType::Ping => {
                let res = Interaction::create_interaction_response(&interaction, &ctx.http, |r| {
                    r.kind(InteractionResponseType::Pong)
                })
                .await;

                if let Err(e) = res {
                    error!("{:?}", e);
                }
            }

            InteractionType::ApplicationCommand => {
                get_slash_commands!(cmds, FunS, UtilityS);

                let interaction_name = if let Some(a) = interaction.data.as_ref() {
                    a
                } else {
                    error!("Couldn't get interaction name!");
                    return;
                }
                .name
                .as_str();

                if let Some((cmd, grp)) = cmds
                    .into_iter()
                    .find(|(cmd, _)| cmd.name == interaction_name)
                {
                    let conf = CONFIGURATION.get().unwrap();

                    match commands::util::should_fail(conf, &ctx, &interaction, cmd.options, grp)
                        .await
                    {
                        Some(err) => {
                            debug!("{:?}", err);
                            return;
                        }
                        None => {
                            tokio::spawn(async move {
                                if let Err(err) = (cmd.fun)(&ctx, &interaction).await {
                                    error!("{:?}", err);
                                    return;
                                }
                            });
                        }
                    }
                } else {
                    warn!("Unknown interaction: '{}'!", interaction_name)
                }
            }

            _ => (),
        }
    }

    async fn message(&self, ctx: Ctx, msg: Message) {
        if msg.author.bot {
            return;
        }

        let data = ctx.data.read().await;
        let sender = data.get::<MessageSender>().unwrap();

        if sender.receiver_count() == 0 {
            return;
        }

        if let Err(err) = sender.send(MessageUpdate::Sent(msg.clone())) {
            error!("{:?}", err);
            return;
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
        _ctx: Ctx,
        _old_if_available: Option<Message>,
        _new: Option<Message>,
        _event: MessageUpdateEvent,
    ) {
    }

    async fn reaction_add(&self, ctx: Ctx, reaction: Reaction) {
        let data = ctx.data.read().await;
        let sender = data.get::<ReactionSender>().unwrap();

        if sender.receiver_count() == 0 {
            return;
        }

        if let Err(err) = sender.send(ReactionUpdate::Added(reaction)) {
            error!("{:?}", err);
            return;
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
