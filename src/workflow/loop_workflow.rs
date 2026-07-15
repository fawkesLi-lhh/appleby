use std::collections::HashSet;

use crate::state::loop_state::LoopState;
use anthropic_ai_sdk::types::message::{
    ContentBlock, CreateMessageParams, Message, MessageClient, MessageContent,
    RequiredMessageParams, Role, StopReason,
};
use anyhow::Context;
use inquire::Text;

pub async fn loop_workflow(state: &mut LoopState) -> Result<(), anyhow::Error> {
    loop {
        let query = Text::new("--- How can I help you?")
            .prompt()
            .context("An error happened or user cancelled the input.")?;

        //break out of the loop if the user enters exit()
        if query.trim() == "exit()" {
            break;
        }
        state.push_message(Message::new_text(Role::User, query));
        agent_loop(state).await?;
    }
    Ok(())
}

pub async fn agent_loop(state: &mut LoopState) -> Result<(), anyhow::Error> {
    loop {
        let request = CreateMessageParams::new(RequiredMessageParams {
            model: state.model.clone(),
            messages: normalize_messages(&state.get_context()),
            max_tokens: 8000,
        })
        .with_system(state.system_prompt.clone())
        .with_tools(state.tools.values().map(|tool| tool.tool_spec()).collect());

        let response = state.client.create_message(Some(&request)).await?;

        state.push_message(Message::new_blocks(
            Role::Assistant,
            response.content.clone(),
        ));

        if let Some(stop_reason) = response.stop_reason
            && !matches!(stop_reason, StopReason::ToolUse)
        {
            return Ok(());
        }

        let tool_result = state.execute_tool_call(&response.content).await?;

        state.push_message(Message::new_blocks(Role::User, tool_result));
    }
}

pub fn normalize_messages(messages: &[Message]) -> Vec<Message> {
    let mut messages = messages.to_vec();

    // 1. 收集已有 tool_result
    let mut existing_results = HashSet::new();
    for msg in &messages {
        if let MessageContent::Blocks { content } = &msg.content {
            for block in content {
                if let ContentBlock::ToolResult { tool_use_id, .. } = block {
                    existing_results.insert(tool_use_id.clone());
                }
            }
        }
    }

    // 2. 查找 orphan tool_use
    let mut extra_messages = Vec::new();

    for msg in &messages {
        if matches!(msg.role, Role::User) {
            continue;
        }

        if let MessageContent::Blocks { content } = &msg.content {
            for block in content {
                if let ContentBlock::ToolUse { id, .. } = block
                    && !existing_results.contains(id)
                {
                    extra_messages.push(Message::new_blocks(
                        Role::User,
                        vec![ContentBlock::ToolResult {
                            tool_use_id: id.clone(),
                            content: "(cancelled)".to_string(),
                        }],
                    ));
                }
            }
        }
    }
    messages.extend(extra_messages);

    // 3. 合并连续相同 role
    let mut merged: Vec<Message> = Vec::new();
    for msg in messages {
        if let Some(last) = merged.last_mut()
            && matches!(
                (last.role, msg.role),
                (Role::User, Role::User) | (Role::Assistant, Role::Assistant)
            )
        {
            // 合并 content
            match (&mut last.content, msg.content) {
                (
                    MessageContent::Blocks { content: prev },
                    MessageContent::Blocks { content: curr },
                ) => {
                    prev.extend(curr);
                }
                (
                    MessageContent::Text { content: prev },
                    MessageContent::Text { content: curr },
                ) => {
                    prev.push('\n');
                    prev.push_str(&curr);
                }
                (
                    MessageContent::Text { content: prev },
                    MessageContent::Blocks { content: curr },
                ) => {
                    let mut new_blocks = vec![ContentBlock::Text { text: prev.clone() }];
                    new_blocks.extend(curr);
                    last.content = MessageContent::Blocks {
                        content: new_blocks,
                    };
                }
                (
                    MessageContent::Blocks { content: prev },
                    MessageContent::Text { content: curr },
                ) => {
                    prev.push(ContentBlock::Text { text: curr });
                }
            }
            continue;
        }
        merged.push(msg);
    }

    merged
}
