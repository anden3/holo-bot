use async_sse::{decode, Event};
use futures::{future, Stream, TryStreamExt};
use holodex::model::id::VideoId;
use isolang::Language as LanguageCode;
use utility::functions::{validate_json_bytes, validate_response};

use super::types::livetl::*;

pub struct Client {
    http: reqwest::Client,
}

impl Client {
    const ENDPOINT: &'static str = "https://api.livetl.app";
    const USER_AGENT: &'static str =
        concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"),);

    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let http = reqwest::ClientBuilder::new()
            .user_agent(Self::USER_AGENT)
            .build()
            .unwrap();

        Client { http }
    }

    pub async fn translations_for_video(
        &self,
        video_id: &VideoId,
        language_code: &LanguageCode,
        filter: &TranslationFilter,
    ) -> anyhow::Result<Vec<Translation>> {
        let response = self
            .http
            .get(&format!(
                "{}/translations/{}/{}",
                Self::ENDPOINT,
                video_id,
                language_code.to_639_1().unwrap()
            ))
            .query(filter)
            .send()
            .await?;

        let translations: Vec<Translation> = validate_response(response, None).await?;
        Ok(translations)
    }

    pub async fn translation_stream(
        &self,
        video_id: &VideoId,
        language_code: &LanguageCode,
    ) -> anyhow::Result<impl Stream<Item = anyhow::Result<Translation>>> {
        let response = self
            .http
            .get(&format!(
                "{}/notifications/translations?videoId={}&languageCode={}",
                Self::ENDPOINT,
                video_id,
                language_code.to_639_1().unwrap(),
            ))
            .send()
            .await?;

        let reader = response
            .bytes_stream()
            .map_err(|e| futures::io::Error::new(futures::io::ErrorKind::Other, e))
            .into_async_read();

        let stream = decode(reader)
            .map_err(|e| anyhow::Error::msg(e.to_string()))
            .try_filter_map(|e| {
                future::ready({
                    match e {
                        Event::Message(m) => match validate_json_bytes::<Translation>(m.data()) {
                            Ok(t) => Ok(Some(t)),
                            Err(e) => Err(e),
                        },
                        _ => Ok(None),
                    }
                })
            });
        Ok(stream)
    }

    pub async fn translators(&self) -> anyhow::Result<Vec<Translator>> {
        let response = self
            .http
            .get(&format!("{}/translators/registered", Self::ENDPOINT))
            .send()
            .await?;

        let translators: Vec<Translator> = validate_response(response, None).await?;
        Ok(translators)
    }

    pub async fn translator(&self, translator_id: &TranslatorId) -> anyhow::Result<Translator> {
        let response = self
            .http
            .get(&format!("{}/translators/{}", Self::ENDPOINT, translator_id))
            .send()
            .await?;

        let translator: Translator = validate_response(response, None).await?;
        Ok(translator)
    }
}

#[cfg(test)]
mod tests {
    use tracing_test::traced_test;

    #[tokio::test]
    async fn test_video_translations() {
        use super::*;
        let client = Client::new();
        let video_id = "IhiievWaZMI".into();
        let language_code = LanguageCode::from_639_1("en").unwrap();

        let translations = client
            .translations_for_video(
                &video_id,
                &language_code,
                &TranslationFilter {
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        for translation in translations {
            println!("{}", translation);
        }
    }

    #[traced_test]
    #[tokio::test]
    async fn test_translations_for_video() {
        use super::*;
        let client = Client::new();
        let video_id = "UtS5wFgSMZs".into();
        let language_code = LanguageCode::from_639_1("en").unwrap();

        let translations = client
            .translation_stream(&video_id, &language_code)
            .await
            .unwrap();

        futures::pin_mut!(translations);

        while let Some(translation) = translations.try_next().await.unwrap() {
            println!("{:?}", translation);
        }
    }
}
