use std::fmt::Display;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::{anyhow, Context};
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr};
use serenity::prelude::TypeMapKey;
use tokio::sync::RwLock;
use tracing::{info, instrument};

use utility::{config::MemeCreationConfig, here};

pub type MemeCache = Arc<RwLock<Vec<Meme>>>;

static CACHE: OnceCell<MemeCache> = OnceCell::new();
static LAST_CACHE_UPDATE: OnceCell<RwLock<SystemTime>> = OnceCell::new();

#[derive(Debug, Clone)]
pub struct MemeApi {
    agent: ureq::Agent,
    username: String,
    password: String,
}

impl MemeApi {
    const CACHE_EXPIRATION_TIME: Duration = Duration::from_secs(60 * 60 * 24);

    pub fn new(config: &MemeCreationConfig) -> anyhow::Result<Self> {
        let agent = ureq::builder()
            .user_agent(concat!(
                env!("CARGO_PKG_NAME"),
                "/",
                env!("CARGO_PKG_VERSION"),
            ))
            .build();

        CACHE.get_or_init(|| Arc::new(RwLock::new(Vec::with_capacity(100))));
        LAST_CACHE_UPDATE.get_or_init(|| RwLock::new(SystemTime::now()));

        Ok(Self {
            agent,
            username: config.imgflip_user.clone(),
            password: config.imgflip_pass.clone(),
        })
    }

    #[instrument(skip(self))]
    pub async fn get_popular_memes(&self) -> anyhow::Result<MemeCache> {
        let mut last_update = LAST_CACHE_UPDATE.get().unwrap().write().await;
        let mut cache = CACHE.get().unwrap().write().await;

        if last_update.elapsed()? >= Self::CACHE_EXPIRATION_TIME {
            *last_update = SystemTime::now();
            cache.clear();
        }

        if cache.is_empty() {
            let response = self.agent.get("https://api.imgflip.com/get_memes").call()?;
            let response: PopularMemesResponse = response.into_json()?;

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

    #[instrument(skip(self))]
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

        let mut request = self.agent.post("https://api.imgflip.com/caption_image");

        for (key, value) in query {
            request = request.query(key, &value);
        }

        let response = if meme.box_count > 2 {
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
            request
                .send_json(serde_json::to_value(boxes)?)
                .context(here!())?
        } else {
            request.call().context(here!())?
        };
        let response: MemeResponse = response.into_json().context(here!())?;

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
#[derive(Debug, Deserialize, Clone)]
pub struct Meme {
    #[serde_as(as = "DisplayFromStr")]
    pub id: u64,
    pub name: String,
    pub url: String,
    pub width: u32,
    pub height: u32,
    pub box_count: usize,
}

#[derive(Debug, Serialize, poise::ChoiceParameter)]
#[serde(rename_all = "lowercase")]
pub enum MemeFont {
    #[name = "Impact"]
    Impact,
    #[name = "Arial"]
    Arial,
}

impl Default for MemeFont {
    fn default() -> Self {
        MemeFont::Impact
    }
}

impl Display for MemeFont {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemeFont::Impact => write!(f, "impact"),
            MemeFont::Arial => write!(f, "arial"),
        }
    }
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
