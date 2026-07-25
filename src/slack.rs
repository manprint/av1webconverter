//! Notifiche di terminazione ed errore verso Slack tramite Incoming Webhook.

use std::time::Duration;

use serde::Serialize;

use crate::config::SlackConfig;
use crate::models::{Job, JobStatus};

#[derive(Debug, Clone)]
pub struct SlackNotifier {
    config: SlackConfig,
    client: reqwest::Client,
}

#[derive(Debug, Serialize)]
struct SlackPayload {
    text: String,
}

impl SlackNotifier {
    pub fn new(config: SlackConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_default();
        Self { config, client }
    }

    pub fn is_enabled(&self) -> bool {
        self.config.enabled && !self.config.webhook_url.is_empty()
    }

    /// Invia un messaggio libero. Un fallimento viene registrato ma non
    /// propagato: una notifica persa non deve far fallire una conversione.
    pub async fn send(&self, text: String) {
        if !self.is_enabled() {
            tracing::debug!("notifiche Slack disabilitate, messaggio non inviato");
            return;
        }
        let payload = SlackPayload { text };
        match self
            .client
            .post(&self.config.webhook_url)
            .json(&payload)
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => {
                tracing::debug!("notifica Slack inviata");
            }
            Ok(response) => {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                tracing::error!(%status, body = %body.trim(), "Slack ha rifiutato la notifica");
            }
            Err(error) => {
                tracing::error!(%error, "invio notifica Slack fallito");
            }
        }
    }

    /// Notifica di fine job, filtrata dalle preferenze di configurazione.
    pub async fn notify_job_finished(&self, job: &Job) {
        let should_notify = match job.status {
            JobStatus::Completed => self.config.notify_on_job_completed,
            JobStatus::Failed => self.config.notify_on_job_failed,
            JobStatus::Stopped => self.config.notify_on_job_completed,
            _ => false,
        };
        if !should_notify {
            return;
        }
        self.send(format_job_message(job)).await;
    }

    pub async fn notify_file_failed(&self, job: &Job, file_name: &str, error: &str) {
        if !self.config.notify_on_file_failed {
            return;
        }
        self.send(format!(
            ":x: av1webconverter — errore nel job *{}*: file `{}` non convertito.\n> {}",
            job.name, file_name, error
        ))
        .await;
    }
}

/// Messaggio di riepilogo del job, con emoji coerente allo stato.
pub fn format_job_message(job: &Job) -> String {
    let counts = job.counts();
    let (emoji, label) = match job.status {
        JobStatus::Completed => (":white_check_mark:", "completato"),
        JobStatus::Failed => (":x:", "terminato con errori"),
        JobStatus::Stopped => (":black_square_for_stop:", "interrotto"),
        JobStatus::Running => (":hourglass:", "in corso"),
        JobStatus::Pending => (":inbox_tray:", "in attesa"),
    };
    format!(
        "{emoji} av1webconverter — job *{}* {label}.\n\
         File: {} totali · {} convertiti · {} saltati · {} falliti · {} annullati",
        job.name, counts.total, counts.completed, counts.skipped, counts.failed, counts.cancelled
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::EncodingParams;
    use crate::models::{FileStatus, JobFile};
    use std::path::PathBuf;

    fn job_con_esiti() -> Job {
        let mut job = Job::new("notturno".into(), EncodingParams::default(), 1);
        for (index, status) in [FileStatus::Completed, FileStatus::Failed]
            .into_iter()
            .enumerate()
        {
            let mut file = JobFile::new(
                format!("f{index}.mp4"),
                format!("f{index}.mp4"),
                PathBuf::from("/tmp/x.mp4"),
                10,
            );
            file.status = status;
            job.files.push(file);
        }
        job.status = JobStatus::Failed;
        job
    }

    #[test]
    fn messaggio_riassume_gli_esiti() {
        let message = format_job_message(&job_con_esiti());
        assert!(message.contains("notturno"));
        assert!(message.contains("terminato con errori"));
        assert!(message.contains("2 totali"));
        assert!(message.contains("1 convertiti"));
        assert!(message.contains("1 falliti"));
    }

    #[test]
    fn notificatore_disabilitato_senza_webhook() {
        let notifier = SlackNotifier::new(SlackConfig {
            enabled: true,
            webhook_url: String::new(),
            ..Default::default()
        });
        assert!(!notifier.is_enabled());
    }

    #[tokio::test]
    async fn invio_disabilitato_non_fa_nulla() {
        let notifier = SlackNotifier::new(SlackConfig::default());
        notifier.send("ignorato".into()).await;
    }
}
