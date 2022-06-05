use serde_with::{DeserializeFromStr, SerializeDisplay};
use strum::{Display, EnumIter, EnumString};

pub type Ctx = serenity::client::Context;

#[allow(dead_code)]
#[derive(
    Debug,
    Copy,
    Clone,
    Eq,
    PartialEq,
    Hash,
    EnumIter,
    EnumString,
    Display,
    SerializeDisplay,
    DeserializeFromStr,
)]
pub enum TranslatorType {
    /* Azure, */
    DeepL,
    /* Libre, */
}

#[derive(Debug, Copy, Clone, poise::ChoiceParameter)]
pub enum Service {
    StreamIndexer,
    TwitterFeed,
}

impl std::fmt::Display for Service {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Service::StreamIndexer => write!(f, "Stream Indexer"),
            Service::TwitterFeed => write!(f, "Twitter Feed"),
        }
    }
}
