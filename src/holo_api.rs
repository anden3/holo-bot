use graphql_client::{GraphQLQuery, Response};
use std::error::Error;

type ISO8601DateTime = String;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "queries/schema.json",
    query_path = "queries/get_scheduled_lives.graphql",
	response_derives = "Debug",
)]
pub struct GetScheduledLives;

pub struct HoloAPI {
	client: reqwest::Client,
}

impl HoloAPI {
	pub fn new() -> HoloAPI {
		let client = reqwest::ClientBuilder::new()
			.user_agent(concat!(
				env!("CARGO_PKG_NAME"),
				"/",
				env!("CARGO_PKG_VERSION"),
			))
			.build()
			.expect("Failed to build client.");

		return HoloAPI { client };
	}

	pub async fn get_scheduled_streams(&self, variables: get_scheduled_lives::Variables) -> Result<(), Box<dyn Error>> {
		let request_body = GetScheduledLives::build_query(variables);

		let res = self.client.post("https://holo.dev/graphql").json(&request_body).send().await?;
		let response_body: Response<get_scheduled_lives::ResponseData> = res.json().await?;
		println!("{:#?}", response_body);

		Ok(())
	}
}
