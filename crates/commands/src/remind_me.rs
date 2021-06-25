use super::prelude::*;

interaction_setup! {
    name = "remind_me",
    description = "Set reminders.",
    options = [
        //! Add new reminder.
        add: SubCommand = [
            //! When to remind you.
            req when: String,
            //! What to remind you of.
            message: String,
            //! Where to remind you.
            location: String = ["Here", "DM"],
        ],
        //! Remove reminder.
        remove: SubCommand = [
            //! ID of the reminder to remove.
            req id: Integer,
        ],
        //! Show your current reminders.
        list: SubCommand,
    ]
}

#[interaction_cmd]
async fn remind_me(
    ctx: &Ctx,
    interaction: &Interaction,
    config: &Config,
    app_id: u64,
) -> anyhow::Result<()> {
    match_sub_commands! {
        "add" => |when: req String, message: String, location: String| {

        },
        "remove" => |id: u64| {

        },
        "list" => {

        }
    }

    Ok(())
}
