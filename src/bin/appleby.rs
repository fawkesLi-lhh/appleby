use anyhow::Context;
use appleby::{
    api_adapter::openai::OpenAiAdapter, state::loop_state::LoopState, tool::toolmap,
    workflow::loop_workflow::loop_workflow,
};
use tracing::info;

const USAGE: &str = "Usage: appleby [--no-load-context]\n\nOptions:\n  --no-load-context    Archive the previous conversation context and start fresh";

#[auto_context::auto_context]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let load_previous_context = parse_load_previous_context_arg()?;
    let _log_guard = appleby::state::log::init();

    let config = appleby::state::config::CONFIG.clone();
    info!(
        model = %config.openai_model,
        base_url = %config.openai_base_url,
        load_previous_context,
        "loaded config"
    );
    let api_adapter = Box::new(OpenAiAdapter::from_config(&config));
    let tools = toolmap();
    let mut loop_state = LoopState::new(api_adapter, tools, load_previous_context)?;
    loop_workflow(&mut loop_state).await?;

    Ok(())
}

fn parse_load_previous_context_arg() -> anyhow::Result<bool> {
    let mut load_previous_context = true;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--no-load-context" => load_previous_context = false,
            "--help" | "-h" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            _ => anyhow::bail!("unknown argument `{arg}`\n\n{USAGE}"),
        }
    }
    Ok(load_previous_context)
}
