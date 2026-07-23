use std::{env, fs, path::Path};

use anyhow::Context;
use serde::{Deserialize, Serialize};

const CONFIG_FILE_NAME: &str = "config.toml";

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub openai_api_key: String,
    pub openai_base_url: String,
    pub openai_model: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ConfigBuilder {
    openai_api_key: Option<String>,
    openai_base_url: Option<String>,
    openai_model: Option<String>,
}

impl ConfigBuilder {
    fn into_config(self) -> anyhow::Result<Config> {
        let openai_api_key = self
            .openai_api_key
            .ok_or_else(|| anyhow::anyhow!("OPENAI_API_KEY is not set"))?;
        let openai_base_url = self
            .openai_base_url
            .ok_or_else(|| anyhow::anyhow!("OPENAI_BASE_URL is not set"))?;
        let openai_model = self
            .openai_model
            .ok_or_else(|| anyhow::anyhow!("OPENAI_MODEL is not set"))?;
        Ok(Config {
            openai_api_key,
            openai_base_url,
            openai_model,
        })
    }
}

impl Config {
    fn load_from_file(path: impl AsRef<Path>) -> anyhow::Result<ConfigBuilder> {
        let content = fs::read_to_string(&path)
            .with_context(|| format!("read config `{}`", path.as_ref().display()))?;
        toml::from_str::<ConfigBuilder>(&content)
            .with_context(|| format!("parse config `{}`", path.as_ref().display()))
    }

    fn write_to_file(path: impl AsRef<Path>, config: &Config) -> anyhow::Result<()> {
        fs::write(&path, toml::to_string(config)?)
            .with_context(|| format!("write config `{}`", path.as_ref().display()))
    }

    pub fn load_or_create_in_dir(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref().join(CONFIG_FILE_NAME);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create config directory `{}`", parent.display()))?;
        }
        let mut builder = Self::load_from_file(&path).unwrap_or_default();
        if let Ok(openai_api_key) = env::var("OPENAI_API_KEY") {
            builder.openai_api_key = Some(openai_api_key);
        }
        if let Ok(openai_base_url) = env::var("OPENAI_BASE_URL") {
            builder.openai_base_url = Some(openai_base_url);
        }
        if let Ok(openai_model) = env::var("OPENAI_MODEL") {
            builder.openai_model = Some(openai_model);
        }
        let config = builder.into_config()?;
        Self::write_to_file(&path, &config)?;
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::Config;

    #[test]
    fn load_or_create_in_dir_uses_config_file_in_app_directory() {
        let temp = tempfile::tempdir().unwrap();
        let app_dir = temp.path().join("app-data");
        std::fs::create_dir_all(&app_dir).unwrap();
        std::fs::write(
            app_dir.join("config.toml"),
            r#"
openai_api_key = "test-key"
openai_base_url = "https://example.test/v1"
openai_model = "test-model"
"#,
        )
        .unwrap();

        let config = Config::load_or_create_in_dir(&app_dir).unwrap();

        let config_path = app_dir.join("config.toml");
        let persisted: Config =
            toml::from_str(&std::fs::read_to_string(&config_path).unwrap()).unwrap();
        assert_eq!(persisted.openai_api_key, config.openai_api_key);
        assert_eq!(persisted.openai_base_url, config.openai_base_url);
        assert_eq!(persisted.openai_model, config.openai_model);
    }
}
