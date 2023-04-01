/* use async_sse::{decode, Event};
use futures::{future, Stream, TryStreamExt}; */
use isolang::Language as LanguageCode;
use miette::IntoDiagnostic;

use crate::util::validate_response;

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

    pub fn translations_for_video(
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

        let response = request.call();
        validate_response(response).into_diagnostic()
    }

    /* pub async fn translation_stream(
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

        let reader = response.into_reader();
        let mut reader = std::io::BufReader::new(reader);
        let mut reader = tokio::io::BufReader::new(reader);

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
    } */

    pub fn translators(&self) -> miette::Result<Vec<Translator>> {
        let response = self
            .http
            .get(&format!("{}/translators/registered", Self::ENDPOINT))
            .call();

        let translators: Vec<Translator> = validate_response(response).into_diagnostic()?;
        Ok(translators)
    }

    pub fn translator(&self, translator_id: &TranslatorId) -> miette::Result<Translator> {
        let response = self
            .http
            .get(&format!("{}/translators/{}", Self::ENDPOINT, translator_id))
            .call();

        let translator: Translator = validate_response(response).into_diagnostic()?;
        Ok(translator)
    }
}

#[cfg(test)]
mod tests {
    /* use tracing_test::traced_test; */

    #[test]
    #[ignore]
    fn test_video_translations() {
        use super::*;
        let client = Client::new();
        let video_id = "IhiievWaZMI".into();
        let language_code = LanguageCode::from_639_1("en").unwrap();

        let translations = client
            .translations_for_video(&video_id, &language_code, &Default::default())
            .unwrap();

        for translation in translations {
            println!("{}", translation);
        }
    }

    /* #[traced_test]
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
    } */
}
