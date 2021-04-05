use std::{collections::HashSet, ops::Deref, sync::Arc};

use anyhow::anyhow;
use chrono::{DateTime, Utc};
use log::{debug, error};
use rand::prelude::SliceRandom;
use regex::Regex;
use serenity::{
    framework::standard::{
        help_commands,
        macros::{command, group, help, hook},
        Args, CommandGroup, CommandResult, Delimiter, DispatchError, HelpOptions,
    },
    model::interactions::Interaction,
    prelude::*,
    utils::{Colour, MessageBuilder},
    CacheAndHttp, Client,
};
use serenity::{framework::StandardFramework, model::prelude::*};

use crate::regex;
use crate::{
    apis::{
        holo_api::{HoloAPI, StreamState},
        meme_api::MemeAPI,
    },
    commands,
    config::Config,
};

pub struct StreamIndex(crate::apis::holo_api::StreamIndex);

impl Deref for StreamIndex {
    type Target = crate::apis::holo_api::StreamIndex;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl TypeMapKey for Config {
    type Value = Config;
}

impl TypeMapKey for MemeAPI {
    type Value = MemeAPI;
}

impl TypeMapKey for StreamIndex {
    type Value = StreamIndex;
}

pub struct DiscordBot;

impl DiscordBot {
    pub async fn start(config: Config) -> Arc<CacheAndHttp> {
        let framework = StandardFramework::new()
            .help(&HELP_CMD)
            .configure(|c| {
                c.prefixes(vec!["草", "-"]);
                c.owners(vec![UserId(113654526589796356)].into_iter().collect());
                c.blocked_guilds(vec![GuildId(755302276176019557)].into_iter().collect());

                c
            })
            .group(&UTILITY_GROUP)
            .group(&FUN_GROUP);

        let client = Client::builder(&config.discord_token)
            .framework(framework)
            .event_handler(Handler)
            .await
            .unwrap();

        let cache = client.cache_and_http.clone();

        tokio::spawn(async move {
            DiscordBot::run(client, config).await.unwrap();
        });

        return cache;
    }

