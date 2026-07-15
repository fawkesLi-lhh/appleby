use std::borrow::Cow;

use crate::{
    ToolSpec,
    tool::{Tool, safe_path},
};
use anyhow::{Context as _, Result};
use async_trait::async_trait;
use serde_json::Value;
use tokio::fs;

pub struct EditFileTool;

pub fn edit_file_tool() -> Box<dyn Tool> {
    Box::new(EditFileTool {}) as Box<dyn Tool>
}

#[async_trait]
impl Tool for EditFileTool {
    async fn invoke(&self, input: &Value) -> Result<String> {
        let file_path = input
            .get("file_path")
            .and_then(|v| v.as_str())
            .context("Invalid path")?;
        let file_path = safe_path(file_path)?;

        let old_string = input
            .get("old_string")
            .and_then(|v| v.as_str())
            .context("Invalid old_string")?;

        let new_string = input
            .get("new_string")
            .and_then(|v| v.as_str())
            .context("Invalid new_string")?;

        let replace_all = input
            .get("replace_all")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let content = fs::read_to_string(&file_path)
            .await
            .map_err(|e| anyhow::anyhow!("Error: {}", e))?;

        if !content.contains(old_string) {
            return Err(anyhow::anyhow!(
                "Error: Text not found in {}",
                file_path.display()
            ));
        }

        let updated;
        if replace_all {
            updated = content.replacen(old_string, new_string, content.matches(old_string).count());
        } else {
            if old_string.is_empty()
                || !content.contains(old_string)
                || content.matches(old_string).nth(1).is_some()
            {
                return Err(anyhow::anyhow!(
                    "Error: old_string is empty, absent, or appears more than once"
                ));
            }
            updated = content.replacen(old_string, new_string, 1);
        }

        fs::write(&file_path, updated)
            .await
            .map_err(|e| anyhow::anyhow!("Error: {}", e))?;

        Ok(format!("Edited {}", file_path.display()))
    }

    fn name(&self) -> Cow<'_, str> {
        "edit_file".into()
    }

    fn tool_spec(&self) -> ToolSpec {
        ToolSpec {
            name: "edit_file".to_string(),
            description: Some("Replace exact text in file. old_string must include enough unchanged surrounding context to uniquely identify the intended text. Line number prefixes are not part of the file content and must never be copied into old_string or new_string".to_string()),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "file_path": { "type": "string" },
                    "old_string": { "type": "string" },
                    "new_string": { "type": "string" },
                    "replace_all": { "type": "boolean", "default": false, "description": "If true, replace all occurrences of old_string with new_string. If false, replace only the first occurrence of old_string with new_string." }
                },
                "required": ["file_path", "old_string", "new_string", "replace_all"]
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
    async fn replaces_unique_text() {
        let temp = workspace_tempdir();
        let path = temp.path().join("input.txt");
        std::fs::write(&path, "before old after").unwrap();

        EditFileTool
            .invoke(&json!({
                "file_path": path,
                "old_string": "old",
                "new_string": "new",
                "replace_all": false
            }))
            .await
            .unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "before new after");
    }

    #[tokio::test]
    async fn rejects_multiple_matches_by_default() {
        let temp = workspace_tempdir();
        let path = temp.path().join("input.txt");
        std::fs::write(&path, "old and old").unwrap();

        let error = EditFileTool
            .invoke(&json!({
                "file_path": path,
                "old_string": "old",
                "new_string": "new",
                "replace_all": false
            }))
            .await
            .unwrap_err();

        assert!(error.to_string().contains("appears more than once"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "old and old");
    }

    #[tokio::test]
    async fn replaces_all_matches_when_enabled() {
        let temp = workspace_tempdir();
        let path = temp.path().join("input.txt");
        std::fs::write(&path, "old and old").unwrap();

        EditFileTool
            .invoke(&json!({
                "file_path": path,
                "old_string": "old",
                "new_string": "new",
                "replace_all": true
            }))
            .await
            .unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new and new");
    }

    #[tokio::test]
    async fn rejects_empty_old_string() {
        let temp = workspace_tempdir();
        let path = temp.path().join("input.txt");
        std::fs::write(&path, "content").unwrap();

        let error = EditFileTool
            .invoke(&json!({
                "file_path": path,
                "old_string": "",
                "new_string": "new",
                "replace_all": false
            }))
            .await
            .unwrap_err();

        assert!(error.to_string().contains("old_string is empty"));
    }

    #[tokio::test]
    async fn rejects_missing_old_string() {
        let temp = workspace_tempdir();
        let path = temp.path().join("input.txt");
        std::fs::write(&path, "content").unwrap();

        let error = EditFileTool
            .invoke(&json!({
                "file_path": path,
                "old_string": "missing",
                "new_string": "new",
                "replace_all": false
            }))
            .await
            .unwrap_err();

        assert!(error.to_string().contains("Text not found"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "content");
    }

    #[tokio::test]
    async fn rejects_path_outside_workspace() {
        let outside = tempfile::tempdir().unwrap();
        let path = outside.path().join("input.txt");
        std::fs::write(&path, "old").unwrap();

        let error = EditFileTool
            .invoke(&json!({
                "file_path": path,
                "old_string": "old",
                "new_string": "new",
                "replace_all": false
            }))
            .await
            .unwrap_err();

        assert!(error.to_string().contains("Path escapes workspace"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "old");
    }
}
