use linkme::distributed_slice;
use serenity::framework::standard::macros::group;

use interactions::DeclaredInteraction;
use utility::define_command_group;

/* use crate::interactions::InteractionGroup; */

pub mod prelude;
pub mod util;

mod help;
mod interactions;

pub mod birthdays;
pub mod config;
pub mod eightball;
pub mod emoji_usage;
pub mod live;
pub mod meme;
pub mod ogey;
pub mod quote;
/* pub mod reminder; */
pub mod upcoming;

define_command_group!(Fun, [pekofy]);

#[distributed_slice]
pub static FUN_COMMANDS: [DeclaredInteraction] = [..];

#[distributed_slice]
pub static UTILITY_COMMANDS: [DeclaredInteraction] = [..];

pub use help::*;
