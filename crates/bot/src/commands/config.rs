use super::prelude::*;

#[poise::command(
    slash_command,
    prefix_command,
    required_permissions = "KICK_MEMBERS",
    subcommands("remove_command")
)]
/// Configure Pekobot.
pub async fn config(_ctx: Context<'_>) -> anyhow::Result<()> {
    Ok(())
}

#[poise::command(slash_command, prefix_command, required_permissions = "KICK_MEMBERS")]
/// Remove command.
pub(crate) async fn remove_command(
    ctx: Context<'_>,
    #[description = "Command to remove."]
    #[autocomplete = "autocomplete_command"]
    command_name: String,
) -> anyhow::Result<()> {
    let discord_ctx = ctx.discord();

    let guild_id = ctx
        .guild_id()
        .ok_or_else(|| anyhow::anyhow!("Could not get guild id."))?
        .0;

    let commands = discord_ctx
        .http
        .get_guild_application_commands(guild_id)
        .await?;

    match commands.iter().find(|c| c.name == command_name) {
        Some(cmd) => {
            discord_ctx
                .http
                .delete_guild_application_command(guild_id, cmd.id.into())
                .await?;

            ctx.send(|m| {
                m.ephemeral(true)
                    .content(format!("Removed command: `{command_name}`."))
            })
            .await?;
        }
        None => {
            ctx.send(|m| {
                m.ephemeral(true).content(format!(
                    "Error! Could not find a command with the name `{command_name}`."
                ))
            })
            .await?;
        }
    }

    Ok(())
}

async fn autocomplete_command(
    ctx: Context<'_>,
    partial: String,
) -> impl Iterator<Item = AutocompleteChoice<String>> {
    let commands = if let Some(guild_id) = ctx.guild_id() {
        match ctx
            .discord()
            .http
            .get_guild_application_commands(guild_id.0)
            .await
        {
            Ok(commands) => commands
                .iter()
                .filter(|c| c.name.starts_with(&partial))
                .map(|c| AutocompleteChoice {
                    name: c.name.clone(),
                    value: c.name.clone(),
                })
                .collect(),
            Err(e) => {
                error!("Could not get guild application commands: {e:?}");
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };

    commands.into_iter()
}
