use std::fmt;

use futures::future::BoxFuture;
use serenity::{
    client::Context,
    model::{
        guild::Guild,
        interactions::{ApplicationCommand, Interaction},
        Permissions,
    },
};

pub type CheckFunction =
    for<'fut> fn(
        &'fut Context,
        &'fut Interaction,
        &'fut SlashCommandOptions,
    ) -> BoxFuture<'fut, Result<(), serenity::framework::standard::Reason>>;

pub type SlashCommandResult<T = ()> = anyhow::Result<T>;
pub type SlashCommandSetupResult = anyhow::Result<ApplicationCommand>;

pub type SlashCommandFn =
    for<'fut> fn(&'fut Context, &'fut Interaction) -> BoxFuture<'fut, SlashCommandResult>;
pub type SlashCommandSetupFn =
    for<'fut> fn(&'fut Context, &'fut Guild, u64) -> BoxFuture<'fut, SlashCommandSetupResult>;

pub struct SlashCommand {
    pub name: &'static str,
    pub fun: SlashCommandFn,
    pub setup: SlashCommandSetupFn,
    pub options: &'static SlashCommandOptions,
}

impl fmt::Debug for SlashCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Command")
            .field("options", &self.options)
            .finish()
    }
}

impl PartialEq for SlashCommand {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        (self.fun as usize == other.fun as usize) && (self.options == other.options)
    }
}

#[derive(Debug, PartialEq)]
pub struct SlashCommandGroup {
    pub name: &'static str,
    pub options: &'static SlashGroupOptions,
}

#[derive(Debug, Default, PartialEq)]
pub struct SlashCommandOptions {
    pub checks: &'static [&'static Check],
    pub allowed_roles: &'static [&'static str],
    pub required_permissions: Permissions,
    pub owners_only: bool,
    pub owner_privilege: bool,
}

#[derive(Debug, Default, PartialEq)]
pub struct SlashGroupOptions {
    pub owners_only: bool,
    pub owner_privilege: bool,
    pub allowed_roles: &'static [&'static str],
    pub required_permissions: Permissions,
    pub checks: &'static [&'static Check],
    pub default_command: Option<&'static SlashCommand>,
    pub commands: &'static [&'static SlashCommand],
    pub sub_groups: &'static [&'static SlashCommandGroup],
}

pub struct Check {
    pub name: &'static str,
    pub function: CheckFunction,
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
