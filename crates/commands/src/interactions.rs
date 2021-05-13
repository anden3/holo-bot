use std::{
    collections::{HashMap, HashSet},
    fmt,
};

use anyhow::Context;
use chrono::{DateTime, Duration, Utc};
use futures::future::BoxFuture;
use log::error;
use reqwest::{header, Client, Url};
use serde::Serialize;
use serde_json::{json, Value};
use serde_with::{serde_as, DisplayFromStr};
use serenity::model::{
    guild::Guild,
    id::{RoleId, UserId},
    interactions::{
        ApplicationCommand, Interaction, InteractionApplicationCommandCallbackDataFlags,
    },
};
use tokio::sync::RwLock;

type Ctx = serenity::client::Context;
use tracing::{info_span, instrument, Instrument};
use utility::here;

pub type CheckFunction =
    for<'fut> fn(
        &'fut Ctx,
        &'fut Interaction,
        &'fut RegisteredInteraction,
    ) -> BoxFuture<'fut, Result<(), serenity::framework::standard::Reason>>;

pub type SetupFunction =
    for<'fut> fn(
        &'fut Guild,
    ) -> BoxFuture<'fut, anyhow::Result<(::bytes::Bytes, InteractionOptions)>>;

pub type InteractionFn =
    for<'fut> fn(&'fut Ctx, &'fut Interaction) -> BoxFuture<'fut, anyhow::Result<()>>;

pub struct DeclaredInteraction {
    pub name: &'static str,
    pub group: &'static str,
    pub setup: SetupFunction,
    pub func: InteractionFn,
}

pub struct RegisteredInteraction {
    pub command: Option<ApplicationCommand>,
    pub name: &'static str,
    pub func: InteractionFn,
    pub options: InteractionOptions,
    pub config_json: bytes::Bytes,

    pub global_rate_limits: RwLock<(u32, DateTime<Utc>)>,
    pub user_rate_limits: RwLock<HashMap<UserId, (u32, DateTime<Utc>)>>,
}

