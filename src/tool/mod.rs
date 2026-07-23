use std::collections::HashMap;
use std::{borrow::Cow, fmt::Write};

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

mod bash;
mod edit_file;
mod read_file;
mod write_file;
use bash::bash_tool;
use edit_file::edit_file_tool;
use read_file::read_file_tool;
use write_file::write_file_tool;

#[derive(Debug, Clone, Deserialize, PartialEq, Serialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: Value,
}

pub fn toolmap() -> HashMap<String, Box<dyn Tool>> {
    HashMap::from([
        ("Bash".to_string(), bash_tool()),
        ("Read".to_string(), read_file_tool()),
        ("Write".to_string(), write_file_tool()),
        ("Edit".to_string(), edit_file_tool()),
    ])
}

#[async_trait]
pub trait Tool: Send + Sync {
    async fn invoke(&self, input: &Value) -> Result<String>;
    fn name(&self) -> Cow<'_, str>;
    fn tool_spec(&self) -> ToolSpec;
    fn show_to_human(&self, writer: &mut dyn Write, input: &Value) -> Result<(), anyhow::Error>;
}

#[auto_context::auto_context]
fn safe_path(path: &str) -> Result<std::path::PathBuf> {
    let cwd = std::env::current_dir()?.canonicalize()?;
    let requested = std::path::Path::new(path);
    let candidate = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        cwd.join(requested)
    };

    let mut ancestor = candidate.as_path();
    let mut missing_components = Vec::new();
    while !ancestor.exists() {
        let component = ancestor.file_name().context("Invalid path")?.to_os_string();
        missing_components.push(component);
        ancestor = ancestor.parent().context("Invalid path")?;
    }

    let mut full = ancestor.canonicalize()?;
    if !full.starts_with(&cwd) {
        return Err(anyhow::anyhow!("Path escapes workspace"));
    }

    for component in missing_components.iter().rev() {
        full.push(component);
    }

    Ok(full)
}

#[cfg(test)]
pub(crate) fn workspace_tempdir() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(".appleby-unit-test-")
        .tempdir_in(std::env::current_dir().expect("get current directory"))
        .expect("create temporary directory in workspace")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toolset_registers_expected_tools() {
        let tools = toolmap();
        let mut names = tools.keys().map(String::as_str).collect::<Vec<_>>();
        names.sort_unstable();

        assert_eq!(names, ["Bash", "Edit", "Read", "Write"]);
        for (registered_name, tool) in tools {
            assert_eq!(tool.name().as_ref(), registered_name);
            assert_eq!(tool.tool_spec().name, registered_name);
        }
    }

    #[test]
    fn safe_path_accepts_existing_path_inside_workspace() {
        let temp = workspace_tempdir();

        let resolved = safe_path(temp.path().to_str().unwrap()).unwrap();

        assert_eq!(resolved, temp.path().canonicalize().unwrap());
    }

    #[test]
    fn safe_path_accepts_missing_path_inside_workspace() {
        let temp = workspace_tempdir();
        let requested = temp.path().join("nested").join("new.txt");

        let resolved = safe_path(requested.to_str().unwrap()).unwrap();

        assert_eq!(
            resolved,
            temp.path()
                .canonicalize()
                .unwrap()
                .join("nested")
                .join("new.txt")
        );
    }

    #[test]
    fn safe_path_rejects_existing_path_outside_workspace() {
        let outside = tempfile::tempdir().unwrap();

        let error = safe_path(outside.path().to_str().unwrap()).unwrap_err();

        assert!(error.to_string().contains("Path escapes workspace"));
    }

    #[test]
    fn safe_path_rejects_missing_path_outside_workspace() {
        let outside = tempfile::tempdir().unwrap();
        let requested = outside.path().join("nested").join("new.txt");

        let error = safe_path(requested.to_str().unwrap()).unwrap_err();

        assert!(error.to_string().contains("Path escapes workspace"));
    }

    #[test]
    fn safe_path_rejects_parent_directory_traversal() {
        let error = safe_path("..").unwrap_err();

        assert!(error.to_string().contains("Path escapes workspace"));
    }
}
