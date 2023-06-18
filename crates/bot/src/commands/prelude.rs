pub use std::{collections::HashSet, iter::FromIterator};

pub use anyhow::{anyhow, Context as _};
pub use holodex::model::{id::*, VideoStatus};
pub use poise::{ApplicationCommandOrAutocompleteInteraction, AutocompleteChoice, ChoiceParameter};
pub use serenity::{
    model::{
        channel::{Channel, Message, Reaction},
        guild::Guild,
        id::{ChannelId, MessageId, RoleId, UserId},
        mention::Mention,
    },
    utils::{Colour, MessageBuilder},
};
pub use tokio_util::sync::CancellationToken;
pub use tracing::{debug, error, info, instrument, warn};

pub use utility::{config::Config, discord::*, here, regex, streams::*};

pub use crate::{
    paginated_list::{PageLayout, PaginatedList},
    DataWrapper,
};

pub type Error = anyhow::Error;
pub type Context<'a> = poise::Context<'a, DataWrapper, Error>;
pub type Command = poise::Command<DataWrapper, Error>;