    async fn run(mut client: Client, config: Config) -> anyhow::Result<()> {
        {
            let mut data = client.data.write().await;

            data.insert::<MemeAPI>(MemeAPI::new(&config));
            data.insert::<Config>(config);

            let stream_index_lock =
                backoff::future::retry(backoff::ExponentialBackoff::default(), || async {
                    HoloAPI::get_stream_index_lock()
                        .ok_or(backoff::Error::Transient(anyhow!("Failed to get lock")))
                })
                .await?;

            data.insert::<StreamIndex>(StreamIndex(stream_index_lock));
        }

        client.start().await?;

        Ok(())
    }
}

#[group]
#[commands(live, upcoming, claim, unclaim)]
struct Utility;

#[group]
#[commands(eightball, ogey, pekofy, meme)]
struct Fun;

#[help]
async fn help_cmd(
    ctx: &Context,
    msg: &Message,
    args: Args,
    help_options: &'static HelpOptions,
    groups: &[&'static CommandGroup],
    owners: HashSet<UserId>,
) -> CommandResult {
    let _ = help_commands::with_embeds(ctx, msg, args, help_options, groups, owners).await;

    Ok(())
}

#[command]
#[owners_only]
/// Shows the currently live talents.
async fn live(ctx: &Context, msg: &Message) -> CommandResult {
    struct LiveEmbedData {
        role: RoleId,
        colour: Colour,
        title: String,
        thumbnail: String,
        url: String,
    }

    let data = ctx.data.read().await;
    let stream_index = data.get::<StreamIndex>().unwrap().read().await;

    let currently_live = stream_index
        .iter()
        .filter(|(_, l)| l.state == StreamState::Live)
        .map(|(_, l)| LiveEmbedData {
            role: l.streamer.discord_role.into(),
            colour: Colour::new(l.streamer.colour),
            title: l.title.clone(),
            thumbnail: l.thumbnail.clone(),
            url: l.url.clone(),
        })
        .collect::<Vec<_>>();

    std::mem::drop(stream_index);

    futures::future::join_all(
        currently_live
            .into_iter()
            .map(|live| {
                msg.channel_id.send_message(&ctx.http, move |m| {
                    m.embed(|e| {
                        e.colour(live.colour);
                        e.thumbnail(live.thumbnail);
                        e.description(format!(
                            "{}\r\n{}\r\n<https://youtube.com/watch?v={}>\r\n\r\n",
                            Mention::from(live.role),
                            live.title,
                            live.url
                        ));

                        e
                    });

                    m
                })
            })
            .collect::<Vec<_>>(),
    )
    .await;

    Ok(())
}

#[command]
#[owners_only]
#[usage = "[within minutes]"]
#[example = "20"]
/// Shows upcoming streams.
async fn upcoming(ctx: &Context, msg: &Message) -> CommandResult {
    struct ScheduledEmbedData {
        role: RoleId,
        colour: Colour,
        title: String,
        thumbnail: String,
        url: String,
        start_at: DateTime<Utc>,
    }

    let mut args = Args::new(
        &msg.content_safe(&ctx.cache).await,
        &[Delimiter::Single(' ')],
    );
    args.trimmed();
    args.advance();

    let minutes = match args.single::<i64>() {
        Ok(m) => m,
        Err(_) => 60,
    };

    let data = ctx.data.read().await;
    let stream_index = data.get::<StreamIndex>().unwrap().read().await;

    let now = Utc::now();

    let mut scheduled = stream_index
        .iter()
        .filter(|(_, l)| {
            l.state == StreamState::Scheduled && (l.start_at - now).num_minutes() <= minutes
        })
        .map(|(_, l)| ScheduledEmbedData {
            role: l.streamer.discord_role.into(),
            colour: Colour::new(l.streamer.colour),
            title: l.title.clone(),
            thumbnail: l.thumbnail.clone(),
            url: l.url.clone(),
            start_at: l.start_at,
        })
        .collect::<Vec<_>>();

    std::mem::drop(stream_index);
    scheduled.sort_unstable_by_key(|l| l.start_at);

    futures::future::join_all(
        scheduled
            .into_iter()
            .map(|scheduled| {
                msg.channel_id.send_message(&ctx.http, move |m| {
                    m.embed(|e| {
                        e.colour(scheduled.colour);
                        e.thumbnail(scheduled.thumbnail);
                        e.timestamp(&scheduled.start_at);
                        e.description(format!(
                            "{}\r\n{}\r\n<https://youtube.com/watch?v={}>\r\n\r\n",
                            Mention::from(scheduled.role),
                            scheduled.title,
                            scheduled.url
                        ));

                        e
                    });

                    m
                })
            })
            .collect::<Vec<_>>(),
    )
    .await;

    Ok(())
}

#[command]
#[usage = "<talent_name>[|talent2_name|...]"]
#[example = "Rikka"]
#[example = "Tokino Sora | Sakura Miko"]
#[owners_only]
/// Claims the channel for some Hololive talents.
async fn claim(ctx: &Context, msg: &Message) -> CommandResult {
    let mut args = Args::new(
        &msg.content_safe(&ctx.cache).await[6..],
        &[Delimiter::Single('|')],
    );
    args.trimmed();

    let mut talents = Vec::new();

    let data = ctx.data.read().await;
    let config = data.get::<Config>().unwrap();

    for arg in args.iter::<String>() {
        if let Ok(talent_name) = arg {
            debug!("{}", talent_name);

            if let Some(user) = config
                .users
                .iter()
                .find(|u| u.display_name.to_lowercase() == talent_name.trim().to_lowercase())
            {
                talents.push(user);
            }
        }
    }

    let mut channel = msg.channel(&ctx.cache).await.unwrap().guild().unwrap();

    channel
        .edit(&ctx.http, |c| {
            c.topic(talents.iter().fold(String::new(), |acc, u| acc + &u.emoji));
            c
        })
        .await
        .unwrap();

    Ok(())
}

#[command]
#[owners_only]
/// Unclaims all talents from a channel.
async fn unclaim(ctx: &Context, msg: &Message) -> CommandResult {
    let mut channel = msg.channel(&ctx.cache).await.unwrap().guild().unwrap();

    channel
        .edit(&ctx.http, |c| {
            c.topic("");
            c
        })
        .await
        .unwrap();

    Ok(())
}

#[command]
/// rrat
async fn ogey(ctx: &Context, msg: &Message) -> CommandResult {
    msg.channel_id
        .say(
            &ctx.http,
            MessageBuilder::new()
                .push("rrat <:pekoSlurp:824792426530734110>")
                .build(),
        )
        .await?;

    Ok(())
}

#[command]
#[allowed_roles(
    "Admin",
    "Moderator",
    "Moderator (JP)",
    "Server Booster",
    "40 m deep",
    "50 m deep",
    "60 m deep",
    "70 m deep",
    "80 m deep",
    "90 m deep",
    "100 m deep"
)]
/// Pekofies replied-to message or the provided text.
async fn pekofy(ctx: &Context, msg: &Message) -> CommandResult {
    let sentence_rgx: &'static Regex = regex!(
        r#"(?msx)                                                           # Flags
        (?P<text>.*?[\w&&[^_]]+.*?)                                         # Text, not including underscores at the end.
        (?P<punct>
            [\.!\?\u3002\uFE12\uFE52\uFF0E\uFF61\uFF01\uFF1F"_\*`\)]+       # Match punctuation not at the end of a line.
            |
            \s*(?:                                                          # Include eventual whitespace after peko.
                [\.!\?\u3002\uFE12\uFE52\uFF0E\uFF61\uFF01\uFF1F"_\*`\)]    # Match punctuation at the end of a line.
                |
                (?:<:\w+:\d+>)                                              # Match Discord emotes at the end of a line.
                |
                [\x{1F600}-\x{1F64F}]                                       # Match Unicode emoji at the end of a line.
            )*$
        )"#
    );

    let mut args = Args::new(
        &msg.content_safe(&ctx.cache).await,
        &[Delimiter::Single(' ')],
    );
    args.trimmed();
    args.advance();

    let text;

    if let Some(remains) = args.remains() {
        text = remains.to_string();
        msg.delete(&ctx.http).await.unwrap();
    } else {
        if let Some(src) = &msg.referenced_message {
            if src.author.bot {
                return Ok(());
            }

            text = src.content_safe(&ctx.cache).await;
            msg.delete(&ctx.http).await.unwrap();
        } else {
            return Ok(());
        }
    }

    if text.starts_with("-pekofy") {
        msg.channel_id.say(&ctx.http, "Nice try peko").await?;
        return Ok(());
    }

    let mut pekofied_text = String::with_capacity(text.len());

    for capture in sentence_rgx.captures_iter(&text) {
        if capture.get(0).unwrap().as_str().trim().is_empty() {
            continue;
        }

        let mut response = " peko";
        let text = capture.name("text").unwrap().as_str();

        // Check if text is all uppercase.
        if text == &text.to_uppercase() {
            response = " PEKO";
        }

        // Check if text is Japanese.
        match text.chars().last().unwrap() as u32 {
            0x3040..=0x30FF | 0xFF00..=0xFFEF | 0x4E00..=0x9FAF => {
                response = "ぺこ";
            }
            _ => (),
        }

        capture.expand(&format!("$text{}$punct", response), &mut pekofied_text);
    }

    if pekofied_text.trim().is_empty() {
        return Ok(());
    }

    msg.channel_id.say(&ctx.http, pekofied_text).await?;

    Ok(())
}

#[command]
#[aliases("8ball")]
#[allowed_roles(
    "Admin",
    "Moderator",
    "Moderator (JP)",
    "Server Booster",
    "20 m deep",
    "30 m deep",
    "40 m deep",
    "50 m deep",
    "60 m deep",
    "70 m deep",
    "80 m deep",
    "90 m deep",
    "100 m deep"
)]
/// Rolls an 8-ball peko.
async fn eightball(ctx: &Context, msg: &Message) -> CommandResult {
    const RESPONSES: &'static [&'static str] = &[
        "As I see it, yes peko.",
        "Ask again later peko.",
        "Better not tell you now peko.",
        "Cannot predict now peko.",
        "Concentrate and ask again peko.",
        "Don’t count on it peko.",
        "It is certain peko.",
        "It is decidedly so peko.",
        "Most likely peko.",
        "My reply is no peko.",
        "My sources say no peko.",
        "Outlook not so good peko.",
        "Outlook good peko.",
        "Reply hazy, try again peko.",
        "Signs point to yes peko.",
        "Very doubtful peko.",
        "Without a doubt peko.",
        "Yes peko.",
        "Yes – definitely peko.",
        "You may rely on it peko.",
    ];

    let response = RESPONSES.choose(&mut rand::thread_rng()).unwrap();
    msg.channel_id
        .send_message(&ctx.http, |m| {
            m.content(response);
            m.reference_message(msg);
            m
        })
        .await
        .unwrap();

    Ok(())
}

