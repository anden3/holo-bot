use once_cell::sync::OnceCell;

use super::prelude::*;

use apis::openai_api::OpenAiApi;

static OPENAI_API: OnceCell<OpenAiApi> = OnceCell::new();

interaction_setup! {
    name = "chatbot",
    description = "Chat with Pekora!",
    enabled_if = |config| config.ai_chatbot.enabled,
    options = [
        //! Ask Usada Pekora anything!
        ask: SubCommand = [
            //! Your question.
            req prompt: String,

            //! What sampling temperature to use. Higher values means the model will take more risks.
            temperature: Integer = [
                "0.0": 0, "0.1": 1, "0.2": 2, "0.3": 3, "0.4": 4, "0.5": 5,
                "0.6": 6, "0.7": 7, "0.8": 8, "0.9": 9, "1.0": 10,
            ],

            //! Uses nucleus sampling, model considers the results of the tokens with top_p probability mass.
            top_p: Integer = [
                "0.0": 0, "0.1": 1, "0.2": 2, "0.3": 3, "0.4": 4, "0.5": 5,
                "0.6": 6, "0.7": 7, "0.8": 8, "0.9": 9, "1.0": 10,
            ],

            //! Number between 0 and 1, higher values increases model's likelihood to talk about new topics.
            presence_penalty: Integer = [
                "0.0": 0, "0.1": 1, "0.2": 2, "0.3": 3, "0.4": 4, "0.5": 5,
                "0.6": 6, "0.7": 7, "0.8": 8, "0.9": 9, "1.0": 10,
            ],

            //! Number between 0 and 1, higher values decreases model's likelihood to repeat the same line again.
            frequency_penalty: Integer = [
                "0.0": 0, "0.1": 1, "0.2": 2, "0.3": 3, "0.4": 4, "0.5": 5,
                "0.6": 6, "0.7": 7, "0.8": 8, "0.9": 9, "1.0": 10,
            ],
        ]
        //! Clear chat history.
        clear: SubCommand,
    ],
    restrictions = [
        rate_limit = 100 in 24 hours,
        allowed_roles = [
            "Admin",
            "Moderator",
            "Moderator (JP)",
            824337391006646343
        ]
    ]
}

#[interaction_cmd]
pub async fn chatbot(
    ctx: &Ctx,
    interaction: &Interaction,
    config: &Config
) -> anyhow::Result<()> {
    show_deferred_response(&interaction, &ctx).await?;

    let api = match OPENAI_API.get() {
        Some(a) => a,
        None => {
            let data = ctx.data.read().await;
            let config = data.get::<Config>().unwrap();

            if OPENAI_API.set(OpenAiApi::new(config)?).is_err() {
                return Err(anyhow!("Failed to store OpenAI struct."));
            }

            OPENAI_API.get().unwrap()
        }
    };

    for cmd in &interaction.data.as_ref().unwrap().options {
        match cmd.name.as_str() {
            "ask" => {
                parse_interaction_options!(
                cmd, [
                    prompt: req String,
                    temperature: i64,
                    top_p: i64,
                    presence_penalty: i64,
                    frequency_penalty: i64
                ]);

                let response = api
                    .prompt(
                        &prompt,
                        (temperature.unwrap_or(10) as f64) / 10.0,
                        (top_p.unwrap_or(10) as f64) / 10.0,
                        (presence_penalty.unwrap_or(0) as f64) / 10.0,
                        (frequency_penalty.unwrap_or(0) as f64) / 10.0,
                    )
                    .await
                    .context(here!())?;

                interaction.edit_original_interaction_response(&ctx.http, |e| {
                    e.embed(|e| {
                        e.description(response).author(|a| {
                            a.name(&prompt).icon_url(
                                "https://i1.sndcdn.com/artworks-JyzoM7cN8ymbymnR-BL97oQ-t500x500.jpg",
                            )
                        })
                    })
                })
                .await
                .context(here!())?;
            }
            "clear" => api.clear().await,
            _ => return Err(anyhow!("Unknown subcommand!")),
        }
    }

    Ok(())
}
