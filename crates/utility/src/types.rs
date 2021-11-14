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
