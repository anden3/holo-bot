pub use prelude::Context;

mod prelude;

pub(crate) mod config;
// pub(crate) mod music;

mod birthdays;
mod donate;
mod eightball;
mod emoji_usage;
mod help;
mod live;
mod meme;
mod move_conversation;
mod ogey;
pub(crate) mod pekofy;
mod sticker_usage;
mod timestamp;
mod tsfmt;
mod upcoming;
pub(crate) mod uwuify;

pub(crate) fn get_commands() -> Vec<prelude::Command> {
    vec![
        config::config(),
        // music::music(),
        birthdays::birthdays(),
        donate::donate(),
        eightball::eightball(),
        emoji_usage::emoji_usage(),
        help::help(),
        live::live(),
        meme::meme(),
        move_conversation::move_conversation(),
        ogey::ogey(),
        pekofy::pekofy(),
        pekofy::pekofy_message(),
        sticker_usage::sticker_usage(),
        timestamp::timestamp(),
        tsfmt::tsfmt(),
        upcoming::upcoming(),
        uwuify::uwuify(),
        uwuify::uwuify_message(),
    ]
}
