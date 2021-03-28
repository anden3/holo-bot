use std::{collections::HashSet, sync::Arc};

use lazy_static::lazy_static;
use log::error;
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

pub struct DiscordBot;

impl DiscordBot {
    pub async fn start(config: Config) -> Arc<CacheAndHttp> {
        let framework = StandardFramework::new()
            .help(&HELP_CMD)
            .configure(|c| {
                c.prefixes(vec!["草", "|"]);
                c.owners(vec![UserId(113654526589796356)].into_iter().collect());
                c.blocked_guilds(vec![GuildId(755302276176019557)].into_iter().collect());

                c
            })
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

    async fn run(mut client: Client, _config: Config) -> Result<(), Box<dyn std::error::Error>> {
        client.start().await?;

        Ok(())
    }
}

#[group]
#[commands(ogey, pekofy)]
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
#[description = "rrat"]
async fn ogey(ctx: &Context, msg: &Message) -> CommandResult {
    msg.channel_id
        .say(
            &ctx.http,
            MessageBuilder::new()
                .push("rrat <:pekoSlurp:764301779453476914>")
                .build(),
        )
        .await?;

    Ok(())
}

#[command]
#[owners_only]
async fn pekofy(ctx: &Context, msg: &Message) -> CommandResult {
    lazy_static! {
        static ref SENTENCE: Regex = Regex::new(
            r"(?m)(?P<text>.+?)(?P<punct>[\.!\?\u3002\uFE12\uFE52\uFF0E\uFF61\uFF01\uFF1F]+|[\.!\?\u3002\uFE12\uFE52\uFF0E\uFF61\uFF01\uFF1F]*$)"
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
        let mut response = "peko";
        let text = capture.name("text").unwrap().as_str();

        // Check if text is all uppercase.
        if text == &text.to_uppercase() {
            response = "PEKO";
        }

        match text.chars().last().unwrap() as u32 {
            0x3040..=0x30FF | 0xFF00..=0xFFEF | 0x4E00..=0x9FAF => {
                response = "ぺこ";
            }
            _ => (),
        }

        capture.expand(&format!("$text {}$punct", response), &mut pekofied_text);
    }

    msg.channel_id.say(&ctx.http, pekofied_text).await?;

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
        _ => error!("[BOT] Unhandled dispatch error."),
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
