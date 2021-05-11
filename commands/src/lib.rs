use crate::interactions::InteractionGroup;
use serenity::framework::standard::macros::group;

/* use holo_bot_macros::interaction_group; */
use utility::{define_command_group, define_interaction_group};

pub mod prelude;
pub mod util;

mod help;
mod interactions;

define_command_group!(Fun, [pekofy]);
define_command_group!(Utility, [claim, unclaim]);

define_interaction_group!(Fun, [ogey, eightball, meme]);
define_interaction_group!(Utility, [birthdays, live, upcoming, config, emoji_usage]);

pub use help::HELP_CMD;
