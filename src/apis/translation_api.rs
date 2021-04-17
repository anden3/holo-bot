use std::collections::HashMap;

use anyhow::anyhow;
use async_trait::async_trait;
use deepl_api::{DeepL, TranslatableTextList};
use libretranslate::{translate, Language};
use log::info;
use reqwest::{header, Client};
use serde::Deserialize;
use serde_json::json;
use strum::IntoEnumIterator;
use strum_macros::EnumIter;

use crate::config::Config;

#[allow(dead_code)]
#[derive(Copy, Clone, Eq, PartialEq, Hash, EnumIter)]
pub enum TranslatorType {
    Azure,
    DeepL,
    Libre,
}

pub struct TranslationApi {
    translators: HashMap<TranslatorType, Box<dyn Translator + 'static>>,
}

impl TranslationApi {
    pub fn new(config: &Config) -> anyhow::Result<Self> {
        let mut translators: HashMap<TranslatorType, Box<dyn Translator + 'static>> =
            HashMap::new();

        for translator in TranslatorType::iter() {
            translators.insert(
                translator,
                match translator {
                    TranslatorType::Azure => Box::new(AzureApi { client: None }),
                    TranslatorType::DeepL => Box::new(DeepLApi { client: None }),
                    TranslatorType::Libre => Box::new(LibreApi {}),
                },
            );

            translators
                .get_mut(&translator)
                .ok_or_else(|| anyhow!("Couldn't access translator!"))?
                .initialize(config)?;
        }

        Ok(Self { translators })
    }

    #[must_use]
    #[allow(clippy::indexing_slicing)]
    pub fn get_translator_for_lang(&self, lang: &str) -> &(dyn Translator + 'static) {
        let best_api = match lang {
            "ja" | "jp" | "de" => TranslatorType::Libre,
            _ => TranslatorType::Azure,
        };

        self.translators[&best_api].as_ref()
    }
}

#[async_trait]
pub trait Translator: Send + Sync {
    fn initialize(&mut self, config: &Config) -> anyhow::Result<()>;
    async fn translate(&self, text: &str, from: &str) -> anyhow::Result<String>;
}

struct AzureApi {
    client: Option<Client>,
}

#[async_trait]
impl Translator for AzureApi {
    fn initialize(&mut self, config: &Config) -> anyhow::Result<()> {
        let mut headers = header::HeaderMap::new();

        let mut auth_val = header::HeaderValue::from_str(&config.azure_key.clone())?;
        auth_val.set_sensitive(true);

        headers.insert("Ocp-Apim-Subscription-Key", auth_val);

        self.client = Some(
            reqwest::ClientBuilder::new()
                .user_agent(concat!(
                    env!("CARGO_PKG_NAME"),
                    "/",
                    env!("CARGO_PKG_VERSION"),
                ))
                .default_headers(headers)
                .build()?,
        );

        Ok(())
    }

    async fn translate(&self, text: &str, from: &str) -> anyhow::Result<String> {
        let data = json!([{ "Text": &text }]);
        let src_lang = match from {
            "jp" => "ja",
            "in" => "id",
            "und" => {
                return Err(anyhow!("[AZURE] Invalid source language."));
            }
            _ => from,
        };

        if let Some(client) = &self.client {
            let response = client
                .post("https://api.cognitive.microsofttranslator.com/translate")
                .query(&[("api-version", "3.0"), ("to", "en"), ("from", src_lang)])
                .header(header::CONTENT_TYPE, "application/json; charset=UTF-8")
                .header(header::CONTENT_LENGTH, data.to_string().len())
                .json(&data)
                .send()
                .await?;

            let response_bytes = response.bytes().await?;
            let response: TlResponse = serde_json::from_slice(&response_bytes)?;

            match response {
                TlResponse::Result(result) => match &result[..] {
                    [tl, ..] => match &tl.translations[..] {
                        [t, ..] => Ok(t.text.clone()),
                        [] => Err(anyhow!("[AZURE] Did not receive translation.")),
                    },
                    [] => Err(anyhow!("[AZURE] Did not receive translation.")),
                },
                TlResponse::Error(e) => Err(anyhow!(
                    "Code: {}, Message: '{}'.",
                    e.error.code,
                    e.error.message
                )),
            }
        } else {
            Err(anyhow!(
                "[AZURE] Attempting to use translator before initializing client."
            ))
        }
    }
}

struct DeepLApi {
    client: Option<DeepL>,
}

#[async_trait]
impl Translator for DeepLApi {
    fn initialize(&mut self, config: &Config) -> anyhow::Result<()> {
        self.client = Some(DeepL::new(config.deepl_key.clone()));

        Ok(())
    }

    #[allow(clippy::cast_precision_loss)]
    async fn translate(&self, text: &str, from: &str) -> anyhow::Result<String> {
        if let Some(client) = &self.client {
            let src_lang = match from {
                "ja" | "jp" => "JA",
                "de" => "DE",
                _ => return Err(anyhow!("[DEEPL] Invalid source language.")),
            };

            let text_list = TranslatableTextList {
                source_language: Some(src_lang.to_owned()),
                target_language: "EN-US".to_owned(),
                texts: vec![text.to_owned()],
            };

            let result = client
                .translate(None, text_list)
                .map_err(|e| anyhow!("{}", e))?;

            let usage = client.usage_information().map_err(|e| anyhow!("{}", e))?;

            info!(
                "[DEEPL] Translated {} of {} ({:.1}%) characters this month.",
                usage.character_count,
                usage.character_limit,
                (usage.character_count as f32 / usage.character_limit as f32) * 100.0
            );

            match &result[..] {
                [tl, ..] => Ok(tl.text.clone()),
                [] => Err(anyhow!("[DEEPL] Translated text wasn't found.")),
            }
        } else {
            Err(anyhow!(
                "[DEEPL] Attempting to use translator before initializing client."
            ))
        }
    }
}

struct LibreApi {}

#[async_trait]
impl Translator for LibreApi {
    fn initialize(&mut self, _config: &Config) -> anyhow::Result<()> {
        Ok(())
    }

    async fn translate(&self, text: &str, from: &str) -> anyhow::Result<String> {
        let src_lang = from.parse::<Language>()?;
        let data = translate(Some(src_lang), Language::English, text)?;

        Ok(data.output)
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum TlResponse {
    Result(Vec<TlResult>),
    Error(ApiErrorResponse),
}

#[derive(Debug, Deserialize)]
struct TlResult {
    translations: Vec<Translation>,
}

#[derive(Debug, Deserialize)]
struct Translation {
    to: String,
    text: String,
}

#[derive(Debug, Deserialize)]
struct ApiErrorResponse {
    error: ApiError,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct ApiError {
    code: u32,
    message: String,
}
