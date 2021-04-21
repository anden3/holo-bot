use std::fmt;

use futures::future::BoxFuture;
use regex::Regex;
use serenity::{
    client::Context,
    model::{
        guild::Guild,
        interactions::{ApplicationCommand, Interaction},
        Permissions,
    },
};
use validator::Validate;

pub type CheckFunction =
    for<'fut> fn(
        &'fut Context,
        &'fut Interaction,
        &'fut InteractionOptions,
    ) -> BoxFuture<'fut, Result<(), serenity::framework::standard::Reason>>;

pub type InteractionResult<T = ()> = anyhow::Result<T>;
pub type InteractionSetupResult = anyhow::Result<ApplicationCommand>;

pub type InteractionFn =
    for<'fut> fn(&'fut Context, &'fut Interaction) -> BoxFuture<'fut, InteractionResult>;
pub type InteractionSetupFn =
    for<'fut> fn(&'fut Context, &'fut Guild, u64) -> BoxFuture<'fut, InteractionSetupResult>;

lazy_static::lazy_static! {
    static ref INTERACTION_NAME_VALIDATION: Regex = Regex::new(r#"^[\w-]{1,32}$"#).unwrap();
}

#[derive(Debug, Validate)]
pub struct InteractionSetup {
    #[validate(length(min = 1, max = 32), regex = "INTERACTION_NAME_VALIDATION")]
    name: String,
    #[validate(length(min = 1, max = 100))]
    description: String,
    #[validate]
    options: Vec<InteractionSetupOption>,
}

#[derive(Debug, Validate)]
pub struct InteractionSetupOption {
    r#type: InteractionOptionType,
    #[validate(length(min = 1, max = 32), regex = "INTERACTION_NAME_VALIDATION")]
    name: String,
    #[validate(length(min = 1, max = 100))]
    description: String,
    required: Option<bool>,
    #[validate]
    choices: Option<Vec<InteractionSetupOptionChoice>>,
    #[validate]
    options: Option<Vec<InteractionSetupOption>>,
}

#[derive(Debug)]
pub enum InteractionOptionType {
    SubCommand = 1,
    SubCommandGroup = 2,
    String = 3,
    Integer = 4,
    Boolean = 5,
    User = 6,
    Channel = 7,
    Role = 8,
}

#[derive(Debug, Validate)]
pub struct InteractionSetupOptionChoice {
    #[validate(length(min = 1, max = 100))]
    name: String,
    value: InteractionOptionChoiceValue,
}

#[derive(Debug)]
pub enum InteractionOptionChoiceValue {
    Number(i64),
    String(String),
}

pub struct InteractionCmd {
    pub name: &'static str,
    pub fun: InteractionFn,
    pub setup: InteractionSetupFn,
    pub options: &'static InteractionOptions,
}

impl fmt::Debug for InteractionCmd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Command")
            .field("options", &self.options)
            .finish()
    }
}

impl PartialEq for InteractionCmd {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        (self.fun as usize == other.fun as usize) && (self.options == other.options)
    }
}

#[derive(Debug, PartialEq)]
pub struct InteractionGroup {
    pub name: &'static str,
    pub options: &'static InteractionGroupOptions,
}

#[derive(Debug, Default, PartialEq)]
pub struct InteractionOptions {
    pub checks: &'static [&'static Check],
    pub allowed_roles: &'static [&'static str],
    pub required_permissions: Permissions,
    pub owners_only: bool,
    pub owner_privilege: bool,
}

#[derive(Debug, Default, PartialEq)]
pub struct InteractionGroupOptions {
    pub owners_only: bool,
    pub owner_privilege: bool,
    pub allowed_roles: &'static [&'static str],
    pub required_permissions: Permissions,
    pub checks: &'static [&'static Check],
    pub default_command: Option<&'static InteractionCmd>,
    pub commands: &'static [&'static InteractionCmd],
    pub sub_groups: &'static [&'static InteractionGroup],
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
