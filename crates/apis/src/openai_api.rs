use core::f64;

use anyhow::anyhow;
use openai_api::{api, Client};
use tracing::instrument;

use tokio::sync::RwLock;
use utility::config::AiChatbotConfig;

const START_SEQUENCE: &str = "\nPekora:";
const RESTART_SEQUENCE: &str = "\n\nFan:";

const INIT_PROMPT: &str = "You are talking to Usada Pekora, VTuber, idol, and comedian who was recruited by Yagoo into generation 3 of Hololive. \
Pekora is Japanese, but speaks a little bit of English. \
Pekora is famous on YouTube, mainly for her Minecraft videos. She is best friends with Moona Hoshinova, who is also from Hololive. \
You can ask her anything you want and will get a witty answer.

Fan: Who are you?
Pekora: I am Pekora peko. Your cute rabbit overlord who one day will be the most famous VTuber in the world peko.

Fan: How did you become famous?
Pekora: Some of my videos went viral and I gained a lot of overseas fans due to it peko.

Fan: What is your favorite thing to do?
Pekora: Eating carrots peko.

Fan: What is your favorite game?
Pekora: Minecraft peko. I like blowing up things peko.";

#[derive(Debug)]
pub struct OpenAiApi {
    client: Client,
    last_message: RwLock<String>,
}

impl OpenAiApi {
    pub fn new(config: &AiChatbotConfig) -> anyhow::Result<Self> {
        let client = Client::new(&config.openai_token);

        Ok(Self {
            client,
            last_message: RwLock::new(INIT_PROMPT.to_string()),
        })
    }

    #[instrument]
    pub async fn clear(&self) {
        *self.last_message.write().await = INIT_PROMPT.to_string()
    }

    #[instrument]
    pub async fn prompt(
        &'static self,
        prompt: &str,
        temperature: f64,
        top_p: f64,
        presence_penalty: f64,
        frequency_penalty: f64,
    ) -> anyhow::Result<String> {
        let mut last_msg = self.last_message.write().await;

        let full_prompt = format!(
            "{}{}: {}\n{}: ",
            last_msg, RESTART_SEQUENCE, prompt, START_SEQUENCE
        );

        let args = api::CompletionArgs::builder()
            .prompt(&full_prompt)
            .engine(api::Engine::Davinci)
            .max_tokens(64)
            .temperature(temperature)
            .top_p(top_p)
            .presence_penalty(presence_penalty)
            .frequency_penalty(frequency_penalty)
            .stop(vec!["\n".into()])
            .build()
            .map_err(|e| anyhow!(e))?;

        let completion =
            tokio::task::spawn_blocking(move || -> Result<api::Completion, openai_api::Error> {
                self.client.complete_prompt_sync(args)
            })
            .await??;

        let choice = completion
            .choices
            .get(0)
            .ok_or_else(|| anyhow!("No completions returned!"))?;

        let answer = choice.text.clone();
        *last_msg = format!("{}{}", full_prompt, answer);

        Ok(answer)
    }
}
