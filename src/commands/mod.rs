use std::fmt;

use futures::future::BoxFuture;
use serenity::{
    client::Context,
    model::{interactions::Interaction, Permissions},
};

pub mod live;
pub mod ogey;
pub mod upcoming;

pub type CheckFunction = for<'fut> fn(
    &'fut Context,
    &'fut Interaction,
    &'fut CommandOptions,
) -> BoxFuture<'fut, Result<(), Reason>>;

pub type CommandResult<T = ()> = anyhow::Result<T>;
pub type CommandFn =
    for<'fut> fn(&'fut Context, &'fut Interaction) -> BoxFuture<'fut, CommandResult>;

pub struct Command {
    pub fun: CommandFn,
    pub options: &'static CommandOptions,
}

#[derive(Debug, Default, PartialEq)]
pub struct CommandOptions {
    /// A set of checks to be called prior to executing the command. The checks
    /// will short-circuit on the first check that returns `false`.
    pub checks: &'static [&'static Check],
    /// Ratelimit bucket.
    pub bucket: Option<&'static str>,
    /// Roles allowed to use this command.
    pub allowed_roles: &'static [&'static str],
    /// Permissions required to use this command.
    pub required_permissions: Permissions,
    /// Whether the command can only be used by owners or not.
    pub owners_only: bool,
    /// Whether the command treats owners as normal users.
    pub owner_privilege: bool,
}

pub struct Check {
    /// Name listed in help-system.
    pub name: &'static str,
    /// Function that will be executed.
    pub function: CheckFunction,
    /// Whether a check should be evaluated in the help-system.
    /// `false` will ignore check and won't fail execution.
    pub check_in_help: bool,
    /// Whether a check shall be listed in the help-system.
    /// `false` won't affect whether the check will be evaluated help,
    /// solely `check_in_help` sets this.
    pub display_in_help: bool,
}

impl fmt::Debug for Check {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Check")
            .field("name", &self.name)
            .field("function", &"<fn>")
            .field("check_in_help", &self.check_in_help)
            .field("display_in_help", &self.display_in_help)
            .finish()
    }
}

impl PartialEq for Check {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum Reason {
    /// No information on the failure.
    Unknown,
    /// Information dedicated to the user.
    User(String),
    /// Information purely for logging purposes.
    Log(String),
    /// Information for the user but also for logging purposes.
    UserAndLog { user: String, log: String },
}
