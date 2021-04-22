use serenity::framework::standard::macros::group;

/* use holo_bot_macros::interaction_group; */
use utility::{define_command_group, define_interactions};

pub mod prelude;
pub mod util;

mod help;
mod interactions;

define_command_group!(Fun, [pekofy]);
define_command_group!(Utility, [claim, unclaim]);

define_interactions!(ogey, eightball, meme);
define_interactions!(birthdays, live, upcoming, config);

pub use help::HELP_CMD;
