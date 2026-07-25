//! Punto di ingresso: carica la configurazione, prepara le cartelle di lavoro,
//! verifica ffmpeg e serve frontend e API.

use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;

use av1webconverter::{api, config::Config, logging, state::AppState};

#[derive(Debug, Parser)]
#[command(
    name = "av1webconverter",
    version,
    about = "Convertitore video AV1 con frontend web embedded"
)]
struct Cli {
    /// File di configurazione TOML.
    #[arg(short, long, env = "AV1WC_CONFIG", default_value = "config.toml")]
    config: PathBuf,

    /// Stampa la configurazione effettiva e termina.
    #[arg(long)]
    print_config: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let mut config = Config::load(Some(&cli.config))?;
    config.prepare_directories()?;

    let _log_guard = logging::init(
        &config.paths.log_dir,
        &config.logging.level,
        config.logging.json_file,
    )?;

    if cli.print_config {
        println!("{}", toml::to_string_pretty(&config)?);
        return Ok(());
    }

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        input_dir = %config.paths.input_dir.display(),
        output_dir = %config.paths.output_dir.display(),
        data_dir = %config.paths.data_dir.display(),
        log_dir = %config.paths.log_dir.display(),
        "avvio av1webconverter"
    );

    check_ffmpeg(&config).await;
    if config.auth.enabled && config.auth.password == "changeme" {
        tracing::warn!(
            "la password di basic auth è ancora quella di default: cambiarla in configurazione"
        );
    }
    if !config.slack.enabled {
        tracing::info!("notifiche Slack disabilitate");
    }

    let address = SocketAddr::new(config.server.bind, config.server.port);
    let state = AppState::new(config).await?;
    let app = api::router(state);

    let listener = tokio::net::TcpListener::bind(address)
        .await
        .with_context(|| format!("bind su {address}"))?;
    tracing::info!(%address, "server in ascolto");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("errore del server HTTP")?;

    tracing::info!("arresto completato");
    Ok(())
}

/// La versione di ffmpeg viene solo segnalata: un binario assente non impedisce
/// l'avvio, ma va reso evidente subito nel log.
async fn check_ffmpeg(config: &Config) {
    for binary in [&config.ffmpeg.ffmpeg_bin, &config.ffmpeg.ffprobe_bin] {
        match tokio::process::Command::new(binary)
            .arg("-version")
            .output()
            .await
        {
            Ok(output) if output.status.success() => {
                let first_line = String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_string();
                tracing::info!(binary = %binary.display(), version = %first_line, "binario ffmpeg disponibile");
            }
            Ok(output) => {
                tracing::error!(
                    binary = %binary.display(),
                    status = ?output.status.code(),
                    "il binario ffmpeg non risponde a -version"
                );
            }
            Err(error) => {
                tracing::error!(
                    binary = %binary.display(),
                    %error,
                    "binario ffmpeg non trovato: le conversioni falliranno"
                );
            }
        }
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => tracing::error!(%error, "registrazione del segnale SIGTERM fallita"),
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => tracing::info!("ricevuto SIGINT, arresto in corso"),
        _ = terminate => tracing::info!("ricevuto SIGTERM, arresto in corso"),
    }
}
