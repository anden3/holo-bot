use serenity::framework::standard::macros::group;

use holo_bot_macros::interaction_group;
use utility::{define_command_group, define_slash_command_group};

pub mod prelude;
pub mod util;

mod help;
mod interactions;

define_command_group!(Fun, [pekofy]);
define_command_group!(Utility, [claim, unclaim]);

define_slash_command_group!(FunS, [ogey, eightball, meme]);
define_slash_command_group!(UtilityS, [birthdays, live, upcoming]);

pub use help::HELP_CMD;
