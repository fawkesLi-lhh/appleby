use anthropic_ai_sdk::{client::AnthropicClientBuilder, types::message::MessageError};
use anyhow::Context;
use appleby::{
    state::loop_state::LoopState, tool::toolmap, workflow::loop_workflow::loop_workflow,
};
use tracing::info;

#[auto_context::auto_context]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _log_guard = appleby::state::log::init();

    let config = appleby::state::config::CONFIG.clone();
    info!(?config, "loaded config");
    let client = AnthropicClientBuilder::new(config.anthropic_api_key, "")
        .with_api_base_url(config.anthropic_base_url)
        .build::<MessageError>()?;
    let tools = toolmap();
    let mut loop_state = LoopState::new(client, tools);
    loop_workflow(&mut loop_state).await?;
    
    Ok(())
}
