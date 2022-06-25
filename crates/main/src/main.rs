#![forbid(unsafe_code)]
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
    clippy::wrong_self_convention
)]
#![allow(
    clippy::missing_panics_doc,
    clippy::missing_errors_doc,
    clippy::non_ascii_literal,
    clippy::cargo_common_metadata,
    clippy::multiple_crate_versions
)]

mod logger;

use std::{path::Path, sync::Arc};

use tokio::sync::{broadcast, mpsc, oneshot};
use tracing::{info, instrument};

use apis::{
    birthday_reminder::BirthdayReminder,
    discord_api::{DiscordApi, DiscordMessageData},
    holo_api::HoloApi,
    twitter_api::TwitterApi,
};
use bot::DiscordBot;
use utility::{config::Config, streams::StreamUpdate};

fn main() -> anyhow::Result<()> {
    let _logging_guard = logger::Logger::initialize()?;

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async move { async_main().await })
}

#[allow(clippy::too_many_lines)]
#[instrument]
async fn async_main() -> anyhow::Result<()> {
    let config = Config::load(get_config_path()).await?;

    let (discord_message_tx, discord_message_rx): (
        mpsc::Sender<DiscordMessageData>,
        mpsc::Receiver<DiscordMessageData>,
    ) = mpsc::channel(10);

    let (stream_update_tx, _): (
        broadcast::Sender<StreamUpdate>,
        broadcast::Receiver<StreamUpdate>,
    ) = broadcast::channel(64);

    let (guild_ready_tx, guild_ready_rx) = oneshot::channel();
    let (service_restarter, _) = broadcast::channel(4);

    #[allow(clippy::if_then_some_else_none)]
    let stream_indexing = if config.stream_tracking.enabled {
        let service_restarter = service_restarter.subscribe();

        Some(
            HoloApi::start(
                Arc::<Config>::clone(&config),
                discord_message_tx.clone(),
                stream_update_tx.clone(),
                service_restarter,
            )
            .await,
        )
    } else {
        None
    };

    if config.twitter.enabled {
        let service_restarter = service_restarter.subscribe();

        TwitterApi::start(
            Arc::<Config>::clone(&config),
            discord_message_tx.clone(),
            service_restarter,
        )
        .await?;
    }

    if config.birthday_alerts.enabled {
        BirthdayReminder::start(Arc::<Config>::clone(&config), discord_message_tx.clone()).await;
    }

    let (task, cache) = DiscordBot::start(
        Arc::<Config>::clone(&config),
        stream_update_tx.clone(),
        stream_indexing.clone(),
        guild_ready_tx,
        service_restarter,
    )
    .await?;

    DiscordApi::start(
        cache,
        Arc::<Config>::clone(&config),
        discord_message_rx,
        stream_update_tx.clone(),
        stream_indexing,
        guild_ready_rx,
    )
    .await;

    task.await?;
    info!(task = "Main thread", "Shutting down.");

    Ok(())
}

#[cfg(any(target_arch = "arm", target_arch = "aarch64"))]
fn get_config_path() -> &'static Path {
    Path::new(".")
}

#[cfg(target_arch = "x86_64")]
fn get_config_path() -> &'static Path {
    Path::new("settings/development")
}
