use linkme::distributed_slice;
use serenity::framework::standard::macros::group;

use utility::{define_command_group, discord::DeclaredInteraction};

pub mod prelude;
pub mod util;

mod help;

pub mod birthdays;
pub mod config;
pub mod donate;
pub mod eightball;
pub mod emoji_usage;
pub mod live;
pub mod meme;
pub mod music;
pub mod ogey;
pub mod quote;
pub mod timestamp;
/* pub mod reminder; */
pub mod upcoming;

define_command_group!(Fun, [pekofy]);
define_command_group!(Utility, [tsfmt]);

#[distributed_slice]
pub static FUN_COMMANDS: [DeclaredInteraction] = [..];

#[distributed_slice]
pub static UTILITY_COMMANDS: [DeclaredInteraction] = [..];

pub use help::*;
pub use pekofy::pekofy_text;
