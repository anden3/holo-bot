use crate::{define_command_group, define_slash_command_group};
use holo_bot_macros::slash_group;
use serenity::framework::standard::macros::group;

pub mod prelude {
    pub use crate::config::Config;

    pub use anyhow::anyhow;
    pub use holo_bot_macros::{slash_command, slash_setup};
    pub use log::{debug, error, info, warn};
    pub use serenity::{
        client::Context,
        framework::standard::{macros::command, Args, CommandResult, Delimiter},
        model::{
            channel::{Channel, Message},
            guild::Guild,
            id::RoleId,
            interactions::{
                ApplicationCommand, ApplicationCommandOptionType, Interaction,
                InteractionApplicationCommandCallbackDataFlags, InteractionResponseType,
            },
            misc::Mention,
        },
        utils::{Colour, MessageBuilder},
    };
}

mod slash_types;
pub mod util;

define_command_group!(Fun, [pekofy]);
define_command_group!(Utility, [claim, unclaim]);

define_slash_command_group!(FunS, [ogey, eightball, meme]);
define_slash_command_group!(UtilityS, [live, upcoming]);

mod help;
pub use help::HELP_CMD;
