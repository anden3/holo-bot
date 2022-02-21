use futures::{future, StreamExt, TryStreamExt};
use itertools::Itertools;

use super::prelude::*;

#[poise::command(
    slash_command,
    prefix_command,
    rename = "move",
    required_permissions = "SEND_MESSAGES",
    member_cooldown = 300
)]
/// Moves the conversation to a different channel.
pub(crate) async fn move_conversation(
    ctx: Context<'_>,
    #[description = "The channel to move the conversation to."] channel: ChannelId,
    #[description = "Users to ping when the conversation is moved."] users: Vec<UserId>,
) -> anyhow::Result<()> {
    move_impl(ctx, channel, users).await
}

async fn move_impl(ctx: Context<'_>, channel: ChannelId, users: Vec<UserId>) -> anyhow::Result<()> {
    let user = ctx.author().tag();

    ctx.say(format!(
        "{user} requested that this conversation moves to {channel}.",
        channel = Mention::from(channel)
    ))
    .await?;

    if users.is_empty() {
        channel
            .say(
                ctx.discord(),
                format!("{user} requested that a conversation was moved to this channel."),
            )
            .await?;
    } else {
        let last_user_messages = ctx
            .channel_id()
            .messages_iter(ctx.discord())
            .take(100)
            .try_filter(|m| future::ready(users.contains(&m.author.id)))
            .take(10)
            .map(|m| m.map(|m| format!("{}: {}", m.author.tag(), m.content)))
            .try_collect::<Vec<_>>()
            .await?;

        const LIMIT: usize = 1024;
        let mut current_bytes = 0;

        let mut last_user_messages = last_user_messages
            .into_iter()
            .take_while(|m| {
                current_bytes += m.len() + "\n".len();
                current_bytes <= LIMIT
            })
            .collect::<Vec<_>>();

        last_user_messages.reverse();
        let log = last_user_messages.join("\n");

        channel
            .say(
                ctx.discord(),
                MessageBuilder::new()
                    .push_bold_line(format!(
                        "{user} requested that a conversation was moved to this channel.",
                    ))
                    .push_line(format!(
                        "Participating users: {}",
                        users.iter().map(|u| Mention::from(*u)).join(", ")
                    ))
                    .push_codeblock(log, None)
                    .build(),
            )
            .await?;
    }

    Ok(())
}
