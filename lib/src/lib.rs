#![allow(unknown_lints)]
#![warn(
    clippy::pedantic,
    clippy::cargo,
    clippy::perf,
    clippy::nursery,
    clippy::complexity,
    clippy::correctness,
    clippy::clone_on_ref_ptr,
    clippy::create_dir,
    clippy::decimal_literal_representation,
    clippy::default_numeric_fallback,
    clippy::exit,
    clippy::expect_used,
    clippy::filetype_is_file,
    clippy::if_then_some_else_none,
    clippy::indexing_slicing,
    clippy::inline_asm_x86_att_syntax,
    clippy::let_underscore_must_use,
    clippy::lossy_float_literal,
    clippy::map_err_ignore,
    clippy::mem_forget,
    clippy::multiple_inherent_impl,
    clippy::panic_in_result_fn,
    clippy::rc_buffer,
    clippy::rest_pat_in_fully_bound_structs,
    clippy::semicolon_if_nothing_returned,
    clippy::str_to_string,
    clippy::string_to_string,
    clippy::todo,
    clippy::unimplemented,
    clippy::unneeded_field_pattern,
    clippy::unreachable,
    clippy::unwrap_in_result,
    clippy::unwrap_used,
    clippy::verbose_file_reads,
    clippy::wildcard_enum_match_arm,
    clippy::wrong_pub_self_convention
)]
#![allow(
    clippy::missing_panics_doc,
    clippy::missing_errors_doc,
    clippy::non_ascii_literal,
    clippy::cargo_common_metadata,
    clippy::multiple_crate_versions
)]

use tokio::sync::mpsc::{self, Receiver, Sender, UnboundedReceiver, UnboundedSender};

use apis::{
    birthday_reminder::BirthdayReminder,
    discord_api::{DiscordApi, DiscordMessageData},
    holo_api::{HoloApi, StreamUpdate},
    twitter_api::TwitterApi,
};
use bot::DiscordBot;
use utility::{config::Config, logger::Logger};

pub struct HoloBot {}

impl HoloBot {
    pub async fn start() -> anyhow::Result<()> {
        Logger::initialize()?;

        let config = Config::load_config("settings.json")?;

        let (discord_message_tx, discord_message_rx): (
            Sender<DiscordMessageData>,
            Receiver<DiscordMessageData>,
        ) = mpsc::channel(10);

        let (stream_update_tx, stream_update_rx): (
            UnboundedSender<StreamUpdate>,
            UnboundedReceiver<StreamUpdate>,
        ) = mpsc::unbounded_channel();

        HoloApi::start(config.clone(), discord_message_tx.clone(), stream_update_tx).await;
        TwitterApi::start(config.clone(), discord_message_tx.clone()).await;
        BirthdayReminder::start(config.clone(), discord_message_tx.clone()).await;

        let (task, cache) = DiscordBot::start(config.clone()).await?;

        DiscordApi::start(cache, discord_message_rx, stream_update_rx, config.clone()).await;

        task.await?;
        Ok(())
    }
}
