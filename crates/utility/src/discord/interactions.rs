use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    fmt,
};

use anyhow::Context;
use chrono::{DateTime, Duration, Utc};
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use serde_with::{serde_as, DisplayFromStr};
use serenity::model::{
    guild::Guild,
    id::{RoleId, UserId},
    interactions::{
        application_command::{ApplicationCommand, ApplicationCommandInteraction},
        InteractionApplicationCommandCallbackDataFlags,
    },
};
use tokio::sync::RwLock;
use tracing::{error, info_span, instrument, warn, Instrument};

use crate::{config::Config, functions::get_response_or_error, here};

type Ctx = serenity::client::Context;

pub type CheckFunction =
    for<'fut> fn(
        &'fut Ctx,
        &'fut ApplicationCommandInteraction,
        &'fut RegisteredInteraction,
    ) -> BoxFuture<'fut, Result<(), serenity::framework::standard::Reason>>;

pub type IsInteractionEnabledFn = fn(&Config) -> bool;

pub type SetupFunction =
    for<'fut> fn(
        &'fut Guild,
    ) -> BoxFuture<'fut, anyhow::Result<(::bytes::Bytes, InteractionOptions)>>;

pub type InteractionFn = for<'fut> fn(
    &'fut Ctx,
    &'fut ApplicationCommandInteraction,
    &'fut Config,
) -> BoxFuture<'fut, anyhow::Result<()>>;

pub struct DeclaredInteraction {
    pub name: &'static str,
    pub group: &'static str,
    pub setup: SetupFunction,
    pub func: InteractionFn,
    pub enabled: Option<IsInteractionEnabledFn>,
}

impl std::fmt::Debug for DeclaredInteraction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}::{}", self.group, self.name)
    }
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

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum InteractionRegistrationResult {
    Success(Vec<ApplicationCommand>),
    Error(InteractionRegistrationErrors),
}

#[serde_as]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct InteractionRegistrationErrors {
    pub code: u32,
    pub message: String,

    #[serde_as(as = "HashMap<DisplayFromStr, _>")]
    pub errors: HashMap<u8, InteractionOptionError>,
}

impl InteractionRegistrationErrors {
    pub fn print_error(self, commands: &[RegisteredInteraction]) -> anyhow::Error {
        let err = anyhow::anyhow!("[{}] {:?}", self.code, self.message);

        for (index, error) in self.errors {
            let cmd = match commands.get(index as usize) {
                Some(c) => c,
                None => continue,
            };

            let json: serde_json::Value = match serde_json::from_slice(&cmd.config_json) {
                Ok(v) => v,
                Err(e) => {
                    error!(?e, "Failed to parse option json.");
                    continue;
                }
            };

            error.print_error(&[cmd.name], &json);
        }

        err
    }
}

#[serde_as]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct InteractionOptionError {
    #[serde(default)]
    #[serde_as(as = "HashMap<DisplayFromStr, _>")]
    pub options: HashMap<u8, InteractionOptionError>,
    #[serde(default)]
    #[serde_as(as = "HashMap<DisplayFromStr, _>")]
    pub choices: HashMap<u8, InteractionChoiceError>,
    #[serde(default)]
    pub required: Option<RegErrorList>,
}

impl InteractionOptionError {
    pub fn print_error(self, path: &[&str], opt: &serde_json::Value) {
        let name = opt.get("name").and_then(|n| n.as_str()).unwrap_or("?");

        let mut path = path.to_vec();
        path.push(name);

        if let Some(required) = self.required {
            for error in required {
                error!(path = %path.join("::") + "::required", code = %error.code, message = %error.message);
            }
        }

        for (choice_index, error) in self.choices {
            let choice = match opt
                .get("choices")
                .and_then(|o| o.get(choice_index as usize))
            {
                Some(o) => o,
                None => continue,
            };

            error.print_error(&path, choice);
        }

        for (opt_index, error) in self.options {
            let sub_opt = match opt.get("options").and_then(|o| o.get(opt_index as usize)) {
                Some(o) => o,
                None => continue,
            };

            error.print_error(&path, sub_opt);
        }
    }
}

#[serde_as]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct InteractionChoiceError {
    #[serde(default)]
    pub name: Option<RegErrorList>,
    #[serde(default)]
    pub value: Option<RegErrorList>,
}

