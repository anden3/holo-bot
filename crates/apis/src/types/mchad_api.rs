use serde::Deserialize;
use serde_with::{serde_as, CommaSeparator, NoneAsEmptyString, StringWithSeparator};

#[serde_as]
#[derive(Debug, Clone, Deserialize)]
pub struct Room {
    #[serde(rename = "Nick")]
    pub name: String,
    #[serde(alias = "EntryPass", alias = "Entrypass")]
    pub needs_password: bool,
    #[serde(rename = "Empty")]
    pub is_empty: bool,
    #[serde(rename = "StreamLink")]
    #[serde_as(as = "NoneAsEmptyString")]
    pub stream: Option<String>,
    #[serde(rename = "Tags")]
    #[serde(default)]
    #[serde_as(as = "StringWithSeparator::<CommaSeparator, String>")]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "flag", content = "content")]
pub enum EventData {
    #[serde(alias = "connect")]
    Connect(String),
    #[serde(alias = "insert")]
    Insert {
        #[serde(rename = "_id")]
        id: String,
        #[serde(rename = "Stext")]
        text: String,
        #[serde(rename = "Stime")]
        time: u64,
    },
    #[serde(alias = "delete")]
    Delete(String),
    #[serde(alias = "update")]
    Update {
        #[serde(rename = "_id")]
        id: String,
        #[serde(rename = "Stext")]
        text: String,
        #[serde(rename = "Stime")]
        time: u64,
    },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(untagged)]
pub enum DataOrEmptyObject<T> {
    Some(T),
    None {},
}

impl<T> From<DataOrEmptyObject<T>> for Option<T> {
    fn from(empty_option: DataOrEmptyObject<T>) -> Option<T> {
        match empty_option {
            DataOrEmptyObject::Some(option) => Some(option),
            DataOrEmptyObject::None {} => None,
        }
    }
}

impl<T> From<Option<T>> for DataOrEmptyObject<T> {
    fn from(option: Option<T>) -> Self {
        match option {
            Some(option) => Self::Some(option),
            None {} => Self::None {},
        }
    }
}

impl<T> DataOrEmptyObject<T> {
    pub fn into_option(self) -> Option<T> {
        self.into()
    }
    pub fn as_option(&self) -> Option<&T> {
        match self {
            Self::Some(option) => Some(option),
            Self::None {} => None,
        }
    }
}
