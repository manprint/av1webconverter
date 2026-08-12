//! Stato condiviso dell'applicazione: registro dei job, persistenza, eventi.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::sync::{watch, RwLock};
use uuid::Uuid;

use crate::config::{Config, EncodingParams};
use crate::error::{ApiError, ApiResult};
use crate::events::{Event, EventBus};
use crate::joblog::JobLogs;
use crate::models::{FileStatus, Job, JobFile, JobStatus, JobView, Progress};
use crate::slack::SlackNotifier;
use crate::store::JobStore;

/// Stato runtime di un job: la parte serializzabile più il canale di stop.
#[derive(Debug)]
pub struct JobHandle {
    pub id: Uuid,
    pub job: RwLock<Job>,
    /// `true` chiede l'interruzione: i processi ffmpeg attivi vengono terminati.
    pub cancel_tx: watch::Sender<bool>,
    /// Evita di avviare due worker sullo stesso job.
    pub worker_running: AtomicBool,
}

impl JobHandle {
    fn new(job: Job) -> Arc<Self> {
        let (cancel_tx, _) = watch::channel(false);
        Arc::new(Self {
            id: job.id,
            job: RwLock::new(job),
            cancel_tx,
            worker_running: AtomicBool::new(false),
        })
    }

    pub fn cancel_rx(&self) -> watch::Receiver<bool> {
        self.cancel_tx.subscribe()
    }

    pub fn request_stop(&self) {
        let _ = self.cancel_tx.send(true);
    }

    pub fn is_stop_requested(&self) -> bool {
        *self.cancel_tx.borrow()
    }

    /// Riarma il canale di stop prima di un nuovo avvio.
    pub fn reset_stop(&self) {
        let _ = self.cancel_tx.send(false);
    }
}

pub struct AppState {
    pub config: Config,
    pub store: JobStore,
    pub logs: Arc<JobLogs>,
    pub events: EventBus,
    pub slack: SlackNotifier,
    jobs: RwLock<HashMap<Uuid, Arc<JobHandle>>>,
}

pub type SharedState = Arc<AppState>;

impl AppState {
    /// Costruisce lo stato e ricarica i job già presenti su disco.
    pub async fn new(config: Config) -> Result<SharedState> {
        let store = JobStore::new(&config.paths.data_dir)?;
        let logs = JobLogs::new(&config.paths.log_dir, config.jobs.log_buffer_lines);
        let slack = SlackNotifier::new(config.slack.clone());
        let state = Arc::new(Self {
            config,
            store,
            logs,
            events: EventBus::default(),
            slack,
            jobs: RwLock::new(HashMap::new()),
        });
        state.restore_jobs().await?;
        Ok(state)
    }

    /// Un job che risulta in esecuzione dopo un riavvio non ha più il processo
    /// ffmpeg corrispondente: i file interrotti tornano in attesa e il job può
    /// essere riavviato dall'utente.
    async fn restore_jobs(&self) -> Result<()> {
        let mut restored = self.store.load_all().await?;
        let mut handles = self.jobs.write().await;
        for job in &mut restored {
            let mut changed = false;
            for file in &mut job.files {
                if file.status == FileStatus::Running {
                    file.status = FileStatus::Pending;
                    file.progress = Progress::default();
                    file.started_at = None;
                    changed = true;
                }
            }
            if job.status == JobStatus::Running {
                job.status = JobStatus::Pending;
                job.error = Some("interrotto dal riavvio del servizio".to_string());
                changed = true;
            }
            if changed {
                self.store.save(job).await?;
                tracing::warn!(job_id = %job.id, name = %job.name, "job ripristinato in stato pending dopo un riavvio");
            }
            handles.insert(job.id, JobHandle::new(job.clone()));
        }
        tracing::info!(count = handles.len(), "job ricaricati da disco");
        Ok(())
    }

    // -- percorsi -----------------------------------------------------------

    pub fn job_input_dir(&self, job_id: Uuid) -> PathBuf {
        self.config.paths.input_dir.join(job_id.to_string())
    }

