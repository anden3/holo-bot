#[path = "latex_renderer.rs"]
mod latex_renderer;

use std::{collections::HashSet, sync::Arc};

use latex_renderer::LaTeXRenderer;
use serenity::{
    framework::standard::{
        help_commands,
        macros::{command, group, help, hook},
        Args, CommandGroup, CommandResult, DispatchError, HelpOptions,
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
                c.prefixes(vec!["草"]);
                c.owners(vec![UserId(113654526589796356)].into_iter().collect());
                c.blocked_guilds(vec![GuildId(755302276176019557)].into_iter().collect());

                c
            })
            .group(&GENERAL_GROUP);

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
#[commands(ogey)]
struct General;

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
        _ => println!("[BOT] Unhandled dispatch error."),
    }
}

struct Handler;

#[serenity::async_trait]
impl EventHandler for Handler {
    async fn message(&self, context: Context, msg: Message) {
        if msg.author.bot {
            return;
        }

        if vec![ChannelId(319017124775460865), ChannelId(775518900502134784)]
            .contains(&msg.channel_id)
        {
            let mut expression = None;

            if msg.content.starts_with("$$") && msg.content.ends_with("$$") {
                expression = Some(msg.content.as_str());
            } else if msg.content.starts_with("```") && msg.content.ends_with("```") {
                if msg.content.starts_with("```latex") {
                    expression = Some(&msg.content[8..msg.content.len() - 3]);
                } else {
                    expression = Some(&msg.content[3..msg.content.len() - 3]);
                }
            } else if msg.content.starts_with("`") && msg.content.ends_with("`") {
                expression = Some(&msg.content[1..msg.content.len() - 1]);
            }

            if let Some(expression) = expression {
                if expression.is_empty() {
                    return;
                }

                let typing = msg.channel_id.start_typing(&context.http).unwrap();

                let _ = match LaTeXRenderer::render(&expression).await {
                    Ok(image) => msg
                        .channel_id
                        .send_files(&context.http, vec![image.as_str()], |m| m)
                        .await
                        .unwrap(),
                    Err(error) => msg
                        .channel_id
                        .send_message(&context.http, |m| {
                            m.embed(|e| {
                                e.description(error);

                                e
                            });

                            m
                        })
                        .await
                        .unwrap(),
                };

                typing.stop();
            }
        }
    }
}
