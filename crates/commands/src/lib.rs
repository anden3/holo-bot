use crate::interactions::InteractionGroup;
use serenity::framework::standard::macros::group;

use utility::{define_command_group, define_interaction_group};

pub mod prelude;
pub mod util;

mod help;
mod interactions;

define_command_group!(Fun, [pekofy]);

define_interaction_group!(Fun, [ogey, eightball, meme]);
define_interaction_group!(
    Utility,
    [
        birthdays,
        claim,
        unclaim,
        watching,
        live,
        upcoming,
        config,
        emoji_usage,
        quote
    ]
);

pub use help::*;