#[command]
#[usage = "<meme template ID> <caption 1> [<caption 2>...]"]
#[example = "87743020 \"hit left button\" \"hit right button\""]
#[min_args(2)]
#[allowed_roles("Admin", "Moderator", "Moderator (JP)", "Server Booster")]
/// Creates a meme with the given ID and captions.
async fn meme(ctx: &Context, msg: &Message) -> CommandResult {
    let data = ctx.data.read().await;
    let meme_api = data.get::<MemeAPI>().unwrap();

    let mut args = Args::new(
        &msg.content_safe(&ctx.cache).await,
        &[Delimiter::Single(' ')],
    );
    args.trimmed();
    args.quoted();
    args.advance();

    let template = args.single::<u32>()?;
    let captions = args
        .iter::<String>()
        .map(|a| {
            let str = a.unwrap();
            let str = str.strip_prefix("\"").unwrap_or(&str);
            let str = str.strip_suffix("\"").unwrap_or(&str);
            str.to_string()
        })
        .collect::<Vec<_>>();

    match meme_api.create_meme(template, &captions).await {
        Ok(url) => {
            msg.reply_ping(&ctx.http, url).await.unwrap();
        }
        Err(err) => error!("{}", err),
    };

    Ok(())
}

#[hook]
async fn dispatch_error_hook(ctx: &Context, msg: &Message, error: DispatchError) {
    match error {
        DispatchError::NotEnoughArguments { min, given } => {
            let _ = msg
                .channel_id
                .say(
                    &ctx,
                    &format!("Need {} arguments, but only got {}.", min, given),
                )
                .await;
        }
        DispatchError::TooManyArguments { max, given } => {
            let _ = msg
                .channel_id
                .say(
                    &ctx,
                    &format!("Max arguments allowed is {}, but got {}.", max, given),
                )
                .await;
        }
        _ => error!("Unhandled dispatch error."),
    }
}

