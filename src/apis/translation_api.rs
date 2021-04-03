use std::collections::HashMap;

use async_trait::async_trait;
use deepl_api::{DeepL, TranslatableTextList};
use libretranslate::{translate, Language};
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

pub struct TranslationAPI {
    translators: HashMap<TranslatorType, Box<dyn Translator + 'static>>,
}

impl TranslationAPI {
    pub fn new(config: &Config) -> Self {
        let mut translators: HashMap<TranslatorType, Box<dyn Translator + 'static>> =
            HashMap::new();

        for translator in TranslatorType::iter() {
            translators.insert(
                translator,
                match &translator {
                    TranslatorType::Azure => Box::new(AzureAPI { client: None }),
                    TranslatorType::DeepL => Box::new(DeepLAPI { client: None }),
                    TranslatorType::Libre => Box::new(LibreAPI {}),
                },
            );

            translators.get_mut(&translator).unwrap().initialize(config);
        }

        TranslationAPI { translators }
    }

    pub fn get_translator_for_lang(&self, lang: &str) -> &Box<dyn Translator + 'static> {
        let best_api = match lang {
            "ja" | "jp" => TranslatorType::Libre,
            "de" => TranslatorType::Libre,
            "id" | "in" => TranslatorType::Azure,
            _ => TranslatorType::Azure,
        };

        self.translators.get(&best_api).unwrap()
    }
}

#[async_trait]
pub trait Translator: Send + Sync {
    fn initialize(&mut self, config: &Config);
    async fn translate(&self, text: &str, from: &str) -> Result<String, String>;
}

struct AzureAPI {
    client: Option<Client>,
}

#[async_trait]
impl Translator for AzureAPI {
    fn initialize(&mut self, config: &Config) {
        let mut headers = header::HeaderMap::new();

        let mut auth_val =
            header::HeaderValue::from_str(&format!("{}", &config.azure_key)).unwrap();
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
                .build()
                .unwrap(),
        );
    }

    async fn translate(&self, text: &str, from: &str) -> Result<String, String> {
        let data = json!([{ "Text": &text }]);
        let src_lang = match from {
            "jp" => "ja",
            "in" => "id",
            "und" => {
                return Err("[TL][AZURE] ERROR: Invalid source language.".to_string());
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
                .await
                .map_err(|e| e.to_string())?;

            let response_bytes = response.bytes().await.unwrap();
            let response: TLResponse = serde_json::from_slice(&response_bytes).unwrap();

            match response {
                TLResponse::Result(result) => {
                    if result.is_empty() || result[0].translations.is_empty() {
                        return Err("[TL][AZURE] ERROR: Did not receive translation.".to_string());
                    }

                    Ok(result[0].translations[0].text.clone())
                }
                TLResponse::Error(e) => Err(format!(
                    "[TL][AZURE] ERROR: Code: {}, Message: '{}'.",
                    e.error.code, e.error.message
                )),
            }
        } else {
            Err(
                "[TL][AZURE] ERROR: Attempting to use translator before initializing client."
                    .to_string(),
            )
        }
    }
}

struct DeepLAPI {
    client: Option<DeepL>,
}

#[async_trait]
impl Translator for DeepLAPI {
    fn initialize(&mut self, config: &Config) {
        self.client = Some(DeepL::new(config.deepl_key.clone()));
    }

    async fn translate(&self, text: &str, from: &str) -> Result<String, String> {
        if let Some(client) = &self.client {
            let src_lang = match from {
                "ja" => "JA",
                "jp" => "JA",
                "de" => "DE",
                _ => return Err("[TL][DEEPL] ERROR: Invalid source language.".to_string()),
            };

            let text_list = TranslatableTextList {
                source_language: Some(src_lang.to_string()),
                target_language: "EN-US".to_string(),
                texts: vec![text.to_string()],
            };

            let result = client
                .translate(None, text_list)
                .map_err(|e| e.to_string())?;

            let usage = client.usage_information().map_err(|e| e.to_string())?;
            println!(
                "[TL][DEEPL] Translated {} of {} ({:.1}%) characters this month.",
                usage.character_count,
                usage.character_limit,
                (usage.character_count as f32 / usage.character_limit as f32) * 100.0
            );

            Ok(result[0].text.clone())
        } else {
            Err(
                "[TL][DEEPL] ERROR: Attempting to use translator before initializing client."
                    .to_string(),
            )
        }
    }
}

struct LibreAPI {}

#[async_trait]
impl Translator for LibreAPI {
    fn initialize(&mut self, _config: &Config) {}

    async fn translate(&self, text: &str, from: &str) -> Result<String, String> {
        let src_lang = from.parse::<Language>().map_err(|e| e.to_string())?;
        let data = translate(Some(src_lang), Language::English, text).map_err(|e| e.to_string())?;

        Ok(data.output)
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum TLResponse {
    Result(Vec<TLResult>),
    Error(APIErrorResponse),
}

#[derive(Debug, Deserialize)]
struct TLResult {
    translations: Vec<Translation>,
}

#[derive(Debug, Deserialize)]
struct Translation {
    to: String,
    text: String,
}

#[derive(Debug, Deserialize)]
struct APIErrorResponse {
    error: APIError,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct APIError {
    code: u32,
    message: String,
}
