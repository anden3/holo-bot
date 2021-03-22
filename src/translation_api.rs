use reqwest::{header, Client};
use serde::Deserialize;
use serde_json::json;

use super::config::Config;

pub struct TranslationAPI {
    client: Client,
}

impl TranslationAPI {
    pub fn new(config: &Config) -> Self {
        let mut headers = header::HeaderMap::new();

        let mut auth_val =
            header::HeaderValue::from_str(&format!("{}", &config.azure_key)).unwrap();
        auth_val.set_sensitive(true);

        headers.insert("Ocp-Apim-Subscription-Key", auth_val);

        let client = reqwest::ClientBuilder::new()
            .user_agent(concat!(
                env!("CARGO_PKG_NAME"),
                "/",
                env!("CARGO_PKG_VERSION"),
            ))
            .default_headers(headers)
            .build()
            .unwrap();

        TranslationAPI { client }
    }

    pub async fn translate(&self, text: &str, from: &str) -> Result<String, String> {
        let data = json!([{ "Text": &text }]);

        let response = self
            .client
            .post("https://api.cognitive.microsofttranslator.com/translate")
            .query(&[("api-version", "3.0"), ("to", "en"), ("from", from)])
            .header(header::CONTENT_TYPE, "application/json; charset=UTF-8")
            .header(header::CONTENT_LENGTH, data.to_string().len())
            .json(&data)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        let response_bytes = response.bytes().await.unwrap();
        println!("{}", std::str::from_utf8(&response_bytes).unwrap());
        let response: TLResponse = serde_json::from_slice(&response_bytes).unwrap();

        println!("{:#?}", response);

        match response {
            TLResponse::Result(result) => {
                if result.is_empty() || result[0].translations.is_empty() {
                    return Err("[TL] ERROR: Did not receive translation.".to_string());
                }

                Ok(result[0].translations[0].text.clone())
            }
            TLResponse::Error(e) => Err(format!(
                "[TL] ERROR: Code: {}, Message: '{}'.",
                e.error.code, e.error.message
            )),
        }
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
