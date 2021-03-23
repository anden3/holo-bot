use cached::proc_macro::cached;
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;

pub struct LaTeXRenderer {}

#[cached(size = 1)]
fn get_client() -> Client {
    Client::builder()
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .unwrap()
}

impl LaTeXRenderer {
    pub async fn render(expression: &str) -> Result<String, String> {
        let code = format!(
            r"\documentclass{{article}}
\usepackage{{xcolor}}
\color{{white}}
\begin{{document}}
{}
\pagenumbering{{gobble}}
\end{{document}}",
            expression
        );

        let body = json!({
            "format": "png",
            "code": code
        });

        let response = get_client()
            .post("https://rtex.probablyaweb.site/api/v2")
            .json(&body)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        let response: RenderResponse = response.json().await.map_err(|e| e.to_string())?;

        match response.status.as_str() {
            "success" => {
                let filename = response.filename.ok_or("Failed to get filename")?;
                Ok(format!(
                    "https://rtex.probablyaweb.site/api/v2/{}",
                    filename
                ))
            }
            "error" => Err(format!(
                "[LATEX] Error: {:#?}",
                response.description.unwrap()
            )),
            _ => Err(format!(
                "[LATEX] Error: Invalid response received '{}'",
                response.status
            )),
        }
    }
}

#[derive(Deserialize, Debug)]
struct RenderResponse {
    status: String,
    log: Option<String>,
    filename: Option<String>,
    description: Option<String>,
}
