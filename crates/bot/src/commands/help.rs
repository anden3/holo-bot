use super::prelude::*;

/// Show this menu.
#[poise::command(prefix_command, track_edits, slash_command)]
pub async fn help(
    ctx: Context<'_>,
    #[description = "Specific command to show help about"] command: Option<String>,
) -> Result<(), Error> {
    let config = poise::builtins::HelpConfiguration {
        extra_text_at_bottom: &format!(
            "\
Type {}help command for more info on a command.
You can edit your message to the bot and the bot will edit its response.",
            ctx.prefix()
        ),
        show_context_menu_commands: true,
        ..Default::default()
    };

    poise::builtins::help(ctx, command.as_deref(), config).await?;
    Ok(())
}
