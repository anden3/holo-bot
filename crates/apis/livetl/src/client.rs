use async_sse::{decode, Event};
use futures::{future, Stream, TryStreamExt};
use isolang::Language as LanguageCode;
use miette::IntoDiagnostic;

use crate::util::{validate_json_bytes, validate_response};

use super::types::*;

pub struct Client {
    http: ureq::Agent,
}

impl Client {
    const ENDPOINT: &'static str = "https://api.livetl.app";
    const USER_AGENT: &'static str =
        concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"),);

    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let http = ureq::builder().user_agent(Self::USER_AGENT).build();

        Client { http }
    }

    pub async fn translations_for_video(
        &self,
        video_id: &VideoId,
        language_code: &LanguageCode,
        filter: &TranslationFilter,
    ) -> miette::Result<Vec<Translation>> {
        let query_string = serde_urlencoded::to_string(filter).into_diagnostic()?;
        let query_pairs: Vec<(&str, &str)> =
            serde_urlencoded::from_str(&query_string).into_diagnostic()?;

        let mut request = self.http.get(&format!(
            "{}/translations/{}/{}",
            Self::ENDPOINT,
            video_id,
            language_code.to_639_1().unwrap()
        ));

        for (key, value) in query_pairs {
            request = request.query(key, value);
        }

        let response = request.call().into_diagnostic()?;

        Ok(validate_response(response).await?)
    }

    pub async fn translation_stream(
        &self,
        video_id: &VideoId,
        language_code: &LanguageCode,
    ) -> miette::Result<impl Stream<Item = miette::Result<Translation>>> {
        let response = self
            .http
            .get(&format!(
                "{}/notifications/translations?videoId={}&languageCode={}",
                Self::ENDPOINT,
                video_id,
                language_code.to_639_1().unwrap(),
            ))
            .call()
            .into_diagnostic()?;

        let reader = response
            .bytes_stream()
            .map_err(|e| futures::io::Error::new(futures::io::ErrorKind::Other, e))
            .into_async_read();

        let stream = decode(reader)
            .map_err(|e| miette::miette!(e.to_string()))
            .try_filter_map(|e| {
                future::ready({
                    match e {
                        Event::Message(m) => {
                            match validate_json_bytes::<Translation>(m.data()).into_diagnostic() {
                                Ok(t) => Ok(Some(t)),
                                Err(e) => Err(e),
                            }
                        }
                        _ => Ok(None),
                    }
                })
            });
        Ok(stream)
    }

    pub async fn translators(&self) -> miette::Result<Vec<Translator>> {
        let response = self
            .http
            .get(&format!("{}/translators/registered", Self::ENDPOINT))
            .call()
            .into_diagnostic()?;

        let translators: Vec<Translator> = validate_response(response).await?;
        Ok(translators)
    }

    pub async fn translator(&self, translator_id: &TranslatorId) -> miette::Result<Translator> {
        let response = self
            .http
            .get(&format!("{}/translators/{}", Self::ENDPOINT, translator_id))
            .call()
            .into_diagnostic()?;

        let translator: Translator = validate_response(response).await?;
        Ok(translator)
    }
}

#[cfg(test)]
mod tests {
    use tracing_test::traced_test;

    #[tokio::test]
    #[ignore]
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
