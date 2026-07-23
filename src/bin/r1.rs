use anyhow::Context;
use appleby::{
    api_adapter::{ApiAdapter, ApiRequest, ConversationMessage, openai::OpenAiAdapter},
    state::config::Config,
    tool::toolmap,
};

const APP_DIR: &str = ".appleby";

#[auto_context::auto_context]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::load_or_create_in_dir(APP_DIR)?;
    println!(
        "config: model={}, base_url={}",
        config.openai_model, config.openai_base_url
    );

    let api_adapter = OpenAiAdapter::from_config(&config);
    let tools = toolmap();
    let response = api_adapter
        .complete(ApiRequest {
            model: config.openai_model,
            system_prompt: String::new(),
            messages: vec![ConversationMessage::user("收到请回复ok")],
            tools: tools.values().map(|tool| tool.tool_spec()).collect(),
            max_tokens: 8192,
        })
        .await?;

    println!("{:?}", response.assistant_message);

    Ok(())
}
