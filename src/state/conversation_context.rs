use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Context;
use chrono::Local;

use crate::{api_adapter::ConversationMessage, utils::jsonl::JsonlLog};

const CONTEXT_FILE_NAME: &str = "context.jsonl";

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ContextLoadMode {
    LoadRecent { limit: usize },
    FreshArchive,
}

pub struct ConversationContext {
    messages: Vec<ConversationMessage>,
    log: JsonlLog,
}

impl ConversationContext {
    pub fn open_in_dir(app_dir: impl AsRef<Path>, mode: ContextLoadMode) -> anyhow::Result<Self> {
        Self::open_jsonl(app_dir.as_ref().join(CONTEXT_FILE_NAME), mode)
    }

    pub fn open_jsonl(path: impl AsRef<Path>, mode: ContextLoadMode) -> anyhow::Result<Self> {
        let path = path.as_ref();
        let messages = match mode {
            ContextLoadMode::LoadRecent { limit } => load_recent_messages(path, limit)?,
            ContextLoadMode::FreshArchive => {
                archive_context_file(path)?;
                Vec::new()
            }
        };
        let log = open_context_log(path)?;

        Ok(Self { messages, log })
    }

    pub fn push(&mut self, message: ConversationMessage) -> anyhow::Result<()> {
        self.log
            .append(&message)
            .context("append message to conversation context log")?;
        self.messages.push(message);
        Ok(())
    }

    pub fn messages(&self) -> &[ConversationMessage] {
        &self.messages
    }
}

fn load_recent_messages(
    path: impl AsRef<Path>,
    limit: usize,
) -> anyhow::Result<Vec<ConversationMessage>> {
    let path = path.as_ref();
    if limit == 0 || !path.exists() {
        return Ok(Vec::new());
    }

    let mut log = JsonlLog::open(path)
        .with_context(|| format!("open conversation context log `{}`", path.display()))?;
    let mut reader = log.last_reader().with_context(|| {
        format!(
            "open tail reader for conversation context `{}`",
            path.display()
        )
    })?;
    let mut messages = Vec::new();

    while messages.len() < limit {
        match reader.read_last_one::<ConversationMessage>() {
            Ok(Some(message)) => messages.push(message),
            Ok(None) => break,
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("read conversation context from `{}`", path.display())
                });
            }
        }
    }

    messages.reverse();
    Ok(messages)
}

fn archive_context_file(path: impl AsRef<Path>) -> anyhow::Result<Option<PathBuf>> {
    let path = path.as_ref();
    if !path.exists() {
        return Ok(None);
    }

    let timestamp = Local::now().format("%Y%m%d-%H%M%S");
    let archive_path = unique_archive_path(path, &timestamp.to_string());
    fs::rename(path, &archive_path).with_context(|| {
        format!(
            "archive conversation context `{}` to `{}`",
            path.display(),
            archive_path.display()
        )
    })?;
    Ok(Some(archive_path))
}

fn open_context_log(path: impl AsRef<Path>) -> anyhow::Result<JsonlLog> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "create conversation context directory `{}`",
                parent.display()
            )
        })?;
    }

    JsonlLog::open(path)
        .with_context(|| format!("open conversation context log `{}`", path.display()))
}

fn unique_archive_path(path: &Path, timestamp: &str) -> PathBuf {
    let base = PathBuf::from(format!("{}.{}", path.display(), timestamp));
    if !base.exists() {
        return base;
    }

    for index in 1.. {
        let candidate = PathBuf::from(format!("{}.{}.{}", path.display(), timestamp, index));
        if !candidate.exists() {
            return candidate;
        }
    }

    unreachable!("unbounded archive suffix search should always find a free path")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::api_adapter::ConversationMessage;

    use super::{ContextLoadMode, ConversationContext};

    #[test]
    fn open_in_dir_uses_context_file_in_app_directory() {
        let temp = tempfile::tempdir().unwrap();
        let app_dir = temp.path().join("app-data");

        let context =
            ConversationContext::open_in_dir(&app_dir, ContextLoadMode::LoadRecent { limit: 20 })
                .unwrap();

        assert!(context.messages().is_empty());
        assert!(app_dir.join("context.jsonl").exists());
    }

    #[test]
    fn loads_recent_messages_in_chronological_order() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("context.jsonl");
        let mut context =
            ConversationContext::open_jsonl(&path, ContextLoadMode::LoadRecent { limit: 0 })
                .unwrap();

        for index in 0..5 {
            context
                .push(ConversationMessage::user(format!("message-{index}")))
                .unwrap();
        }
        drop(context);

        let context =
            ConversationContext::open_jsonl(&path, ContextLoadMode::LoadRecent { limit: 3 })
                .unwrap();
        let contents: Vec<_> = context
            .messages()
            .iter()
            .map(|message| match message {
                ConversationMessage::User { content } => content.as_str(),
                _ => unreachable!(),
            })
            .collect();

        assert_eq!(contents, vec!["message-2", "message-3", "message-4"]);
    }

    #[test]
    fn missing_or_zero_limit_context_loads_empty() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("missing.jsonl");

        let context =
            ConversationContext::open_jsonl(&path, ContextLoadMode::LoadRecent { limit: 20 })
                .unwrap();
        assert!(context.messages().is_empty());
        assert!(path.exists());

        let mut context =
            ConversationContext::open_jsonl(&path, ContextLoadMode::LoadRecent { limit: 0 })
                .unwrap();
        context.push(ConversationMessage::user("kept out")).unwrap();
        drop(context);

        let context =
            ConversationContext::open_jsonl(&path, ContextLoadMode::LoadRecent { limit: 0 })
                .unwrap();
        assert!(context.messages().is_empty());
    }

    #[test]
    fn fresh_archive_archives_existing_context_file() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("context.jsonl");
        fs::write(&path, "{\"content\":\"old\"}\n").unwrap();

        let context =
            ConversationContext::open_jsonl(&path, ContextLoadMode::FreshArchive).unwrap();

        assert!(context.messages().is_empty());
        assert!(path.exists());
        assert_eq!(fs::read_to_string(&path).unwrap(), "");
        let archived = fs::read_dir(temp.path())
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(
            archived
                .iter()
                .any(|name| name.starts_with("context.jsonl."))
        );
    }

    #[test]
    fn fresh_archive_missing_context_creates_empty_file() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("context.jsonl");

        let context =
            ConversationContext::open_jsonl(&path, ContextLoadMode::FreshArchive).unwrap();

        assert!(context.messages().is_empty());
        assert!(path.exists());
    }

    #[test]
    fn incompatible_legacy_context_returns_error_without_archiving() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("context.jsonl");
        let legacy_content = "{\"role\":\"user\",\"content\":{\"unexpected\":\"legacy\"}}\n";
        fs::write(&path, legacy_content).unwrap();

        let result =
            ConversationContext::open_jsonl(&path, ContextLoadMode::LoadRecent { limit: 20 });
        let Err(error) = result else {
            panic!("expected incompatible context to return an error");
        };

        assert!(error.to_string().contains("read conversation context"));
        assert!(path.exists());
        assert_eq!(fs::read_to_string(&path).unwrap(), legacy_content);
        let archived = fs::read_dir(temp.path())
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(
            !archived
                .iter()
                .any(|name| name.starts_with("context.jsonl."))
        );
    }
}
