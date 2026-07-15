use anthropic_ai_sdk::{
    client::{AnthropicClient, AnthropicClientBuilder},
    types::message::{
        ContentBlock, CreateMessageParams, Message, MessageClient, MessageContent, MessageError,
        RequiredMessageParams,
        Role::{self, User},
        StopReason, Tool,
    },
};
use anyhow::Context;

#[auto_context::auto_context]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = appleby::config::CONFIG.clone();
    println!("config: {:?}", config);
    let client = AnthropicClientBuilder::new(config.anthropic_api_key, "")
        .with_api_base_url(config.anthropic_base_url)
        .build::<MessageError>()?;
    let mut messages = Vec::new();
    messages.push(Message::new_text(User, "Hello, how are you?"));
    let request = CreateMessageParams::new(RequiredMessageParams {
        model: config.anthropic_model.clone(),
        messages,
        max_tokens: 8192,
    });
    let response = client.create_message(Some(&request)).await?;
    for content in response.content {
        println!("{:?}", content);
    }

    Ok(())
}
