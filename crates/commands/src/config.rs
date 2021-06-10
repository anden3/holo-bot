use super::prelude::*;

interaction_setup! {
    name = "config",
    description = "HoloBot configuration.",
    options = [
        //! Command related settings.
        command: SubCommandGroup = [
            //! Remove command.
            remove: SubCommand = [
                //! Command to remove.
                req command_name: String,
            ],
        ],
    ],
    restrictions = [
        allowed_roles = [
            "Admin"
        ]
    ],
}

#[allow(dead_code, unused_variables, unused_assignments, clippy::single_match)]
#[interaction_cmd]
async fn config(
    ctx: &Ctx,
    interaction: &Interaction,
    config: &Config,
    app_id: u64,
) -> anyhow::Result<()> {
    match_sub_commands! {
        "command remove" => |command_name: req String| {
            let commands = ctx
                .http
                .get_guild_application_commands(app_id, interaction.guild_id.into())
                .await?;

            match commands.iter().find(|c| c.name == command_name) {
                Some(cmd) => {
                    ctx.http
                        .delete_guild_application_command(
                            app_id,
                            interaction.guild_id.into(),
                            cmd.id.into(),
                        )
                        .await?;

                    interaction.create_interaction_response(&ctx.http, |r| {
                        r.kind(InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|d|
                                d.flags(InteractionApplicationCommandCallbackDataFlags::EPHEMERAL)
                                .content("Command deleted!"))
                    }).await?;
                }
                None => {
                    interaction.create_interaction_response(&ctx.http, |r| {
                        r.kind(InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|d|
                                d.flags(InteractionApplicationCommandCallbackDataFlags::EPHEMERAL)
                                .content(format!("Error! Could not find a command with the name '{}'", command_name)))
                    }).await?;
                }
            }
        }
    };

    Ok(())
}
