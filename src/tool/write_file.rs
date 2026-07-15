use std::borrow::Cow;

use crate::{
    ToolSpec,
    tool::{Tool, safe_path},
};
use anyhow::{Context as _, Result};
use async_trait::async_trait;
use serde_json::Value;
use tokio::fs;

pub struct WriteFileTool;

pub fn write_file_tool() -> Box<dyn Tool> {
    Box::new(WriteFileTool {}) as Box<dyn Tool>
}

#[async_trait]
impl Tool for WriteFileTool {
    async fn invoke(&self, input: &Value) -> Result<String> {
        let path = input
            .get("path")
            .and_then(|v| v.as_str())
            .context("Invalid path")?;
        let path = safe_path(path)?;

        let content = input
            .get("content")
            .and_then(|v| v.as_str())
            .context("Invalid content")?;

        // 创建父目录
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await.ok();
        }

        fs::write(&path, content)
            .await
            .map_err(|e| anyhow::anyhow!("Error: {}", e))?;

        Ok(format!(
            "Wrote {} bytes to {}",
            content.len(),
            path.display()
        ))
    }

    fn name(&self) -> Cow<'_, str> {
        "write_file".into()
    }

    fn tool_spec(&self) -> ToolSpec {
        ToolSpec {
            name: "write_file".to_string(),
            description: Some("Write content to file.".to_string()),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["path", "content"]
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
    async fn creates_parent_directories_and_writes_new_file() {
        let temp = workspace_tempdir();
        let path = temp.path().join("nested").join("output.txt");

        let result = WriteFileTool
            .invoke(&json!({
                "path": path,
                "content": "你好, Appleby!"
            }))
            .await
            .unwrap();

        assert!(result.starts_with("Wrote 16 bytes to "));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "你好, Appleby!");
    }

    #[tokio::test]
    async fn overwrites_existing_file() {
        let temp = workspace_tempdir();
        let path = temp.path().join("output.txt");
        std::fs::write(&path, "old content").unwrap();

        WriteFileTool
            .invoke(&json!({
                "path": path,
                "content": "new content"
            }))
            .await
            .unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new content");
    }

    #[tokio::test]
    async fn writes_empty_content() {
        let temp = workspace_tempdir();
        let path = temp.path().join("empty.txt");

        WriteFileTool
            .invoke(&json!({
                "path": path,
                "content": ""
            }))
            .await
            .unwrap();

        assert_eq!(std::fs::metadata(&path).unwrap().len(), 0);
    }

    #[tokio::test]
    async fn rejects_path_outside_workspace() {
        let outside = tempfile::tempdir().unwrap();
        let path = outside.path().join("output.txt");

        let error = WriteFileTool
            .invoke(&json!({
                "path": path,
                "content": "should not be written"
            }))
            .await
            .unwrap_err();

        assert!(error.to_string().contains("Path escapes workspace"));
        assert!(!path.exists());
    }
}
