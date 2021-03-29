#[path = "apis/meme_api.rs"]
mod meme_api;

use std::{collections::HashSet, sync::Arc};

use lazy_static::lazy_static;
use log::{debug, error};
use regex::Regex;
use serenity::{
    framework::standard::{
        help_commands,
        macros::{command, group, help, hook},
        Args, CommandGroup, CommandResult, Delimiter, DispatchError, HelpOptions,
    },
    prelude::*,
    utils::MessageBuilder,
    CacheAndHttp, Client,
};
use serenity::{framework::StandardFramework, model::prelude::*};

use super::config::Config;
use meme_api::MemeAPI;

impl TypeMapKey for Config {
    type Value = Config;
}

impl TypeMapKey for MemeAPI {
    type Value = MemeAPI;
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

    async fn run(mut client: Client, config: Config) -> Result<(), Box<dyn std::error::Error>> {
        {
            let mut data = client.data.write().await;
            data.insert::<MemeAPI>(MemeAPI::new(&config));
            data.insert::<Config>(config);
        }

        client.start().await?;

        Ok(())
    }
}

#[group]
#[commands(claim, unclaim)]
struct Utility;

#[group]
#[commands(ogey, pekofy, meme)]
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
#[allowed_roles("Admin", "Moderator", "Moderator (JP)", "Server Booster", "60 m deep")]
/// Pekofies replied-to message or the provided text.
async fn pekofy(ctx: &Context, msg: &Message) -> CommandResult {
    lazy_static! {
        static ref SENTENCE: Regex = Regex::new(
            r"(?ms)(?P<text>.+?)(?P<punct>[\.!\?\u3002\uFE12\uFE52\uFF0E\uFF61\uFF01\uFF1F]+|[\.!\?\u3002\uFE12\uFE52\uFF0E\uFF61\uFF01\uFF1F]*$)"
        )
        .unwrap();
    }

    if msg.author.bot {
        return Ok(());
    }

    let mut args = Args::new(
        &msg.content_safe(&ctx.cache).await,
        &[Delimiter::Single(' ')],
    );
    args.trimmed();
    args.advance();

    let text;

    if let Some(remains) = args.remains() {
        text = remains.to_string();
    } else {
        if let Some(src) = &msg.referenced_message {
            if src.author.bot {
                return Ok(());
            }

            text = src.content_safe(&ctx.cache).await;
        } else {
            return Ok(());
        }
    }

    let mut pekofied_text = String::with_capacity(text.len());

    for capture in SENTENCE.captures_iter(&text) {
        if capture.get(0).unwrap().as_str().trim().is_empty() {
            continue;
        }

        let mut response = "peko";
        let text = capture.name("text").unwrap().as_str();

        // Check if text is all uppercase.
        if text == &text.to_uppercase() {
            response = "PEKO";
        }

        // Check if text is Japanese.
        match text.chars().last().unwrap() as u32 {
            0x3040..=0x30FF | 0xFF00..=0xFFEF | 0x4E00..=0x9FAF => {
                response = "ぺこ";
            }
            _ => (),
        }

        capture.expand(&format!("$text {}$punct", response), &mut pekofied_text);
    }

    if pekofied_text.trim().is_empty() {
        return Ok(());
    }

    msg.channel_id.say(&ctx.http, pekofied_text).await?;

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

    let template = args.single::<u32>().unwrap();
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
            msg.channel_id.say(&ctx.http, url).await.unwrap();
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
    async fn message(&self, _ctx: Context, msg: Message) {
        if msg.author.bot {
            return;
        }
    }
}
