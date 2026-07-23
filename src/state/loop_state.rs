use anyhow::{Context, Result};
use std::collections::HashMap;

use crate::{
    api_adapter::{ApiAdapter, ConversationMessage, ToolCallRecord},
    state::conversation_context::ConversationContext,
    tool::Tool,
};

pub struct LoopState {
    pub api_adapter: Box<dyn ApiAdapter>,
    pub tools: HashMap<String, Box<dyn Tool>>,
    pub model: String,
    pub system_prompt: String,
    conversation_context: ConversationContext,
}

impl LoopState {
    pub fn new(
        api_adapter: Box<dyn ApiAdapter>,
        tools: HashMap<String, Box<dyn Tool>>,
        model: String,
        system_prompt: String,
        conversation_context: ConversationContext,
    ) -> Self {
        Self {
            api_adapter,
            tools,
            model,
            system_prompt,
            conversation_context,
        }
    }

    pub fn push_message(&mut self, message: ConversationMessage) -> Result<()> {
        self.conversation_context.push(message)
    }

    pub fn get_context(&self) -> &[ConversationMessage] {
        self.conversation_context.messages()
    }

    pub async fn execute_tool_call(&self, tool_call: &ToolCallRecord) -> ConversationMessage {
        let output = self
            .execute(&tool_call.name, &tool_call.arguments)
            .await
            .unwrap_or_else(|error| format!("Error: {error}"));
        ConversationMessage::tool(tool_call.id.clone(), output)
    }

    pub fn describe_tool_call(&self, tool_call: &ToolCallRecord) -> String {
        let Some(tool) = self.tools.get(&tool_call.name) else {
            return format!("Unknown tool: {}", tool_call.name);
        };

        let mut description = String::new();
        match tool.show_to_human(&mut description, &tool_call.arguments) {
            Ok(()) => description,
            Err(error) => format!("Unable to display tool call: {error}"),
        }
    }

    pub async fn execute(&self, name: &str, input: &serde_json::Value) -> Result<String> {
        let Some(tool) = self.tools.get(name) else {
            anyhow::bail!("Unknown tool: {name}");
        };

        tool.invoke(input)
            .await
            .context(format!("Error invoking tool {name}"))
    }
}
