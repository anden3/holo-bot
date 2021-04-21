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
}

#[allow(dead_code, unused_variables, unused_assignments, clippy::single_match)]
#[interaction_cmd]
#[owners_only]
async fn config(ctx: &Ctx, interaction: &Interaction) -> anyhow::Result<()> {
    for group in &interaction.data.as_ref().unwrap().options {
        match group.name.as_str() {
            "command" => {
                for command in &group.options {
                    match command.name.as_str() {
                        "remove" => {
                            parse_interaction_options!(command, [cmd_name: req String]);

                            let app_id = *ctx.cache.current_user_id().await.as_u64();

                            let commands = ctx
                                .http
                                .get_guild_application_commands(app_id, interaction.guild_id.into())
                                .await?;

                            match commands.iter().find(|c| c.name == cmd_name) {
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
                                                .content(format!("Error! Could not find a command with the name '{}'", cmd_name)))
                                    }).await?;
                                }
                            }
                            break;
                        }
                        _ => (),
                    }
                }
                break;
            }
            _ => (),
        }
    }

    Ok(())
}
