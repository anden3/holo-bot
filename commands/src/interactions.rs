use std::{collections::HashSet, fmt};

use anyhow::Context;
use futures::future::BoxFuture;
use log::error;
use reqwest::{header, Client, Url};
use serde::Serialize;
use serde_json::{json, Value};
use serde_with::{serde_as, DisplayFromStr};
use serenity::model::{
    guild::Guild,
    id::RoleId,
    interactions::{ApplicationCommand, Interaction},
};

type Ctx = serenity::client::Context;
use utility::here;

pub type CheckFunction =
    for<'fut> fn(
        &'fut Ctx,
        &'fut Interaction,
        &'fut RegisteredInteraction,
    ) -> BoxFuture<'fut, Result<(), serenity::framework::standard::Reason>>;

pub type InteractionFn =
    for<'fut> fn(&'fut Ctx, &'fut Interaction) -> BoxFuture<'fut, anyhow::Result<()>>;

#[derive(Clone)]
pub struct RegisteredInteraction {
    pub command: Option<ApplicationCommand>,
    pub name: &'static str,
    pub func: InteractionFn,
    pub options: InteractionOptions,
    pub config_json: bytes::Bytes,
}

impl RegisteredInteraction {
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
}

impl std::fmt::Debug for RegisteredInteraction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}

#[derive(Debug)]
pub struct InteractionGroup {
    pub name: &'static str,
}

#[derive(Debug, Clone, Default)]
pub struct InteractionOptions {
    pub checks: &'static [Check],
    pub allowed_roles: HashSet<RoleId>,
    pub owners_only: bool,
    pub permissions: Vec<InteractionPermission>,
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
