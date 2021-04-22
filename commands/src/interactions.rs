use std::fmt;

use futures::future::BoxFuture;
use serenity::{
    client::Context,
    model::interactions::{ApplicationCommand, Interaction},
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
    pub command: ApplicationCommand,
    pub name: &'static str,
    pub fun: InteractionFn,
    pub options: &'static InteractionOptions,
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

#[derive(Debug, Default)]
pub struct InteractionOptions {
    pub checks: &'static [Check],
    pub allowed_roles: &'static [&'static str],
    pub owners_only: bool,
}

pub struct Check {
    pub name: &'static str,
    // pub function: CheckFunction,
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
