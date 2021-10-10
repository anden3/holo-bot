use std::collections::HashMap;

use anyhow::{anyhow, Context};
use async_trait::async_trait;
use deepl_api::{DeepL, TranslatableTextList};
use libretranslate::{translate, Language};
use reqwest::{header, Client};
use serde::Deserialize;
use serde_json::json;
use tracing::{info, instrument};

use utility::{config::TranslatorConfig, here, types::TranslatorType};

pub struct TranslationApi {
    translators: HashMap<TranslatorType, Box<dyn Translator + 'static>>,
    languages: HashMap<String, TranslatorType>,
    default_translator: Option<TranslatorType>,
}

impl std::fmt::Debug for TranslationApi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:?}",
            self.translators
                .iter()
                .map(|(ty, _)| ty)
                .collect::<Vec<_>>()
        )
    }
}

impl TranslationApi {
    pub fn new(config: &HashMap<TranslatorType, TranslatorConfig>) -> anyhow::Result<Self> {
        let mut translators: HashMap<TranslatorType, Box<dyn Translator + 'static>> =
            HashMap::new();

        let mut languages: HashMap<String, TranslatorType> = HashMap::new();
        let mut default_translator = None;

        for (translator_type, conf) in config {
            let mut translator: Box<dyn Translator + 'static> = match translator_type {
                TranslatorType::DeepL => Box::new(DeepLApi::default()),
                TranslatorType::Azure => Box::new(AzureApi::default()),
                TranslatorType::Libre => Box::new(LibreApi::default()),
            };

            translator.initialize(conf).context(here!())?;
            translators.insert(*translator_type, translator);

            if conf.languages.is_empty() && default_translator.is_none() {
                default_translator = Some(*translator_type);
            }

            for lang in conf.languages.iter() {
                languages
                    .entry(lang.to_string())
                    .or_insert(*translator_type);
            }
        }

        Ok(Self {
            translators,
            languages,
            default_translator,
        })
    }

    #[must_use]
    #[allow(clippy::indexing_slicing)]
    pub fn get_translator_for_lang(&self, lang: &str) -> Option<&(dyn Translator + 'static)> {
        if let Some(translator) = self.languages.get(lang) {
            Some(self.translators.get(translator).unwrap().as_ref())
        } else if let Some(def) = self.default_translator {
            Some(self.translators.get(&def).unwrap().as_ref())
        } else {
            None
        }
    }
}

#[async_trait]
pub trait Translator: Send + Sync {
    fn initialize(&mut self, config: &TranslatorConfig) -> anyhow::Result<()>;
    async fn translate(&self, text: &str, from: &str) -> anyhow::Result<String>;
}

#[derive(Debug, Default)]
struct AzureApi {
    client: Option<Client>,
}

#[async_trait]
impl Translator for AzureApi {
    fn initialize(&mut self, config: &TranslatorConfig) -> anyhow::Result<()> {
        let mut headers = header::HeaderMap::new();

        let mut auth_val = header::HeaderValue::from_str(&config.token.clone()).context(here!())?;
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
                .context(here!())?,
        );

        Ok(())
    }

    #[instrument]
    async fn translate(&self, text: &str, from: &str) -> anyhow::Result<String> {
        let data = json!([{ "Text": &text }]);
        let src_lang = match from {
            "jp" => "ja",
            "in" => "id",
            "und" => {
                return Err(anyhow!("Invalid source language.").context(here!()));
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
                .context(here!())?;

            let response_bytes = response.bytes().await.context(here!())?;
            let response: TlResponse = serde_json::from_slice(&response_bytes).context(here!())?;

            match response {
                TlResponse::Result(result) => match &result[..] {
                    [tl, ..] => match &tl.translations[..] {
                        [t, ..] => Ok(t.text.clone()),
                        [] => Err(anyhow!("Did not receive translation.").context(here!())),
                    },
                    [] => Err(anyhow!("Did not receive translation.").context(here!())),
                },
                TlResponse::Error(e) => {
                    Err(
                        anyhow!("Code: {}, Message: '{}'.", e.error.code, e.error.message)
                            .context(here!()),
                    )
                }
            }
        } else {
            Err(
                anyhow!("Attempting to use translator before initializing client.")
                    .context(here!()),
            )
        }
    }
}

#[derive(Default)]
struct DeepLApi {
    client: Option<DeepL>,
}

#[async_trait]
impl Translator for DeepLApi {
    fn initialize(&mut self, config: &TranslatorConfig) -> anyhow::Result<()> {
        self.client = Some(DeepL::new(config.token.clone()));

        Ok(())
    }

    #[allow(clippy::cast_precision_loss)]
    #[instrument(skip(self))]
    async fn translate(&self, text: &str, from: &str) -> anyhow::Result<String> {
        if let Some(client) = &self.client {
            let src_lang = match from {
                "ja" | "jp" => "JA",
                "de" => "DE",
                _ => return Err(anyhow!("Invalid source language.").context(here!())),
            };

            let usage = client
                .usage_information()
                .map_err(|e| anyhow!("{}", e))
                .context(here!())?;

            if usage.character_count > usage.character_limit {
                return Err(anyhow!("Character usage has reached its limit this month."));
            }

            let text_list = TranslatableTextList {
                source_language: Some(src_lang.to_owned()),
                target_language: "EN-US".to_owned(),
                texts: vec![text.to_owned()],
            };

            let result = client
                .translate(None, text_list)
                .map_err(|e| anyhow!("{}", e))
                .context(here!())?;

            info!(
                "Translated {} of {} ({:.1}%) characters this month.",
                usage.character_count,
                usage.character_limit,
                (usage.character_count as f32 / usage.character_limit as f32) * 100.0
            );

            match &result[..] {
                [tl, ..] => Ok(tl.text.clone()),
                [] => Err(anyhow!("Translated text wasn't found.").context(here!())),
            }
        } else {
            Err(
                anyhow!("Attempting to use translator before initializing client.")
                    .context(here!()),
            )
        }
    }
}

#[derive(Debug, Default)]
struct LibreApi;

#[async_trait]
impl Translator for LibreApi {
    fn initialize(&mut self, _config: &TranslatorConfig) -> anyhow::Result<()> {
        Ok(())
    }

    #[instrument]
    async fn translate(&self, text: &str, from: &str) -> anyhow::Result<String> {
        let src_lang = from.parse::<Language>()?;
        let data = translate(src_lang, Language::English, text, None).await?;

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

#[allow(dead_code)]
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
