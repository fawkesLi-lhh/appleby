use anyhow::Context;
use appleby::{
    api_adapter::openai::OpenAiAdapter,
    state::{
        config::Config,
        conversation_context::{ContextLoadMode, ConversationContext},
        loop_state::LoopState,
        system_prompt::SystemPrompt,
    },
    tool::toolmap,
    tui,
    workflow::{loop_workflow::agent_loop, tui_channel::tui_channel},
};
use tracing::info;

const APP_DIR: &str = ".appleby";
const CONTEXT_LOAD_LIMIT: usize = 20;
const USAGE: &str = "Usage: appleby [--no-load-context]\n\nOptions:\n  --no-load-context    Archive the previous conversation context and start fresh";

#[auto_context::auto_context]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let context_load_mode = parse_context_load_mode_arg()?;
    let _log_guard = appleby::state::log::init_in_dir(APP_DIR);

    let config = Config::load_or_create_in_dir(APP_DIR)?;
    let system_prompt = SystemPrompt::load_or_create_in_dir(APP_DIR)?.0;
    let conversation_context = ConversationContext::open_in_dir(APP_DIR, context_load_mode)?;

    info!(
        model = %config.openai_model,
        base_url = %config.openai_base_url,
        context_load_mode = ?context_load_mode,
        "loaded app runtime"
    );
    let api_adapter = Box::new(OpenAiAdapter::from_config(&config));
    let tools = toolmap();
    let loop_state = LoopState::new(
        api_adapter,
        tools,
        config.openai_model,
        system_prompt,
        conversation_context,
    );
    let (agent_channel, tui_channel) = tui_channel();
    let agent_task = tokio::spawn(agent_loop(loop_state, agent_channel));
    let tui_result = tui::run(tui_channel).await;
    let agent_result = agent_task.await.context("join agent loop task")?;

    tui_result?;
    agent_result?;

    Ok(())
}

fn parse_context_load_mode_arg() -> anyhow::Result<ContextLoadMode> {
    let mut context_load_mode = ContextLoadMode::LoadRecent {
        limit: CONTEXT_LOAD_LIMIT,
    };
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--no-load-context" => context_load_mode = ContextLoadMode::FreshArchive,
            "--help" | "-h" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            _ => anyhow::bail!("unknown argument `{arg}`\n\n{USAGE}"),
        }
    }
    Ok(context_load_mode)
}
