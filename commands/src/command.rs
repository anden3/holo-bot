use super::prelude::*;

#[slash_setup]
pub async fn setup(ctx: &Ctx, guild: &Guild, app_id: u64) -> anyhow::Result<ApplicationCommand> {
    let cmd = Interaction::create_guild_application_command(&ctx.http, guild.id, app_id, |i| {
        i.name("command")
            .description("Shows upcoming birthdays.")
            .create_interaction_option(|o| {
                o.name("branch")
                    .description("Show only talents from this branch of Hololive.")
                    .kind(ApplicationCommandOptionType::String)
                    .add_string_choice("Hololive JP", HoloBranch::HoloJP.to_string())
                    .add_string_choice("Hololive ID", HoloBranch::HoloID.to_string())
                    .add_string_choice("Hololive EN", HoloBranch::HoloEN.to_string())
                    .add_string_choice("Holostars JP", HoloBranch::HolostarsJP.to_string())
            })
    })
    .await
    .context(here!())?;

    Ok(cmd)
}