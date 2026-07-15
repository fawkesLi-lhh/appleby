use anthropic_ai_sdk::{
    client::AnthropicClient,
    types::message::{ContentBlock, Message, MessageContent},
};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fmt::Write;
use tracing::info;

use crate::{
    state::{config::CONFIG, system_prompt::SYSTEM_PROMPT},
    tool::Tool,
};

fn format_message_for_log(message: &Message) -> String {
    let mut output = String::new();
    let _ = writeln!(output, "role: {:?}", message.role);

    match &message.content {
        MessageContent::Text { content } => {
            let _ = writeln!(output, "content:");
            write_indented(&mut output, content, "  ");
        }
        MessageContent::Blocks { content } => {
            let _ = writeln!(output, "content blocks:");
            for (index, block) in content.iter().enumerate() {
                let _ = writeln!(output, "  [{index}]");
                write_content_block_for_log(&mut output, block);
            }
        }
    }

    output.trim_end().to_string()
}

fn write_content_block_for_log(output: &mut String, block: &ContentBlock) {
    match block {
        ContentBlock::Text { text } => {
            let _ = writeln!(output, "    type: text");
            let _ = writeln!(output, "    text:");
            write_indented(output, text, "      ");
        }
        ContentBlock::Image { source } => {
            let _ = writeln!(output, "    type: image");
            let _ = writeln!(output, "    media_type: {}", source.media_type);
            let _ = writeln!(output, "    data: <{} bytes base64>", source.data.len());
        }
        ContentBlock::ToolUse { id, name, input } => {
            let _ = writeln!(output, "    type: tool_use");
            let _ = writeln!(output, "    id: {id}");
            let _ = writeln!(output, "    name: {name}");
            let _ = writeln!(output, "    input:");
            let input = serde_json::to_string_pretty(input).unwrap_or_else(|_| input.to_string());
            write_indented(output, &input, "      ");
        }
        ContentBlock::ToolResult {
            tool_use_id,
            content,
        } => {
            let _ = writeln!(output, "    type: tool_result");
            let _ = writeln!(output, "    tool_use_id: {tool_use_id}");
            let _ = writeln!(output, "    content:");
            write_indented(output, content, "      ");
        }
        ContentBlock::Thinking {
            thinking,
            signature,
        } => {
            let _ = writeln!(output, "    type: thinking");
            let _ = writeln!(output, "    thinking:");
            write_indented(output, thinking, "      ");
            let _ = writeln!(output, "    signature: {signature}");
        }
        ContentBlock::RedactedThinking { data } => {
            let _ = writeln!(output, "    type: redacted_thinking");
            let _ = writeln!(output, "    data: <{} bytes>", data.len());
        }
    }
}

fn write_indented(output: &mut String, text: &str, indent: &str) {
    for line in text.lines() {
        let _ = writeln!(output, "{indent}{line}");
    }
}

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
            system_prompt,
        }
    }

    pub fn push_message(&mut self, message: Message) {
        info!(
            "Random ID: {}: Pushing message:\n{}",
            self.random_id,
            format_message_for_log(&message)
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
            match block {
                ContentBlock::ToolUse { id, name, input } => {
                    let output = self.execute(name, input).await?;
                    result.push(ContentBlock::ToolResult {
                        tool_use_id: id.clone(),
                        content: output,
                    });
                }
                _ => {}
            }
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