    pub fn job_output_dir(&self, job_id: Uuid) -> PathBuf {
        self.config.paths.output_dir.join(job_id.to_string())
    }

    pub fn temp_dir(&self) -> PathBuf {
        self.config.paths.data_dir.join("tmp")
    }

    // -- accesso ai job -----------------------------------------------------

    pub async fn handle(&self, job_id: Uuid) -> ApiResult<Arc<JobHandle>> {
        self.jobs
            .read()
            .await
            .get(&job_id)
            .cloned()
            .ok_or_else(|| ApiError::NotFound(format!("job {job_id} non trovato")))
    }

    pub async fn list_views(&self) -> Vec<JobView> {
        let handles: Vec<Arc<JobHandle>> = self.jobs.read().await.values().cloned().collect();
        let mut views = Vec::with_capacity(handles.len());
        for handle in handles {
            views.push(JobView::from(&*handle.job.read().await));
        }
        // Ordine dal più recente al più vecchio.
        views.sort_by_key(|view| std::cmp::Reverse(view.job.created_at));
        views
    }

    pub async fn view(&self, job_id: Uuid) -> ApiResult<JobView> {
        let handle = self.handle(job_id).await?;
        let guard = handle.job.read().await;
        Ok(JobView::from(&*guard))
    }

    // -- mutazioni ----------------------------------------------------------

    /// Applica una modifica al job, la persiste e notifica il frontend.
    pub async fn mutate<F, T>(&self, job_id: Uuid, mutation: F) -> ApiResult<T>
    where
        F: FnOnce(&mut Job) -> ApiResult<T>,
    {
        let handle = self.handle(job_id).await?;
        let (result, snapshot) = {
            let mut job = handle.job.write().await;
            let result = mutation(&mut job)?;
            (result, job.clone())
        };
        self.persist_and_publish(&snapshot).await;
        Ok(result)
    }

    pub async fn persist_and_publish(&self, job: &Job) {
        if let Err(error) = self.store.save(job).await {
            tracing::error!(job_id = %job.id, error = %format!("{error:#}"), "persistenza job fallita");
        }
        self.events.publish(Event::JobUpdated {
            job: Box::new(JobView::from(job)),
        });
    }

    pub async fn create_job(
        &self,
        name: String,
        params: EncodingParams,
        concurrency: usize,
    ) -> ApiResult<JobView> {
        let job = Job::new(name, params, concurrency);
        tokio::fs::create_dir_all(self.job_input_dir(job.id))
            .await
            .context("creazione cartella di input del job")?;
        tokio::fs::create_dir_all(self.job_output_dir(job.id))
            .await
            .context("creazione cartella di output del job")?;

        let view = JobView::from(&job);
        self.jobs
            .write()
            .await
            .insert(job.id, JobHandle::new(job.clone()));
        self.persist_and_publish(&job).await;
        tracing::info!(job_id = %job.id, name = %job.name, "job creato");
        Ok(view)
    }

    /// Rimuove il job e tutti i suoi file: input, output e log.
    pub async fn delete_job(&self, job_id: Uuid) -> ApiResult<()> {
        let handle = self.handle(job_id).await?;

        // Prima si interrompe il lavoro in corso, poi si cancella.
        handle.request_stop();
        let file_ids: Vec<Uuid> = {
            let job = handle.job.read().await;
            job.files.iter().map(|file| file.id).collect()
        };
        self.wait_worker_stopped(&handle).await;

        self.jobs.write().await.remove(&job_id);
        remove_dir_if_present(&self.job_input_dir(job_id)).await;
        remove_dir_if_present(&self.job_output_dir(job_id)).await;
        self.logs.purge_job(job_id, &file_ids).await;
        if let Err(error) = self.store.delete(job_id).await {
            tracing::error!(%job_id, error = %format!("{error:#}"), "rimozione stato job fallita");
        }
        self.events.publish(Event::JobDeleted { job_id });
        tracing::info!(%job_id, "job eliminato con i relativi file");
        Ok(())
    }

