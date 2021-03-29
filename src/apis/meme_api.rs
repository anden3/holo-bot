use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::super::config::Config;

pub struct MemeAPI {
    client: Client,
    username: String,
    password: String,
}

impl MemeAPI {
    pub fn new(config: &Config) -> Self {
        let client = Client::builder()
            .user_agent(concat!(
                env!("CARGO_PKG_NAME"),
                "/",
                env!("CARGO_PKG_VERSION"),
            ))
            .build()
            .unwrap();

        MemeAPI {
            client,
            username: config.imgflip_user.to_string(),
            password: config.imgflip_pass.to_string(),
        }
    }

    pub async fn create_meme(&self, template: u32, captions: &[String]) -> Result<String, String> {
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

        let response = self
            .client
            .post("https://api.imgflip.com/caption_image")
            .query(&[
                ("template_id", &template.to_string()),
                ("username", &self.username),
                ("password", &self.password),
                ("text0", &captions.get(0).unwrap_or(&String::new())),
                ("text1", &captions.get(1).unwrap_or(&String::new())),
            ])
            .json(&boxes)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        let response: MemeResponse = response.json().await.map_err(|e| e.to_string())?;

        if response.success {
            Ok(response.data.unwrap().url)
        } else {
            Err(response.error_message.unwrap())
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

#[derive(Serialize)]
enum MemeFont {
    Impact,
    Arial,
}
