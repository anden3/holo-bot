use serde::Deserialize;

#[derive(Deserialize)]
pub(crate) struct Room {
    #[serde(rename = "Nick")]
    pub name: String,
    #[serde(rename = "EntryPass")]
    pub needs_password: bool,
    #[serde(rename = "Empty")]
    pub is_empty: bool,
    #[serde(rename = "StreamLink")]
    pub stream: Option<String>,
    #[serde(rename = "Tags")]
    #[serde(default)]
    pub tags: Vec<String>,
}
