use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::{anyhow, Context};
use once_cell::sync::OnceCell;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr};
use serenity::prelude::TypeMapKey;
use strum_macros::{EnumIter, EnumString, ToString};
use tokio::sync::RwLock;
use tracing::{info, instrument};

use utility::{config::Config, here};

pub type MemeCache = Arc<RwLock<Vec<Meme>>>;

static CACHE: OnceCell<MemeCache> = OnceCell::new();
static LAST_CACHE_UPDATE: OnceCell<RwLock<SystemTime>> = OnceCell::new();

#[derive(Debug, Clone)]
pub struct MemeApi {
    client: Client,
    username: String,
    password: String,
}

impl MemeApi {
    const CACHE_EXPIRATION_TIME: Duration = Duration::from_secs(60 * 60 * 24);

    pub fn new(config: &Config) -> anyhow::Result<Self> {
        let client = Client::builder()
            .user_agent(concat!(
                env!("CARGO_PKG_NAME"),
                "/",
                env!("CARGO_PKG_VERSION"),
            ))
            .build()
            .context(here!())?;

        CACHE.get_or_init(|| Arc::new(RwLock::new(Vec::with_capacity(100))));
        LAST_CACHE_UPDATE.get_or_init(|| RwLock::new(SystemTime::now()));

        Ok(Self {
            client,
            username: config.imgflip_user.clone(),
            password: config.imgflip_pass.clone(),
        })
    }

    #[instrument]
    pub async fn get_popular_memes(&self) -> anyhow::Result<MemeCache> {
        let mut last_update = LAST_CACHE_UPDATE.get().unwrap().write().await;
        let mut cache = CACHE.get().unwrap().write().await;

        if last_update.elapsed()? >= Self::CACHE_EXPIRATION_TIME {
            *last_update = SystemTime::now();
            cache.clear();
        }

        if cache.is_empty() {
            let response = self
                .client
                .get("https://api.imgflip.com/get_memes")
                .send()
                .await?;

            let response: PopularMemesResponse = response.json().await?;

            if response.success {
                match response.data {
                    Some(data) => cache.extend(data.memes),
                    None => return Err(anyhow!("URL not found!").context(here!())),
                }
            } else {
                return match response.error_message {
                    Some(err) => Err(anyhow!("{}", err).context(here!())),
                    None => Err(anyhow!("Error message not found!").context(here!())),
                };
            }
        }

        Ok(MemeCache::clone(CACHE.get().unwrap()))
    }

    #[instrument]
    pub async fn create_meme(
        &self,
        meme: &Meme,
        captions: Vec<String>,
        font: MemeFont,
        max_font_size: i64,
    ) -> anyhow::Result<String> {
        let mut query = vec![
            ("template_id", meme.id.to_string()),
            ("username", self.username.clone()),
            ("password", self.password.clone()),
            ("max_font_size", max_font_size.to_string()),
            ("font", font.to_string()),
            ("text0", captions.get(0).unwrap().to_owned()),
        ];

        if meme.box_count > 1 {
            query.extend(vec![("text1", captions.get(1).unwrap().to_owned())]);
        }

        let mut response = self
            .client
            .post("https://api.imgflip.com/caption_image")
            .query(&query);

        if meme.box_count > 2 {
            let boxes = captions
                .iter()
                .map(|c| MemeBox {
                    text: c.to_string(),
                    x: None,
                    y: None,
                    width: None,
                    height: None,
                    color: None,
                    outline_color: None,
                })
                .collect::<Vec<_>>();
            response = response.json(&boxes);
        }

        let response = response.send().await.context(here!())?;
        let response: MemeResponse = response.json().await.context(here!())?;

        if response.success {
            match response.data {
                Some(data) => {
                    info!("Meme generated: {}", data.url);
                    Ok(data.url)
                }
                None => Err(anyhow!("URL not found!").context(here!())),
            }
        } else {
            match response.error_message {
                Some(err) => Err(anyhow!("{}", err).context(here!())),
                None => Err(anyhow!("Error message not found!").context(here!())),
            }
        }
    }
}

impl TypeMapKey for MemeApi {
    type Value = Self;
}

#[serde_as]
#[derive(Debug, Deserialize)]
pub struct Meme {
    #[serde_as(as = "DisplayFromStr")]
    pub id: u64,
    pub name: String,
    pub url: String,
    pub width: u32,
    pub height: u32,
    pub box_count: usize,
}

#[derive(Debug, Serialize, EnumString, EnumIter, ToString)]
pub enum MemeFont {
    #[strum(serialize = "impact")]
    Impact,
    #[strum(serialize = "arial")]
    Arial,
}

#[derive(Serialize)]
struct MemeRequest {
    template_id: u32,
    username: String,
    password: String,

    text0: Option<String>,
    text1: Option<String>,

    font: Option<MemeFont>,
    max_font_size: Option<u32>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    boxes: Vec<MemeBox>,
}

#[derive(Deserialize)]
struct MemeResponse {
    success: bool,
    data: Option<CreatedMeme>,
    error_message: Option<String>,
}

#[derive(Deserialize)]
struct PopularMemesResponse {
    success: bool,
    data: Option<MemeList>,
    error_message: Option<String>,
}

#[derive(Deserialize)]
struct MemeList {
    #[serde(default = "Vec::new")]
    memes: Vec<Meme>,
}

#[derive(Deserialize)]
struct CreatedMeme {
    url: String,
    #[serde(rename = "page_url")]
    _page_url: String,
}

#[derive(Serialize)]
struct MemeBox {
    text: String,

    x: Option<u32>,
    y: Option<u32>,
    width: Option<u32>,
    height: Option<u32>,

    color: Option<String>,
    outline_color: Option<String>,
}