impl InteractionChoiceError {
    pub fn print_error(self, path: &[&str], choice: &serde_json::Value) {
        let name = choice.get("name").and_then(|n| n.as_str()).unwrap_or("?");

        let mut path = path.to_vec();
        path.push(name);

        if let Some(name_errors) = self.name {
            for error in name_errors {
                error!(path = %path.join("::") + "::name", code = %error.code, message = %error.message);
            }
        }

        if let Some(value_errors) = self.value {
            for error in value_errors {
                error!(path = %path.join("::") + "::value", code = %error.code, message = %error.message);
            }
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegErrorList {
    #[serde(rename = "_errors")]
    pub errors: Vec<InteractionRegistrationError>,
}

impl IntoIterator for RegErrorList {
    type Item = InteractionRegistrationError;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.errors.into_iter()
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct InteractionRegistrationError {
    code: String,
    message: String,
}

impl RegisteredInteraction {
    #[instrument(skip(commands, token, app_id, guild))]
    pub async fn register(
        commands: &mut [Self],
        token: &str,
        app_id: u64,
        guild: &Guild,
    ) -> anyhow::Result<()> {
        let mut headers: Vec<(&'static str, Cow<'static, str>)> = Vec::new();

        headers.push(("Authorization", format!("Bot {}", token).into()));
        headers.push(("Content-Type", "application/json".into()));

        let agent = ureq::builder()
            .user_agent(concat!(
                env!("CARGO_PKG_NAME"),
                "/",
                env!("CARGO_PKG_VERSION"),
            ))
            .build();

        Self::upload_commands(&agent, &headers, commands, app_id, guild).await?;
        Self::set_permissions(&agent, &headers, commands, app_id, guild).await?;

        Ok(())
    }

    #[instrument(skip(agent, headers, commands, app_id, guild))]
    async fn upload_commands(
        agent: &ureq::Agent,
        headers: &[(&'static str, Cow<'static, str>)],
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

        // tracing::info!(?agent, ?headers, "config: {}", config);

        let mut request = agent.put(&path);

        for (key, value) in headers {
            request = request.set(key, value);
        }

        let response = request.send_json(config);

        let registered_commands: InteractionRegistrationResult =
            get_response_or_error(response).context(here!())?;

        let registered_commands = match registered_commands {
            InteractionRegistrationResult::Success(c) => c,
            InteractionRegistrationResult::Error(e) => {
                return Err(e.print_error(commands));
            }
        };

        for cmd in registered_commands {
            if let Some(c) = commands.iter_mut().find(|c| c.name == cmd.name) {
                c.command = Some(cmd);
            }
        }

        Ok(())
    }

    #[instrument(skip(agent, headers, commands, app_id, guild))]
    async fn set_permissions(
        agent: &ureq::Agent,
        headers: &[(&'static str, Cow<'static, str>)],
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

        let mut request = agent.put(&path);

        for (key, value) in headers {
            request = request.set(key, value);
        }

        let response = request.send_json(permissions)?;

        match response.status() {
            200..=299 => (),
            400..=499 | 500..=599 => {
                error!("{}", response.status_text());
                return Err(ureq::Error::Status(response.status(), response).into());
            }
            _ => {
                warn!("{}", response.status_text());
                return Err(ureq::Error::Status(response.status(), response).into());
            }
        }

        Ok(())
    }

    #[instrument(skip(ctx))]
    pub async fn check_rate_limit(
        &self,
        ctx: &Ctx,
        request: &ApplicationCommandInteraction,
    ) -> anyhow::Result<bool> {
        if let Some(rate_limit) = &self.options.rate_limit {
            match rate_limit.grouping {
                RateLimitGrouping::Everyone => {
                    let mut usage = self.global_rate_limits.write()
                        .instrument(info_span!("Waiting for rate limit access.", rate_limit_type = ?rate_limit.grouping))
                        .await;

                    match Self::within_rate_limit(rate_limit, usage.0, usage.1) {
                        Ok((count, interval_start)) => *usage = (count, interval_start),
                        Err(msg) => {
                            self.send_error_message(ctx, request, &msg).await?;
                            return Ok(false);
                        }
                    }
                }
                RateLimitGrouping::User => {
                    let mut usage = self.user_rate_limits.write()
                        .instrument(info_span!("Waiting for rate limit access.", rate_limit_type = ?rate_limit.grouping))
                        .await;
                    let now = Utc::now();

                    let user_id = request.member.as_ref().unwrap().user.id;

                    match usage.get(&user_id) {
                        Some((count, interval_start)) => {
                            match Self::within_rate_limit(rate_limit, *count, *interval_start) {
                                Ok((c, d)) => {
                                    usage.insert(user_id, (c, d));
                                }
                                Err(msg) => {
                                    self.send_error_message(ctx, request, &msg).await?;
                                    return Ok(false);
                                }
                            }
                        }
                        None => {
                            usage.insert(user_id, (1, now));
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
        request: &ApplicationCommandInteraction,
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
    pub function: fn(&Ctx, &ApplicationCommandInteraction, &RegisteredInteraction) -> bool,
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
