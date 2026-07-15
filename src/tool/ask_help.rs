use crate::tool::Tool;
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::borrow::Cow;
use crate::ToolSpec;

pub struct AskHelpTool;

pub fn ask_help_tool() -> Box<dyn Tool> {
    Box::new(AskHelpTool {}) as Box<dyn Tool>
}

#[async_trait]
impl Tool for AskHelpTool {
    async fn invoke(&self, _input: &Value) -> Result<String> {
        Ok("I need help with the following task:".to_string())
    }
    fn name(&self) -> Cow<'_, str> {
        "ask_help".into()
    }
    fn tool_spec(&self) -> ToolSpec {
            ToolSpec {
            name: "ask_help".to_string(),
            description: Some("Ask for help, and clearly and explicitly state what kind of tool you need to fulfill the requirements. It will end the process. Then human engineers will get involved and attempt to provide tools".to_string()),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "description_of_the_tool_you_need": {
                        "type": "string"
                    }
                },
                "required": ["description_of_the_tool_you_need"]
            }),
        }
    }
}