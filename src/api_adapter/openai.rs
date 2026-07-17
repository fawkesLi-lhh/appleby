use anyhow::Context;
use async_openai::{
    Client,
    config::OpenAIConfig,
    types::chat::{
        ChatCompletionMessageToolCall, ChatCompletionMessageToolCalls,
        ChatCompletionRequestAssistantMessage, ChatCompletionRequestAssistantMessageContent,
        ChatCompletionRequestMessage, ChatCompletionRequestSystemMessage,
        ChatCompletionRequestSystemMessageContent, ChatCompletionRequestToolMessage,
        ChatCompletionRequestToolMessageContent, ChatCompletionRequestUserMessage,
        ChatCompletionRequestUserMessageContent, ChatCompletionResponseMessage, ChatCompletionTool,
        ChatCompletionTools, CreateChatCompletionRequestArgs, FunctionCall, FunctionObject,
    },
};
use async_trait::async_trait;

use crate::{
    api_adapter::{ApiAdapter, ApiRequest, ApiResponse, ConversationMessage, ToolCallRecord},
    state::config::Config,
    tool::ToolSpec,
};

pub struct OpenAiAdapter {
    client: Client<OpenAIConfig>,
}

impl OpenAiAdapter {
    pub fn from_config(config: &Config) -> Self {
        let openai_config = OpenAIConfig::new()
            .with_api_key(config.openai_api_key.clone())
            .with_api_base(config.openai_base_url.clone());

        Self {
            client: Client::with_config(openai_config),
        }
    }
}

#[async_trait]
impl ApiAdapter for OpenAiAdapter {
    async fn complete(&self, request: ApiRequest) -> anyhow::Result<ApiResponse> {
        let request = CreateChatCompletionRequestArgs::default()
            .model(request.model)
            .messages(build_chat_messages(
                &request.system_prompt,
                &request.messages,
            ))
            .max_completion_tokens(request.max_tokens)
            .tools(build_tools(request.tools))
            .build()?;

        let response = self.client.chat().create(request).await?;
        let choice = response
            .choices
            .into_iter()
            .next()
            .context("OpenAI response did not include any choices")?;
        let assistant_message = assistant_response_to_message(choice.message)?;

        Ok(ApiResponse { assistant_message })
    }
}

fn build_chat_messages(
    system_prompt: &str,
    messages: &[ConversationMessage],
) -> Vec<ChatCompletionRequestMessage> {
    let mut output = Vec::with_capacity(messages.len() + 1);
    if !system_prompt.trim().is_empty() {
        output.push(ChatCompletionRequestMessage::System(
            ChatCompletionRequestSystemMessage {
                content: ChatCompletionRequestSystemMessageContent::Text(system_prompt.to_string()),
                name: None,
            },
        ));
    }

    output.extend(messages.iter().map(conversation_message_to_openai));
    output
}

fn build_tools(tools: Vec<ToolSpec>) -> Vec<ChatCompletionTools> {
    tools.into_iter().map(tool_spec_to_openai).collect()
}

fn tool_spec_to_openai(tool: ToolSpec) -> ChatCompletionTools {
    ChatCompletionTools::Function(ChatCompletionTool {
        function: FunctionObject {
            name: tool.name,
            description: tool.description,
            parameters: Some(tool.input_schema),
            strict: None,
        },
    })
}

fn assistant_response_to_message(
    message: ChatCompletionResponseMessage,
) -> anyhow::Result<ConversationMessage> {
    let tool_calls = message
        .tool_calls
        .unwrap_or_default()
        .into_iter()
        .map(tool_call_record_from_openai)
        .collect::<anyhow::Result<Vec<_>>>()?;

    Ok(ConversationMessage::assistant(message.content, tool_calls))
}

fn conversation_message_to_openai(message: &ConversationMessage) -> ChatCompletionRequestMessage {
    match message {
        ConversationMessage::User { content } => {
            ChatCompletionRequestMessage::User(ChatCompletionRequestUserMessage {
                content: ChatCompletionRequestUserMessageContent::Text(content.clone()),
                name: None,
            })
        }
        ConversationMessage::Assistant {
            content,
            tool_calls,
        } => {
            let mut message = ChatCompletionRequestAssistantMessage {
                content: content
                    .clone()
                    .map(ChatCompletionRequestAssistantMessageContent::Text),
                ..Default::default()
            };
            if !tool_calls.is_empty() {
                message.tool_calls =
                    Some(tool_calls.iter().map(tool_call_record_to_openai).collect());
            }
            ChatCompletionRequestMessage::Assistant(message)
        }
        ConversationMessage::Tool {
            tool_call_id,
            content,
        } => ChatCompletionRequestMessage::Tool(ChatCompletionRequestToolMessage {
            content: ChatCompletionRequestToolMessageContent::Text(content.clone()),
            tool_call_id: tool_call_id.clone(),
        }),
    }
}

