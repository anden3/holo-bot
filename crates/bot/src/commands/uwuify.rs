use once_cell::sync::Lazy;
use uwuifyy::UwUify;

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
    ctx.say(uwuify_str(&text).unwrap_or_else(|| String::from("failed to uwuify message")))
        .await?;

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
    let text = msg.content_safe(ctx);
    ctx.say(uwuify_str(&text).unwrap_or_else(|| String::from("failed to uwuify message")))
        .await?;

    Ok(())
}

pub(crate) fn uwuify_str(text: &str) -> Option<String> {
    static UWUIFIER: Lazy<UwUify> = Lazy::new(|| {
        UwUify::new(
            None, None, None, false, false, true, None, None, None, None, true,
        )
    });

    let mut uwuified = Vec::with_capacity(text.len());

    if let Err(e) = UWUIFIER.uwuify_sentence(text, &mut uwuified) {
        error!(err = ?e, "Failed to uwuify text!");
        return None;
    }

    match String::from_utf8(uwuified) {
        Ok(text) => Some(text),
        Err(e) => {
            error!(err = ?e, "Uwuified text wasn't valid UTF-8!");
            None
        }
    }
}