    /// Attende che il worker si accorga della richiesta di stop, con un tetto
    /// massimo: la cancellazione non deve poter bloccare la richiesta HTTP.
    pub async fn wait_worker_stopped(&self, handle: &JobHandle) {
        for _ in 0..100 {
            if !handle.worker_running.load(Ordering::SeqCst) {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        tracing::warn!(job_id = %handle.id, "worker ancora attivo dopo l'attesa di stop");
    }

    /// Registra un file caricato. Il nome su disco è ripulito e reso univoco.
    pub async fn add_uploaded_file(
        &self,
        job_id: Uuid,
        original_name: &str,
        temp_path: &Path,
        size_bytes: u64,
    ) -> ApiResult<JobFile> {
        let input_dir = self.job_input_dir(job_id);
        tokio::fs::create_dir_all(&input_dir)
            .await
            .context("creazione cartella di input del job")?;

        let existing: Vec<String> = {
            let handle = self.handle(job_id).await?;
            let job = handle.job.read().await;
            job.files
                .iter()
                .map(|file| file.stored_name.clone())
                .collect()
        };
        let stored_name = unique_stored_name(original_name, &existing);
        let input_path = input_dir.join(&stored_name);

        tokio::fs::rename(temp_path, &input_path)
            .await
            .context("spostamento del file caricato nella cartella di input")?;

        let file = JobFile::new(
            original_name.to_string(),
            stored_name,
            input_path,
            size_bytes,
        );
        let registered = file.clone();
        self.mutate(job_id, move |job| {
            if job.status == JobStatus::Running {
                return Err(ApiError::Conflict(
                    "non si possono aggiungere file a un job in esecuzione".into(),
                ));
            }
            // Un job già terminato che riceve nuovi file torna disponibile.
            if job.status.is_terminal() {
                job.status = JobStatus::Pending;
                job.finished_at = None;
                job.error = None;
            }
            job.files.push(file);
            Ok(())
        })
        .await?;
        Ok(registered)
    }

    pub async fn remove_file(&self, job_id: Uuid, file_id: Uuid) -> ApiResult<()> {
        let file = self
            .mutate(job_id, move |job| {
                let index = job
                    .files
                    .iter()
                    .position(|file| file.id == file_id)
                    .ok_or_else(|| ApiError::NotFound(format!("file {file_id} non trovato")))?;
                if job.files[index].status == FileStatus::Running {
                    return Err(ApiError::Conflict(
                        "il file è in conversione: fermare il job prima di rimuoverlo".into(),
                    ));
                }
                Ok(job.files.remove(index))
            })
            .await?;

        remove_file_if_present(&file.input_path).await;
        if let Some(output_path) = &file.output_path {
            remove_file_if_present(output_path).await;
        }
        self.logs.purge_file(job_id, file_id).await;
        Ok(())
    }
}

/// Nome sicuro e univoco per il filesystem, preservando l'estensione.
pub fn unique_stored_name(original_name: &str, existing: &[String]) -> String {
    // Gli upload di cartelle arrivano con il percorso relativo nel nome.
    let base = original_name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(original_name);
    let sanitized = sanitize_filename::sanitize(base);
    let sanitized = if sanitized.trim().is_empty() {
        "video".to_string()
    } else {
        sanitized
    };

    let (stem, extension) = match sanitized.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => (stem.to_string(), format!(".{extension}")),
        _ => (sanitized.clone(), String::new()),
    };

    let mut candidate = format!("{stem}{extension}");
    let mut counter = 1;
    while existing.iter().any(|name| name == &candidate) {
        candidate = format!("{stem}_{counter}{extension}");
        counter += 1;
    }
    candidate
}

/// Nome dell'output convertito, con il suffisso scelto per il job.
pub fn output_name_for(stored_name: &str, suffix: &str) -> String {
    let stem = stored_name
        .rsplit_once('.')
        .map(|(stem, _)| stem)
        .filter(|stem| !stem.is_empty())
        .unwrap_or(stored_name);
    format!("{stem}{suffix}.mp4")
}

pub async fn remove_dir_if_present(path: &Path) {
    if let Err(error) = tokio::fs::remove_dir_all(path).await {
        if error.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(path = %path.display(), %error, "rimozione cartella fallita");
        }
    }
}