fn tool_call_record_to_openai(record: &ToolCallRecord) -> ChatCompletionMessageToolCalls {
    ChatCompletionMessageToolCalls::Function(ChatCompletionMessageToolCall {
        id: record.id.clone(),
        function: FunctionCall {
            name: record.name.clone(),
            arguments: serde_json::to_string(&record.arguments)
                .unwrap_or_else(|_| "{}".to_string()),
        },
    })
}

fn tool_call_record_from_openai(
    tool_call: ChatCompletionMessageToolCalls,
) -> anyhow::Result<ToolCallRecord> {
    match tool_call {
        ChatCompletionMessageToolCalls::Function(call) => {
            let arguments = parse_tool_arguments(&call.function.arguments).with_context(|| {
                format!("parse arguments for tool call `{}`", call.function.name)
            })?;
            Ok(ToolCallRecord {
                id: call.id,
                name: call.function.name,
                arguments,
            })
        }
        ChatCompletionMessageToolCalls::Custom(call) => {
            let arguments = parse_tool_arguments(&call.custom_tool.input).with_context(|| {
                format!(
                    "parse arguments for custom tool call `{}`",
                    call.custom_tool.name
                )
            })?;
            Ok(ToolCallRecord {
                id: call.id,
                name: call.custom_tool.name,
                arguments,
            })
        }
    }
}

fn parse_tool_arguments(arguments: &str) -> anyhow::Result<serde_json::Value> {
    if arguments.trim().is_empty() {
        return Ok(serde_json::json!({}));
    }

    Ok(serde_json::from_str(arguments)?)
}

#[cfg(test)]
mod tests {
    use async_openai::types::chat::{
        ChatCompletionMessageToolCall, ChatCompletionMessageToolCalls, ChatCompletionTools,
    };

    use super::{assistant_response_to_message, build_chat_messages, tool_spec_to_openai};
    use crate::{
        api_adapter::{ConversationMessage, ToolCallRecord},
        tool::ToolSpec,
    };

    #[test]
    fn build_chat_messages_adds_system_and_conversation_messages() {
        let messages = build_chat_messages(
            "system",
            &[
                ConversationMessage::user("hello"),
                ConversationMessage::assistant(Some("hi".to_string()), Vec::new()),
            ],
        );

        assert_eq!(messages.len(), 3);
    }

    #[test]
    fn tool_spec_converts_to_openai_function_tool() {
        let tool = ToolSpec {
            name: "Read".to_string(),
            description: Some("Read files".to_string()),
            input_schema: serde_json::json!({"type":"object"}),
        };

        let ChatCompletionTools::Function(openai_tool) = tool_spec_to_openai(tool) else {
            panic!("expected function tool")
        };

        assert_eq!(openai_tool.function.name, "Read");
        assert_eq!(
            openai_tool.function.description.as_deref(),
            Some("Read files")
        );
        assert_eq!(
            openai_tool.function.parameters,
            Some(serde_json::json!({"type":"object"}))
        );
    }

    #[test]
    #[allow(deprecated)]
    fn assistant_response_converts_function_tool_calls() {
        let response = async_openai::types::chat::ChatCompletionResponseMessage {
            content: Some("checking".to_string()),
            refusal: None,
            tool_calls: Some(vec![ChatCompletionMessageToolCalls::Function(
                ChatCompletionMessageToolCall {
                    id: "call_1".to_string(),
                    function: async_openai::types::chat::FunctionCall {
                        name: "Read".to_string(),
                        arguments: "{\"path\":\"Cargo.toml\"}".to_string(),
                    },
                },
            )]),
            annotations: None,
            role: async_openai::types::chat::Role::Assistant,
            function_call: None,
            audio: None,
        };

        let message = assistant_response_to_message(response).unwrap();

        assert_eq!(
            message,
            ConversationMessage::assistant(
                Some("checking".to_string()),
                vec![ToolCallRecord {
                    id: "call_1".to_string(),
                    name: "Read".to_string(),
                    arguments: serde_json::json!({"path":"Cargo.toml"}),
                }]
            )
        );
    }
}
