use std::collections::HashMap;
use anthropic_ai_sdk::{
    client::AnthropicClient,
    types::message::{ContentBlock, Message},
};
use anyhow::{Context, Result};
use tracing::info;

use crate::{state::{config::CONFIG, system_prompt::SYSTEM_PROMPT}, tool::Tool};

pub struct LoopState {
    pub client: AnthropicClient,
    pub tools: HashMap<String, Box<dyn Tool>>,
    pub model: String,
    pub system_prompt: String,
    context: Vec<Message>,
    random_id: i32,
}

impl LoopState {
    pub fn new(client: AnthropicClient, tools: HashMap<String, Box<dyn Tool>>) -> Self {
        let random_id = rand::random_range(1000000..9999999);
        let model = CONFIG.anthropic_model.clone();
        let system_prompt = (*SYSTEM_PROMPT).0.clone();
        Self {
            client,
            context: Vec::new(),
            tools,
            random_id,
            model,
            system_prompt
        }
    }

    pub fn push_message(&mut self, message: Message) {
        info!(
            "Random ID: {}: Pushing message: {:?}",
            self.random_id, message
        );
        self.context.push(message);
    }

    pub fn get_context(&self) -> &Vec<Message> {
        &self.context
    }

    pub async fn execute_tool_call(
        &mut self,
        content: &[ContentBlock],
    ) -> Result<Vec<ContentBlock>> {
        let mut result = Vec::new();
        for block in content {
            if let ContentBlock::ToolUse { id, name, input } = block {
                let output = self.execute(name, input).await?;
                result.push(ContentBlock::ToolResult {
                    tool_use_id: id.clone(),
                    content: output,
                });
            }
        }
        Ok(result)
    }

    pub async fn execute(&mut self, name: &str, input: &serde_json::Value) -> Result<String> {
        let Some(tool) = self.tools.get_mut(name) else {
            anyhow::bail!("Unknown tool: {name}");
        };

        tool.invoke(input)
            .await
            .context(format!("Error invoking tool {name}"))
    }
}
