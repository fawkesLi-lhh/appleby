use std::{fs, path::Path};

use anyhow::Context;

const SYSTEM_PROMPT_FILE_NAME: &str = "system_prompt.txt";

#[derive(Debug, Clone)]
pub struct SystemPrompt(pub String);

impl SystemPrompt {
    pub fn load_or_create_in_dir(app_dir: impl AsRef<Path>) -> anyhow::Result<Self> {
        Self::load_or_create(app_dir.as_ref().join(SYSTEM_PROMPT_FILE_NAME))
    }

    pub fn load_or_create(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("create system prompt directory `{}`", parent.display())
            })?;
        }

        if !path.exists() {
            fs::write(path, SystemPrompt::default().0)
                .with_context(|| format!("write default system prompt `{}`", path.display()))?;
        }

        let system_prompt = fs::read_to_string(path)
            .with_context(|| format!("read system prompt `{}`", path.display()))?;
        Ok(Self(system_prompt))
    }
}

#[cfg(test)]
mod tests {
    use super::SystemPrompt;

    #[test]
    fn load_or_create_in_dir_uses_prompt_file_in_app_directory() {
        let temp = tempfile::tempdir().unwrap();
        let app_dir = temp.path().join("app-data");

        let prompt = SystemPrompt::load_or_create_in_dir(&app_dir).unwrap();

        assert_eq!(
            std::fs::read_to_string(app_dir.join("system_prompt.txt")).unwrap(),
            prompt.0
        );
    }
}

impl Default for SystemPrompt {
    fn default() -> Self {
        Self(
            "
You are a helpful assistant. You can use the following tools to help the user.
        "
            .to_string(),
        )
    }
}