pub async fn remove_file_if_present(path: &Path) {
    if let Err(error) = tokio::fs::remove_file(path).await {
        if error.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(path = %path.display(), %error, "rimozione file fallita");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nome_ripulito_dai_percorsi() {
        assert_eq!(unique_stored_name("cartella/film.mp4", &[]), "film.mp4");
        assert_eq!(
            unique_stored_name("a/b/c/serie - ep1.mp4", &[]),
            "serie - ep1.mp4"
        );
    }

    #[test]
    fn nome_ripulito_dal_path_traversal() {
        let name = unique_stored_name("../../etc/passwd", &[]);
        assert!(!name.contains(".."));
        assert!(!name.contains('/'));
        assert_eq!(name, "passwd");
    }

    #[test]
    fn nome_reso_univoco() {
        let existing = vec!["film.mp4".to_string(), "film_1.mp4".to_string()];
        assert_eq!(unique_stored_name("film.mp4", &existing), "film_2.mp4");
    }

    #[test]
    fn nome_vuoto_ha_un_fallback() {
        assert_eq!(unique_stored_name("///", &[]), "video");
    }

    #[test]
    fn nome_di_output_con_suffisso_configurabile() {
        assert_eq!(output_name_for("film.mp4", "_av1"), "film_av1.mp4");
        assert_eq!(
            output_name_for("film.parte.1.mkv", "_convertito"),
            "film.parte.1_convertito.mp4"
        );
        assert_eq!(
            output_name_for("senzaestensione", ""),
            "senzaestensione.mp4"
        );
    }

    #[tokio::test]
    async fn ciclo_di_vita_di_un_job() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.paths.input_dir = dir.path().join("in");
        config.paths.output_dir = dir.path().join("out");
        config.paths.data_dir = dir.path().join("data");
        config.paths.log_dir = dir.path().join("logs");
        config.prepare_directories().unwrap();

        let state = AppState::new(config).await.unwrap();
        let view = state
            .create_job("prova".into(), EncodingParams::default(), 1)
            .await
            .unwrap();
        assert_eq!(state.list_views().await.len(), 1);
        assert!(state.job_input_dir(view.job.id).exists());

        state.delete_job(view.job.id).await.unwrap();
        assert!(state.list_views().await.is_empty());
        assert!(!state.job_input_dir(view.job.id).exists());
        assert!(state.view(view.job.id).await.is_err());
    }

    #[tokio::test]
    async fn i_job_in_esecuzione_tornano_pending_al_riavvio() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.paths.input_dir = dir.path().join("in");
        config.paths.output_dir = dir.path().join("out");
        config.paths.data_dir = dir.path().join("data");
        config.paths.log_dir = dir.path().join("logs");
        config.prepare_directories().unwrap();

        let store = JobStore::new(&config.paths.data_dir).unwrap();
        let mut job = Job::new("interrotto".into(), EncodingParams::default(), 1);
        job.status = JobStatus::Running;
        let mut file = JobFile::new(
            "a.mp4".into(),
            "a.mp4".into(),
            PathBuf::from("/tmp/a.mp4"),
            1,
        );
        file.status = FileStatus::Running;
        file.progress.percent = 42.0;
        job.files.push(file);
        store.save(&job).await.unwrap();

        let state = AppState::new(config).await.unwrap();
        let view = state.view(job.id).await.unwrap();
        assert_eq!(view.job.status, JobStatus::Pending);
        assert_eq!(view.job.files[0].status, FileStatus::Pending);
        assert_eq!(view.job.files[0].progress.percent, 0.0);
    }
}
