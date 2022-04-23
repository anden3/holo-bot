use super::prelude::*;

#[poise::command(
    prefix_command,
    slash_command,
    required_permissions = "SEND_MESSAGES",
    member_cooldown = 15
)]
/// Uwuifies provided text.
pub(crate) async fn uwuify(
    ctx: Context<'_>,
    #[description = "The text to uwuify."]
    #[rest]
    text: String,
) -> anyhow::Result<()> {
    ctx.say(uwuifier::uwuify_str(&text)).await?;
    Ok(())
}

#[poise::command(
    context_menu_command = "Uwuify message",
    required_permissions = "SEND_MESSAGES",
    member_cooldown = 15
)]
/// Uwuifies message.
pub(crate) async fn uwuify_message(
    ctx: Context<'_>,
    #[description = "Message to pekofy (enter a link or ID)"] msg: Message,
) -> anyhow::Result<()> {
    let text = msg.content_safe(&ctx.discord().cache);
    ctx.say(uwuifier::uwuify_str(&text)).await?;
    Ok(())
}
