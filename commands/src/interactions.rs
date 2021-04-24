use std::{collections::HashSet, fmt};

use futures::future::BoxFuture;
use reqwest::{Client, Url};
use serenity::{
    client::Context,
    http::routing::RouteInfo,
    model::{
        guild::Guild,
        id::RoleId,
        interactions::{ApplicationCommand, Interaction},
    },
};

pub type CheckFunction =
    for<'fut> fn(
        &'fut Context,
        &'fut Interaction,
        &'fut RegisteredInteraction,
    ) -> BoxFuture<'fut, Result<(), serenity::framework::standard::Reason>>;

pub type InteractionFn =
    for<'fut> fn(&'fut Context, &'fut Interaction) -> BoxFuture<'fut, anyhow::Result<()>>;

#[derive(Clone)]
pub struct RegisteredInteraction {
    pub command: Option<ApplicationCommand>,
    pub name: &'static str,
    pub func: InteractionFn,
    pub options: InteractionOptions,
    pub config_json: bytes::Bytes,
}

impl RegisteredInteraction {
    pub async fn fetch_command(
        &mut self,
        app_id: u64,
        guild: &Guild,
        client: &Client,
    ) -> anyhow::Result<()> {
        let route = RouteInfo::CreateGuildApplicationCommand {
            application_id: app_id,
            guild_id: *guild.id.as_u64(),
        };
        let (method, _, path) = route.deconstruct();

        let response: ApplicationCommand = client
            .request(method.reqwest_method(), Url::parse(&path)?)
            .body(self.config_json.clone())
            .send()
            .await?
            .json()
            .await?;

        self.command = Some(response);
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
}

pub struct Check {
    pub name: &'static str,
    pub function: fn(&Context, &Interaction, &RegisteredInteraction) -> bool,
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
