use anyhow::{Context, Result};
use std::collections::HashMap;
use tracing::info;

use crate::{
    api_adapter::{ApiAdapter, ConversationMessage, ToolCallRecord},
    state::{
        config::CONFIG,
        conversation_context::{
            CONTEXT_FILE, CONTEXT_LOAD_LIMIT, archive_context_file, load_recent_messages,
            open_context_log,
        },
        system_prompt::SYSTEM_PROMPT,
    },
    tool::Tool,
    utils::jsonl::JsonlLog,
};

pub struct LoopState {
    pub api_adapter: Box<dyn ApiAdapter>,
    pub tools: HashMap<String, Box<dyn Tool>>,
    pub model: String,
    pub system_prompt: String,
    context: Vec<ConversationMessage>,
    context_log: JsonlLog,
}

impl LoopState {
    pub fn new(
        api_adapter: Box<dyn ApiAdapter>,
        tools: HashMap<String, Box<dyn Tool>>,
        load_previous_context: bool,
    ) -> Result<Self> {
        let model = CONFIG.openai_model.clone();
        let system_prompt = (*SYSTEM_PROMPT).0.clone();
        let context = if load_previous_context {
            load_recent_messages(CONTEXT_FILE, CONTEXT_LOAD_LIMIT)?
        } else {
            if let Some(archive_path) = archive_context_file(CONTEXT_FILE)? {
                info!(
                    archive_path = %archive_path.display(),
                    "archived previous conversation context"
                );
            }
            Vec::new()
        };
        let context_log = open_context_log(CONTEXT_FILE)?;

        info!(
            loaded_messages = context.len(),
            load_previous_context, "initialized conversation context"
        );

        Ok(Self {
            api_adapter,
            context,
            context_log,
            tools,
            model,
            system_prompt,
        })
    }

    pub fn push_message(&mut self, message: ConversationMessage) -> Result<()> {
        self.context_log
            .append(&message)
            .context("append message to conversation context log")?;
        self.context.push(message);
        Ok(())
    }

    pub fn get_context(&self) -> &Vec<ConversationMessage> {
        &self.context
    }

    pub async fn execute_tool_calls(
        &mut self,
        tool_calls: &[ToolCallRecord],
    ) -> Result<Vec<ConversationMessage>> {
        let mut result = Vec::new();
        for tool_call in tool_calls {
            let output = self.execute(&tool_call.name, &tool_call.arguments).await?;
            result.push(ConversationMessage::tool(tool_call.id.clone(), output));
        }
        Ok(result)
    }

    pub async fn execute(&mut self, name: &str, input: &serde_json::Value) -> Result<String> {
        let Some(tool) = self.tools.get_mut(name) else {
            anyhow::bail!("Unknown tool: {name}");
        };

        let mut buf = String::new();
        tool.show_to_human(&mut buf, input)?;
        println!("Assistant ToolUse:{}", buf);

        tool.invoke(input)
            .await
            .context(format!("Error invoking tool {name}"))
    }
}
