//! Logging: daily rolling files in `%LOCALAPPDATA%\Medvita\logs` (14 kept), plus stderr in debug.
//!
//! The level comes from `MEDVITA_LOG` (an `EnvFilter` directive, default `info`). What may be logged
//! is restricted by AGENTS.md: never a response body, query string, token or patient data.

use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Keeps the background log writer alive; drop it only when the process ends.
pub struct LoggingGuard {
    _file: Option<WorkerGuard>,
}

pub const ENV_FILTER: &str = "MEDVITA_LOG";
const KEEP_FILES: usize = 14;

/// Installs the global subscriber. Falls back to stderr only when the log directory is unusable.
pub fn init(logs_dir: &Path) -> LoggingGuard {
    let filter = EnvFilter::try_from_env(ENV_FILTER).unwrap_or_else(|_| EnvFilter::new("info"));

    let appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("medvita")
        .filename_suffix("log")
        .max_log_files(KEEP_FILES)
        .build(logs_dir);

    let (file_layer, guard) = match appender {
        Ok(appender) => {
            let (writer, guard) = tracing_appender::non_blocking(appender);
            let layer = tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_target(true)
                .with_writer(writer);
            (Some(layer), Some(guard))
        }
        Err(_) => (None, None),
    };

    let stderr_layer = (cfg!(debug_assertions) || file_layer.is_none())
        .then(|| tracing_subscriber::fmt::layer().with_writer(std::io::stderr));

    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(file_layer)
        .with(stderr_layer)
        .try_init();

    LoggingGuard { _file: guard }
}