impl RegisteredInteraction {
    #[instrument]
    pub async fn register(
        commands: &mut [Self],
        token: &str,
        app_id: u64,
        guild: &Guild,
    ) -> anyhow::Result<()> {
        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            header::HeaderValue::from_str(&format!("Bot {}", token)).unwrap(),
        );
        headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static(&"application/json"),
        );

        let client = Client::builder()
            .user_agent(concat!(
                env!("CARGO_PKG_NAME"),
                "/",
                env!("CARGO_PKG_VERSION"),
            ))
            .default_headers(headers)
            .build()
            .unwrap();

        Self::upload_commands(&client, commands, app_id, guild).await?;
        Self::set_permissions(&client, commands, app_id, guild).await?;

        Ok(())
    }

    #[instrument]
    async fn upload_commands(
        client: &Client,
        commands: &mut [Self],
        app_id: u64,
        guild: &Guild,
    ) -> anyhow::Result<()> {
        let path = format!(
            "https://discord.com/api/v8/applications/{}/guilds/{}/commands",
            app_id,
            guild.id.as_u64()
        );

        let config = Value::Array(
            commands
                .iter()
                .map(|c| serde_json::from_slice(&c.config_json).unwrap())
                .collect::<Vec<Value>>(),
        );

        let response = client.put(Url::parse(&path)?).json(&config).send().await?;

        let response_bytes = response.bytes().await.context(here!())?;
        let deserializer = &mut serde_json::Deserializer::from_slice(&response_bytes);
        let response: Result<Vec<ApplicationCommand>, _> =
            serde_path_to_error::deserialize(deserializer);

        match response {
            Ok(response) => {
                for cmd in response {
                    if let Some(c) = commands.iter_mut().find(|c| c.name == cmd.name) {
                        c.command = Some(cmd);
                    }
                }

                Ok(())
            }
            Err(e) => {
                error!(
                    "Deserialization error at '{}' in {}.",
                    e.path().to_string(),
                    here!()
                );
                error!(
                    "Data:\r\n{:?}",
                    std::str::from_utf8(&response_bytes).context(here!())?
                );
                Err(e.into())
            }
        }
    }

    #[instrument]
    async fn set_permissions(
        client: &Client,
        commands: &mut [Self],
        app_id: u64,
        guild: &Guild,
    ) -> anyhow::Result<()> {
        let path = format!(
            "https://discord.com/api/v8/applications/{}/guilds/{}/commands/permissions",
            app_id,
            guild.id.as_u64()
        );

        let permissions = Value::Array(
            commands
                .iter()
                .map(|c| {
                    let id = c.command.as_ref().unwrap().id;
                    json!({
                        "id": id.to_string(),
                        "permissions": c.options.permissions
                    })
                })
                .collect::<Vec<Value>>(),
        );

        let response = client
            .put(Url::parse(&path)?)
            .json(&permissions)
            .send()
            .await?;

        if let Err(e) = response.error_for_status_ref() {
            error!("{:#}", response.text().await?);
            return Err(anyhow::anyhow!(e));
        }

        Ok(())
    }

    #[instrument(skip(ctx))]
    pub async fn check_rate_limit(&self, ctx: &Ctx, request: &Interaction) -> anyhow::Result<bool> {
        if let Some(rate_limit) = &self.options.rate_limit {
            match rate_limit.grouping {
                RateLimitGrouping::Everyone => {
                    let mut usage = self.global_rate_limits.write()
                        .instrument(info_span!("Waiting for rate limit access.", rate_limit_type = ?rate_limit.grouping))
                        .await;

                    match Self::within_rate_limit(rate_limit, usage.0, usage.1) {
                        Ok((count, interval_start)) => *usage = (count, interval_start),
                        Err(msg) => {
                            self.send_error_message(ctx, &request, &msg).await?;
                            return Ok(false);
                        }
                    }
                }
                RateLimitGrouping::User => {
                    let mut usage = self.user_rate_limits.write()
                        .instrument(info_span!("Waiting for rate limit access.", rate_limit_type = ?rate_limit.grouping))
                        .await;
                    let now = Utc::now();

                    match usage.get(&request.member.user.id) {
                        Some((count, interval_start)) => {
                            match Self::within_rate_limit(rate_limit, *count, *interval_start) {
                                Ok((c, d)) => {
                                    usage.insert(request.member.user.id, (c, d));
                                }
                                Err(msg) => {
                                    self.send_error_message(ctx, &request, &msg).await?;
                                    return Ok(false);
                                }
                            }
                        }
                        None => {
                            usage.insert(request.member.user.id, (1, now));
                        }
                    }
                }
            }
        }

        Ok(true)
    }

    fn within_rate_limit(
        rate_limit: &RateLimit,
        count: u32,
        interval_start: DateTime<Utc>,
    ) -> Result<(u32, DateTime<Utc>), String> {
        let now = Utc::now();
        let elapsed_time: Duration = now - interval_start;

        if elapsed_time.num_seconds() > rate_limit.interval_sec.into() {
            Ok((1, now))
        } else if count < rate_limit.count {
            Ok((count + 1, interval_start))
        } else {
            let time_to_wait = std::time::Duration::from_secs(u64::from(
                rate_limit.interval_sec - elapsed_time.num_seconds() as u32,
            ));

            let time_to_wait = Duration::from_std(time_to_wait).unwrap();

            let time_to_wait = chrono_humanize::HumanTime::from(time_to_wait).to_text_en(
                chrono_humanize::Accuracy::Precise,
                chrono_humanize::Tense::Present,
            );

            Err(format!(
                "Rate limit exceeded, please wait {} before trying again.",
                time_to_wait
            ))
        }
    }

    #[instrument(skip(ctx))]
    async fn send_error_message(
        &self,
        ctx: &Ctx,
        request: &Interaction,
        message: &str,
    ) -> anyhow::Result<()> {
        request
            .create_interaction_response(&ctx.http, |r| {
                r.interaction_response_data(|d| {
                    d.content(message)
                        .flags(InteractionApplicationCommandCallbackDataFlags::EPHEMERAL)
                })
            })
            .await
            .map_err(|e| anyhow::anyhow!(e))
    }
}

impl std::fmt::Debug for RegisteredInteraction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}

pub struct InteractionGroup {
    pub name: &'static str,
    pub interactions: &'static [&'static DeclaredInteraction],
}

#[derive(Debug, Clone, Default)]
pub struct InteractionOptions {
    pub checks: &'static [Check],
    pub allowed_roles: HashSet<RoleId>,
    pub owners_only: bool,
    pub permissions: Vec<InteractionPermission>,
    pub rate_limit: Option<RateLimit>,
}

#[derive(Debug, Clone, Default)]
pub struct RateLimit {
    pub count: u32,
    pub interval_sec: u32,
    pub grouping: RateLimitGrouping,
}

#[derive(Debug, Clone)]
pub enum RateLimitGrouping {
    User,
    Everyone,
}

impl Default for RateLimitGrouping {
    fn default() -> Self {
        Self::Everyone
    }
}

#[serde_as]
#[derive(Debug, Copy, Clone, Serialize)]
pub struct InteractionPermission {
    #[serde_as(as = "DisplayFromStr")]
    pub id: u64,
    #[serde(rename = "type")]
    pub permission_type: u32,
    pub permission: bool,
}
pub struct Check {
    pub name: &'static str,
    pub function: fn(&Ctx, &Interaction, &RegisteredInteraction) -> bool,
}

impl fmt::Debug for Check {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Check")
            .field("name", &self.name)
            .field("function", &"<fn>")
            .finish()
    }
}

impl PartialEq for Check {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}
