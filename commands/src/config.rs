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

#[interaction_cmd]
#[owners_only]
pub async fn config(ctx: &Ctx, interaction: &Interaction) -> anyhow::Result<()> {
    Ok(())
}
