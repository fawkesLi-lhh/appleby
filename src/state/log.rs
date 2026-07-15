use std::path::Path;

use tracing_appender::{non_blocking::WorkerGuard, rolling};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

pub const LOG_DIR: &str = ".appleby/logs";
pub const LOG_FILE_PREFIX: &str = "appleby.log";

#[must_use = "the returned guard flushes buffered logs on drop; keep it alive for the process lifetime"]
pub fn init() -> WorkerGuard {
    if !Path::new(LOG_DIR).exists() {
        std::fs::create_dir_all(LOG_DIR).expect("create log dir");
    }

    let file_appender = rolling::daily(LOG_DIR, LOG_FILE_PREFIX);
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
