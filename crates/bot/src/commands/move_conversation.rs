use super::prelude::*;

#[poise::command(
    slash_command,
    prefix_command,
    rename = "move",
    required_permissions = "SEND_MESSAGES"
)]
/// Moves the conversation to a different channel.
pub(crate) async fn move_conversation(
    ctx: Context<'_>,
    #[description = "The channel to move the conversation to."] channel: ChannelId,
) -> anyhow::Result<()> {
    let user = ctx.author().tag();

    ctx.say(format!(
        "{user} requested that this conversation moves to {channel}.",
        channel = Mention::from(channel)
    ))
    .await?;

    channel
        .say(
            ctx.discord(),
            format!("{user} requested that a conversation was moved to this channel."),
        )
        .await?;

    Ok(())
}
