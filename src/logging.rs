//! Log applicativi: leggibili su stdout, strutturati JSON su file rotante
//! giornaliero in `log_dir/av1webconverter.jsonl`.

use std::path::Path;

use anyhow::{Context, Result};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

/// Inizializza `tracing`. La guardia restituita va tenuta viva per tutta la
/// durata del processo: al drop scarica il buffer del file di log.
pub fn init(log_dir: &Path, level: &str, json_file: bool) -> Result<Option<WorkerGuard>> {
    let filter = EnvFilter::try_from_env("AV1WC_LOG")
        .or_else(|_| EnvFilter::try_new(level))
        .unwrap_or_else(|_| EnvFilter::new("info"));

    let stdout_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stdout()));

    if !json_file {
        tracing_subscriber::registry()
            .with(filter)
            .with(stdout_layer)
            .init();
        return Ok(None);
    }

    std::fs::create_dir_all(log_dir)
        .with_context(|| format!("creazione cartella log {}", log_dir.display()))?;
    let appender = tracing_appender::rolling::daily(log_dir, "av1webconverter.jsonl");
    let (writer, guard) = tracing_appender::non_blocking(appender);

    let file_layer = tracing_subscriber::fmt::layer()
        .json()
        .flatten_event(true)
        .with_current_span(true)
        .with_span_list(false)
        .with_writer(writer);

    tracing_subscriber::registry()
        .with(filter)
        .with(stdout_layer)
        .with(file_layer)
        .init();

    Ok(Some(guard))
}
