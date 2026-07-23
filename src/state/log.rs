use std::path::{Path, PathBuf};

use tracing_appender::{non_blocking::WorkerGuard, rolling};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

const LOG_DIR_NAME: &str = "logs";
const LOG_FILE_PREFIX: &str = "appleby.log";

#[must_use = "the returned guard flushes buffered logs on drop; keep it alive for the process lifetime"]
pub fn init_in_dir(app_dir: impl AsRef<Path>) -> WorkerGuard {
    init_with_prefix(log_dir(app_dir.as_ref()), LOG_FILE_PREFIX)
}

fn log_dir(app_dir: &Path) -> PathBuf {
    app_dir.join(LOG_DIR_NAME)
}

#[must_use = "the returned guard flushes buffered logs on drop; keep it alive for the process lifetime"]
pub fn init_with_prefix(log_dir: impl AsRef<Path>, file_prefix: &str) -> WorkerGuard {
    let log_dir = log_dir.as_ref();
    if !log_dir.exists() {
        std::fs::create_dir_all(log_dir).expect("create log dir");
    }

    let file_appender = rolling::daily(log_dir, file_prefix);
    let (writer, guard) = tracing_appender::non_blocking(file_appender);

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(filter)
        .with(
            fmt::layer()
                .with_writer(writer)
                .with_ansi(false)
                .with_target(true)
                .with_line_number(true)
                .with_file(false),
        )
        .init();

    guard
}

#[cfg(test)]
mod tests {
    use super::log_dir;

    #[test]
    fn log_dir_is_derived_from_the_supplied_app_directory() {
        assert_eq!(
            log_dir(std::path::Path::new("app-data")),
            std::path::Path::new("app-data").join("logs")
        );
    }
}