struct Handler;

#[serenity::async_trait]
impl EventHandler for Handler {
    async fn guild_create(&self, ctx: Context, guild: Guild, _is_new: bool) {
        let app_id = *ctx.cache.current_user_id().await.as_u64();

        if let Err(err) = commands::ogey::setup_interaction(&ctx, &guild, app_id).await {
            error!("{}", err);
            return;
        }

        if let Err(err) = commands::live::setup_interaction(&ctx, &guild, app_id).await {
            error!("{}", err);
            return;
        }

        if let Err(err) = commands::upcoming::setup_interaction(&ctx, &guild, app_id).await {
            error!("{}", err);
            return;
        }
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        match &interaction.kind {
            InteractionType::Ping => {
                Interaction::create_interaction_response(&interaction, &ctx.http, |r| {
                    r.kind(InteractionResponseType::Pong)
                })
                .await
                .unwrap()
            }

            InteractionType::ApplicationCommand => {
                match interaction.data.as_ref().unwrap().name.as_str() {
                    "ogey" => {
                        if let Err(err) = commands::ogey::on_interaction(&ctx, &interaction).await {
                            error!("{}", err);
                            return;
                        }
                    }
                    "live" => {
                        if let Err(err) = commands::live::on_interaction(&ctx, &interaction).await {
                            error!("{}", err);
                            return;
                        }
                    }
                    "upcoming" => {
                        if let Err(err) =
                            commands::upcoming::on_interaction(&ctx, &interaction).await
                        {
                            error!("{}", err);
                            return;
                        }
                    }
                    _ => (),
                }
            }

            _ => (),
        }
    }

    async fn message(&self, ctx: Context, msg: Message) {
        if msg.author.bot {
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
                match &msg.referenced_message {
                    Some(msg) => {
                        if !msg.is_own(&ctx.cache).await {
                            msg.reply_ping(&ctx.http, "parduuun?").await.unwrap();
                        }
                    }
                    None => {
                        let _ = msg.reply_ping(&ctx.http, "parduuun?").await.unwrap();
                    }
                }

                return;
            }

            let response_vec = match args.remains().unwrap() {
                "marry me" | "will you be my wife?" | "will you be my waifu?" => {
                    vec!["AH↓HA↑HA↑HA↑HA↑ no peko"]
                }
                _ => return,
            };
            let response = response_vec.choose(&mut rand::thread_rng()).unwrap();

            msg.reply_ping(&ctx.http, response).await.unwrap();
            return;
        }
    }
}
