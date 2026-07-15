use std::borrow::Cow;

use crate::{
    ToolSpec,
    tool::{Tool, safe_path},
};
use anyhow::{Context as _, Result};
use async_trait::async_trait;
use serde_json::Value;
use tokio::fs;

pub struct ReadFileTool;

pub fn read_file_tool() -> Box<dyn Tool> {
    Box::new(ReadFileTool {}) as Box<dyn Tool>
}

#[async_trait]
impl Tool for ReadFileTool {
    async fn invoke(&self, input: &Value) -> Result<String> {
        let path = input
            .get("path")
            .and_then(|v| v.as_str())
            .context("Invalid path")?;
        let path = safe_path(path)?;

        let start_line = input
            .get("start_line")
            .and_then(|v| v.as_u64())
            .unwrap_or(1) as usize;
        let end_line = input
            .get("end_line")
            .and_then(|v| v.as_u64())
            .unwrap_or(100) as usize;

        if start_line == 0 {
            return Err(anyhow::anyhow!("Error: start_line must be at least 1"));
        }
        if start_line > end_line {
            return Err(anyhow::anyhow!(
                "Error: start_line must not exceed end_line"
            ));
        }

        let content = fs::read_to_string(path)
            .await
            .map_err(|e| anyhow::anyhow!("Error: {}", e))?;

        let lines: Vec<String> = content
            .lines()
            .skip(start_line - 1)
            .take(end_line - start_line + 1)
            .enumerate()
            .map(|(i, s)| format!("{}: {}", start_line + i, s))
            .collect();

        let mut result = lines.join("\n").chars().take(50000).collect::<String>();

        // add file total line count
        let total_line_count = content.lines().count();
        result.push_str(&format!(
            "\n\nFile total line count: {}\n",
            total_line_count
        ));

        Ok(result)
    }

    fn name(&self) -> Cow<'_, str> {
        "read_file".into()
    }

    fn tool_spec(&self) -> ToolSpec {
        ToolSpec {
            name: "read_file".to_string(),
            description: Some(
                "Read file contents. start_line and end_line are inclusive. start_line is 1-based. Include end_line, if chars is greater than 50000, it will be truncated. The line_number is numbered from 1. The file total line count is also included.".to_string(),
            ),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "start_line": { "type": "integer", "default": 1 },
                    "end_line": { "type": "integer", "default": 100 }
                },
                "required": ["path", "start_line", "end_line"]
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::workspace_tempdir;
    use serde_json::json;

    #[tokio::test]
    async fn reads_requested_lines_with_source_line_numbers() {
        let temp = workspace_tempdir();
        let path = temp.path().join("multiline.txt");
        std::fs::write(&path, "alpha\nbeta\ngamma\ndelta\n").unwrap();

        let result = ReadFileTool
            .invoke(&json!({
                "path": path,
                "start_line": 2,
                "end_line": 3
            }))
            .await
            .unwrap();

        assert_eq!(result, "2: beta\n3: gamma\n\nFile total line count: 4\n");
    }

    #[tokio::test]
    async fn reads_unicode_content() {
        let temp = workspace_tempdir();
        let path = temp.path().join("unicode.txt");
        std::fs::write(&path, "第一行\n第二行🙂\n第三行 café\n").unwrap();

        let result = ReadFileTool
            .invoke(&json!({
                "path": path,
                "start_line": 1,
                "end_line": 2
            }))
            .await
            .unwrap();

        assert_eq!(
            result,
            "1: 第一行\n2: 第二行🙂\n\nFile total line count: 3\n"
        );
    }

    #[tokio::test]
    async fn rejects_zero_start_line() {
        let temp = workspace_tempdir();
        let path = temp.path().join("input.txt");
        std::fs::write(&path, "content\n").unwrap();

        let error = ReadFileTool
            .invoke(&json!({
                "path": path,
                "start_line": 0,
                "end_line": 1
            }))
            .await
            .unwrap_err();

        assert!(error.to_string().contains("start_line must be at least 1"));
    }

    #[tokio::test]
    async fn rejects_reversed_line_range() {
        let temp = workspace_tempdir();
        let path = temp.path().join("input.txt");
        std::fs::write(&path, "content\n").unwrap();

        let error = ReadFileTool
            .invoke(&json!({
                "path": path,
                "start_line": 3,
                "end_line": 2
            }))
            .await
            .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("start_line must not exceed end_line")
        );
    }
}
