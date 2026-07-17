use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Context;
use chrono::Local;

use crate::{api_adapter::ConversationMessage, utils::jsonl::JsonlLog};

pub const CONTEXT_FILE: &str = ".appleby/context.jsonl";
pub const CONTEXT_LOAD_LIMIT: usize = 20;

pub fn load_recent_messages(
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
                drop(reader);
                drop(log);
                archive_context_file(path).with_context(|| {
                    format!(
                        "archive incompatible conversation context `{}` after read error: {error}",
                        path.display()
                    )
                })?;
                return Ok(Vec::new());
            }
        }
    }

    messages.reverse();
    Ok(messages)
}

pub fn archive_context_file(path: impl AsRef<Path>) -> anyhow::Result<Option<PathBuf>> {
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

pub fn open_context_log(path: impl AsRef<Path>) -> anyhow::Result<JsonlLog> {
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

    use super::{archive_context_file, load_recent_messages, open_context_log};

    #[test]
    fn loads_recent_messages_in_chronological_order() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("context.jsonl");
        let mut log = open_context_log(&path).unwrap();

        for index in 0..5 {
            log.append(&ConversationMessage::user(format!("message-{index}")))
                .unwrap();
        }
        drop(log);

        let messages = load_recent_messages(&path, 3).unwrap();
        let contents: Vec<_> = messages
            .into_iter()
            .map(|message| match message {
                ConversationMessage::User { content } => content,
                _ => unreachable!(),
            })
            .collect();

        assert_eq!(contents, vec!["message-2", "message-3", "message-4"]);
    }

    #[test]
    fn missing_or_zero_limit_context_loads_empty() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("missing.jsonl");

        assert!(load_recent_messages(&path, 20).unwrap().is_empty());
        assert!(!path.exists());

        let mut log = open_context_log(&path).unwrap();
        log.append(&ConversationMessage::user("kept out")).unwrap();
        drop(log);

        assert!(load_recent_messages(&path, 0).unwrap().is_empty());
    }

    #[test]
    fn archives_existing_context_file() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("context.jsonl");
        fs::write(&path, "{\"content\":\"old\"}\n").unwrap();

        let archive_path = archive_context_file(&path).unwrap().unwrap();

        assert!(!path.exists());
        assert!(archive_path.exists());
        assert!(archive_path.to_string_lossy().contains("context.jsonl."));
        assert_eq!(
            fs::read_to_string(archive_path).unwrap(),
            "{\"content\":\"old\"}\n"
        );
    }

    #[test]
    fn archiving_missing_context_is_noop() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("context.jsonl");

        assert!(archive_context_file(&path).unwrap().is_none());
    }

    #[test]
    fn archives_incompatible_legacy_context_and_starts_empty() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("context.jsonl");
        fs::write(
            &path,
            "{\"role\":\"user\",\"content\":{\"unexpected\":\"legacy\"}}\n",
        )
        .unwrap();

        let messages = load_recent_messages(&path, 20).unwrap();

        assert!(messages.is_empty());
        assert!(!path.exists());
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
}
