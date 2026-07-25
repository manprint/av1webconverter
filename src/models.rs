//! Modello dati dei job di conversione, così come viene persistito su disco
//! e restituito al frontend.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    /// Creato, in attesa di avvio manuale.
    Pending,
    /// Almeno un file in conversione.
    Running,
    /// Tutti i file elaborati senza errori.
    Completed,
    /// Terminato con almeno un file fallito.
    Failed,
    /// Interrotto dall'utente.
    Stopped,
}

impl JobStatus {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Stopped)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileStatus {
    Pending,
    Running,
    Completed,
    /// Output già presente e `overwrite` disattivo.
    Skipped,
    Failed,
    Cancelled,
}

impl FileStatus {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Skipped | Self::Failed | Self::Cancelled
        )
    }
}

/// Avanzamento di una singola conversione, con gli stessi campi mostrati
/// dagli script bash di riferimento.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Progress {
    pub percent: f64,
    pub elapsed_seconds: f64,
    pub total_seconds: f64,
    pub frame: u64,
    pub total_frames: u64,
    pub written_bytes: u64,
    pub bitrate: String,
    pub speed: String,
    pub eta_seconds: Option<f64>,
    /// In modalità a dimensione fissa la codifica ha due passaggi.
    pub pass: u8,
    pub total_passes: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobFile {
    pub id: Uuid,
    /// Nome originale, comprensivo del percorso relativo per gli upload di cartelle.
    pub original_name: String,
    /// Nome sul filesystem, ripulito e reso univoco.
    pub stored_name: String,
    pub input_path: PathBuf,
    pub output_path: Option<PathBuf>,
    pub input_size_bytes: u64,
    pub output_size_bytes: Option<u64>,
    pub status: FileStatus,
    pub progress: Progress,
    pub error: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub uploaded_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub started_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub finished_at: Option<OffsetDateTime>,
}

impl JobFile {
    pub fn new(
        original_name: String,
        stored_name: String,
        input_path: PathBuf,
        input_size_bytes: u64,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            original_name,
            stored_name,
            input_path,
            output_path: None,
            input_size_bytes,
            output_size_bytes: None,
            status: FileStatus::Pending,
            progress: Progress::default(),
            error: None,
            uploaded_at: OffsetDateTime::now_utc(),
            started_at: None,
            finished_at: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: Uuid,
    pub name: String,
    pub status: JobStatus,
    pub params: crate::config::EncodingParams,
    /// Conversioni simultanee ammesse dentro questo job.
    pub concurrency: usize,
    pub files: Vec<JobFile>,
    pub error: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub started_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub finished_at: Option<OffsetDateTime>,
}

impl Job {
    pub fn new(name: String, params: crate::config::EncodingParams, concurrency: usize) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            status: JobStatus::Pending,
            params,
            concurrency,
            files: Vec::new(),
            error: None,
            created_at: OffsetDateTime::now_utc(),
            started_at: None,
            finished_at: None,
        }
    }

    pub fn file(&self, id: Uuid) -> Option<&JobFile> {
        self.files.iter().find(|file| file.id == id)
    }

    pub fn file_mut(&mut self, id: Uuid) -> Option<&mut JobFile> {
        self.files.iter_mut().find(|file| file.id == id)
    }

    /// Avanzamento complessivo del job, pesato sulla dimensione dei sorgenti:
    /// un file da 4 GB non vale quanto uno da 40 MB.
    pub fn overall_progress(&self) -> f64 {
        let total_weight: f64 = self
            .files
            .iter()
            .map(|file| file.input_size_bytes.max(1) as f64)
            .sum();
        if total_weight <= 0.0 {
            return 0.0;
        }
        let done: f64 = self
            .files
            .iter()
            .map(|file| {
                let weight = file.input_size_bytes.max(1) as f64;
                let fraction = match file.status {
                    FileStatus::Completed | FileStatus::Skipped => 1.0,
                    FileStatus::Failed | FileStatus::Cancelled => 1.0,
                    FileStatus::Running => (file.progress.percent / 100.0).clamp(0.0, 1.0),
                    FileStatus::Pending => 0.0,
                };
                weight * fraction
            })
            .sum();
        (done / total_weight * 100.0).clamp(0.0, 100.0)
    }

    pub fn counts(&self) -> JobCounts {
        let mut counts = JobCounts {
            total: self.files.len(),
            ..Default::default()
        };
        for file in &self.files {
            match file.status {
                FileStatus::Pending => counts.pending += 1,
                FileStatus::Running => counts.running += 1,
                FileStatus::Completed => counts.completed += 1,
                FileStatus::Skipped => counts.skipped += 1,
                FileStatus::Failed => counts.failed += 1,
                FileStatus::Cancelled => counts.cancelled += 1,
            }
        }
        counts
    }

    /// I file scaricabili sono quelli con un output effettivamente presente.
    pub fn downloadable_files(&self) -> impl Iterator<Item = &JobFile> {
        self.files.iter().filter(|file| {
            matches!(file.status, FileStatus::Completed | FileStatus::Skipped)
                && file.output_path.is_some()
        })
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct JobCounts {
    pub total: usize,
    pub pending: usize,
    pub running: usize,
    pub completed: usize,
    pub skipped: usize,
    pub failed: usize,
    pub cancelled: usize,
}

/// Vista del job restituita dalle API, con i campi derivati già calcolati.
#[derive(Debug, Clone, Serialize)]
pub struct JobView {
    #[serde(flatten)]
    pub job: Job,
    pub overall_progress: f64,
    pub counts: JobCounts,
}

impl From<&Job> for JobView {
    fn from(job: &Job) -> Self {
        Self {
            overall_progress: job.overall_progress(),
            counts: job.counts(),
            job: job.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::EncodingParams;

    fn job_with_files(sizes_and_status: &[(u64, FileStatus, f64)]) -> Job {
        let mut job = Job::new("test".into(), EncodingParams::default(), 1);
        for (index, (size, status, percent)) in sizes_and_status.iter().enumerate() {
            let mut file = JobFile::new(
                format!("f{index}.mp4"),
                format!("f{index}.mp4"),
                PathBuf::from(format!("/tmp/f{index}.mp4")),
                *size,
            );
            file.status = *status;
            file.progress.percent = *percent;
            job.files.push(file);
        }
        job
    }

    #[test]
    fn progresso_vuoto_e_zero() {
        let job = job_with_files(&[]);
        assert_eq!(job.overall_progress(), 0.0);
    }

    #[test]
    fn progresso_pesato_sulla_dimensione() {
        let job = job_with_files(&[
            (900, FileStatus::Completed, 100.0),
            (100, FileStatus::Pending, 0.0),
        ]);
        assert!((job.overall_progress() - 90.0).abs() < 0.001);
    }

    #[test]
    fn progresso_include_i_file_in_corso() {
        let job = job_with_files(&[
            (100, FileStatus::Completed, 100.0),
            (100, FileStatus::Running, 50.0),
        ]);
        assert!((job.overall_progress() - 75.0).abs() < 0.001);
    }

    #[test]
    fn conteggi_per_stato() {
        let job = job_with_files(&[
            (1, FileStatus::Completed, 100.0),
            (1, FileStatus::Failed, 0.0),
            (1, FileStatus::Pending, 0.0),
        ]);
        let counts = job.counts();
        assert_eq!(counts.total, 3);
        assert_eq!(counts.completed, 1);
        assert_eq!(counts.failed, 1);
        assert_eq!(counts.pending, 1);
    }
}
