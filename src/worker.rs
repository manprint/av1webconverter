//! Esecuzione dei job: per ogni file probe, codifica AV1, remux MP4.
//!
//! I file di un job vengono elaborati nell'ordine di caricamento, con un numero
//! di conversioni simultanee pari a `job.concurrency` (default 1: il prossimo
//! file parte al termine del precedente).

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use anyhow::{Context, Result};
use futures::StreamExt;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::config::EncodingMode;
use crate::error::{ApiError, ApiResult};
use crate::events::Event;
use crate::ffmpeg::encode::{
    build_crf_args, build_remux_args, build_two_pass_args, compute_video_bitrate, run_ffmpeg,
    total_passes, FfmpegEvent, RunOutcome,
};
use crate::ffmpeg::probe::probe;
use crate::ffmpeg::progress::{format_progress_line, format_size, ProgressParser};
use crate::models::{FileStatus, Job, JobStatus};
use crate::state::{output_name_for, remove_file_if_present, JobHandle, SharedState};

/// Avvia il worker del job in background.
///
/// Fallisce se il job è già in esecuzione o se non ci sono file da convertire.
pub async fn start_job(state: SharedState, job_id: Uuid) -> ApiResult<()> {
    let handle = state.handle(job_id).await?;

    {
        let job = handle.job.read().await;
        if job.status == JobStatus::Running {
            return Err(ApiError::Conflict("il job è già in esecuzione".into()));
        }
        if job.files.is_empty() {
            return Err(ApiError::BadRequest(
                "il job non contiene file da convertire".into(),
            ));
        }
        if !job
            .files
            .iter()
            .any(|file| file.status == FileStatus::Pending)
        {
            return Err(ApiError::Conflict(
                "nessun file in attesa: tutti i file sono già stati elaborati".into(),
            ));
        }
    }

    if handle
        .worker_running
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err(ApiError::Conflict("il job è già in esecuzione".into()));
    }
    handle.reset_stop();

    tokio::spawn(async move {
        let handle_for_flag = handle.clone();
        if let Err(error) = run_job(state, handle).await {
            tracing::error!(%job_id, error = %format!("{error:#}"), "worker del job terminato con errore");
        }
        handle_for_flag
            .worker_running
            .store(false, Ordering::SeqCst);
    });

    Ok(())
}

/// Richiede l'interruzione del job e attende che il worker si fermi.
pub async fn stop_job(state: &SharedState, job_id: Uuid) -> ApiResult<()> {
    let handle = state.handle(job_id).await?;
    handle.request_stop();
    state.wait_worker_stopped(&handle).await;

    // Se il job non era in esecuzione, lo stato va comunque marcato come fermo.
    state
        .mutate(job_id, |job| {
            if job.status == JobStatus::Pending {
                job.status = JobStatus::Stopped;
                job.finished_at = Some(OffsetDateTime::now_utc());
            }
            Ok(())
        })
        .await?;
    Ok(())
}

async fn run_job(state: SharedState, handle: Arc<JobHandle>) -> Result<()> {
    let job_id = handle.id;

    let (files, concurrency) = {
        let mut job = handle.job.write().await;
        job.status = JobStatus::Running;
        job.error = None;
        job.finished_at = None;
        if job.started_at.is_none() {
            job.started_at = Some(OffsetDateTime::now_utc());
        }
        let pending: Vec<Uuid> = job
            .files
            .iter()
            .filter(|file| file.status == FileStatus::Pending)
            .map(|file| file.id)
            .collect();
        (pending, job.concurrency.max(1))
    };
    publish_snapshot(&state, &handle).await;
    state.events.publish(Event::JobStatusChanged {
        job_id,
        status: JobStatus::Running,
    });
    tracing::info!(%job_id, files = files.len(), concurrency, "avvio conversione job");

    // `for_each_concurrent` avvia i file nell'ordine di caricamento: con
    // concurrency = 1 si ottiene la sequenza richiesta dalle specifiche.
    futures::stream::iter(files)
        .for_each_concurrent(concurrency, |file_id| {
            let state = state.clone();
            let handle = handle.clone();
            async move {
                if handle.is_stop_requested() {
                    mark_cancelled(&state, &handle, file_id).await;
                    return;
                }
                if let Err(error) = convert_file(&state, &handle, file_id).await {
                    let message = format!("{error:#}");
                    tracing::error!(%job_id, %file_id, error = %message, "conversione fallita");
                    fail_file(&state, &handle, file_id, message).await;
                }
            }
        })
        .await;

    finalize_job(&state, &handle).await;
    Ok(())
}

/// Converte un singolo file: probe, codifica, remux, pubblicazione dell'output.
async fn convert_file(state: &SharedState, handle: &Arc<JobHandle>, file_id: Uuid) -> Result<()> {
    let job_id = handle.id;

    let (params, stored_name, original_name, input_path) = {
        let mut job = handle.job.write().await;
        let params = job.params.clone();
        let file = job
            .file_mut(file_id)
            .context("file rimosso durante l'esecuzione del job")?;
        file.status = FileStatus::Running;
        file.started_at = Some(OffsetDateTime::now_utc());
        file.error = None;
        file.progress = Default::default();
        (
            params,
            file.stored_name.clone(),
            file.original_name.clone(),
            file.input_path.clone(),
        )
    };
    persist(state, handle).await;
    state.events.publish(Event::FileStatusChanged {
        job_id,
        file_id,
        status: FileStatus::Running,
        error: None,
    });

    log_line(
        state,
        job_id,
        file_id,
        &format!("Avvio conversione: {original_name}"),
    )
    .await;

    let output_dir = state.job_output_dir(job_id);
    tokio::fs::create_dir_all(&output_dir)
        .await
        .context("creazione cartella di output del job")?;
    let output_path = output_dir.join(output_name_for(&stored_name, &params.suffix));

    // Come negli script: un output già presente non viene sovrascritto.
    if output_path.exists() && !params.overwrite {
        let size = tokio::fs::metadata(&output_path)
            .await
            .map(|meta| meta.len())
            .unwrap_or(0);
        log_line(
            state,
            job_id,
            file_id,
            &format!("Saltato (output già presente): {}", output_path.display()),
        )
        .await;
        finish_file(
            state,
            handle,
            file_id,
            FileStatus::Skipped,
            Some(output_path),
            Some(size),
            None,
        )
        .await;
        return Ok(());
    }

    let info = probe(&state.config.ffmpeg.ffprobe_bin, &input_path)
        .await
        .context("analisi del sorgente con ffprobe")?;
    log_line(
        state,
        job_id,
        file_id,
        &format!(
            "Sorgente: durata {:.2}s, {} frame stimati, audio {}",
            info.duration_seconds,
            info.estimated_frames,
            if info.has_audio {
                "presente"
            } else {
                "assente"
            }
        ),
    )
    .await;

    let temp_dir = state.temp_dir();
    tokio::fs::create_dir_all(&temp_dir)
        .await
        .context("creazione cartella temporanea")?;
    let temp_mkv = temp_dir.join(format!(".av1-{file_id}.mkv"));
    let temp_mp4 = temp_dir.join(format!(".av1-{file_id}.mp4"));
    let passlog = temp_dir.join(format!(".av1-{file_id}"));
    let guard = TempFiles::new(vec![temp_mkv.clone(), temp_mp4.clone()], passlog.clone());

    let passes = total_passes(params.mode);
    let mut cancel = handle.cancel_rx();

    for pass in 1..=passes {
        let args = match params.mode {
            EncodingMode::Crf => build_crf_args(&params, &input_path, &temp_mkv),
            EncodingMode::TargetSize => {
                let bitrate = compute_video_bitrate(&params, &info)?;
                if pass == 1 {
                    log_line(
                        state,
                        job_id,
                        file_id,
                        &format!(
                            "Dimensione target {} MB: bitrate video calcolato {} bit/s",
                            params.target_size_mb.unwrap_or_default(),
                            bitrate
                        ),
                    )
                    .await;
                }
                build_two_pass_args(&params, &input_path, &temp_mkv, &passlog, bitrate, pass)
            }
        };

        let parser =
            ProgressParser::new(info.duration_seconds, info.estimated_frames, pass, passes);
        let outcome = run_progress_run(state, handle, file_id, &args, parser, &mut cancel)
            .await
            .with_context(|| format!("codifica AV1 (passaggio {pass}/{passes})"))?;

        if outcome == RunOutcome::Cancelled {
            log_line(
                state,
                job_id,
                file_id,
                "Conversione interrotta su richiesta",
            )
            .await;
            mark_cancelled(state, handle, file_id).await;
            return Ok(());
        }
    }

    let encoded_size = tokio::fs::metadata(&temp_mkv)
        .await
        .map(|meta| meta.len())
        .unwrap_or(0);
    if encoded_size == 0 {
        anyhow::bail!("la codifica ha prodotto un file vuoto");
    }

    log_line(state, job_id, file_id, "Remux in MP4 (tag av01, faststart)").await;
    let remux_args = build_remux_args(&temp_mkv, &temp_mp4);
    let outcome = run_ffmpeg(
        &state.config.ffmpeg.ffmpeg_bin,
        &remux_args,
        None,
        &mut cancel,
        |event| {
            if let FfmpegEvent::Log(line) = event {
                tracing::debug!(%job_id, %file_id, "ffmpeg remux: {line}");
            }
        },
    )
    .await
    .context("remux del file MP4 finale")?;

    if outcome == RunOutcome::Cancelled {
        mark_cancelled(state, handle, file_id).await;
        return Ok(());
    }

    // Rinomina finale: l'output definitivo compare solo quando è completo.
    tokio::fs::rename(&temp_mp4, &output_path)
        .await
        .context("spostamento del file convertito nella cartella di output")?;
    let final_size = tokio::fs::metadata(&output_path)
        .await
        .map(|meta| meta.len())
        .unwrap_or(0);

    log_line(
        state,
        job_id,
        file_id,
        &format!(
            "Completato: {} — dimensione finale {}",
            output_path.display(),
            format_size(final_size)
        ),
    )
    .await;
    drop(guard);

    finish_file(
        state,
        handle,
        file_id,
        FileStatus::Completed,
        Some(output_path),
        Some(final_size),
        None,
    )
    .await;
    Ok(())
}

/// Esegue un passaggio ffmpeg propagando progresso e log verso il frontend.
async fn run_progress_run(
    state: &SharedState,
    handle: &Arc<JobHandle>,
    file_id: Uuid,
    args: &[std::ffi::OsString],
    parser: ProgressParser,
    cancel: &mut tokio::sync::watch::Receiver<bool>,
) -> Result<RunOutcome> {
    let job_id = handle.id;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<FfmpegEvent>();

    // Gli eventi ffmpeg arrivano da un callback sincrono: passano su un canale
    // per poter essere gestiti con codice asincrono (log su file, broadcast).
    let pump = {
        let state = state.clone();
        let handle = handle.clone();
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                match event {
                    FfmpegEvent::Progress(progress) => {
                        let line = format_progress_line("Codifica", &progress);
                        let overall = {
                            let mut job = handle.job.write().await;
                            if let Some(file) = job.file_mut(file_id) {
                                file.progress = progress.clone();
                            }
                            job.overall_progress()
                        };
                        log_line(&state, job_id, file_id, &line).await;
                        state.events.publish(Event::FileProgress {
                            job_id,
                            file_id,
                            progress,
                            overall_progress: overall,
                        });
                    }
                    FfmpegEvent::Log(line) => {
                        // SVT-AV1 scrive anche il proprio banner su stderr: resta
                        // nel log di conversione, ma non va segnalato come problema.
                        if is_ffmpeg_error_line(&line) {
                            tracing::warn!(%job_id, %file_id, "ffmpeg: {line}");
                        } else {
                            tracing::debug!(%job_id, %file_id, "ffmpeg: {line}");
                        }
                        log_line(&state, job_id, file_id, &format!("ffmpeg: {line}")).await;
                    }
                }
            }
        })
    };

    let outcome = run_ffmpeg(
        &state.config.ffmpeg.ffmpeg_bin,
        args,
        Some(parser),
        cancel,
        move |event| {
            let _ = tx.send(event);
        },
    )
    .await;

    // Il canale si chiude con il callback: attendere il pump garantisce che
    // tutte le righe siano state scritte prima di cambiare stato al file.
    let _ = pump.await;
    outcome
}

/// Distingue una riga di stderr che segnala un problema dalla diagnostica
/// informativa (il banner di SVT-AV1, per esempio).
fn is_ffmpeg_error_line(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    if lower.contains("[info]") {
        return false;
    }
    ["error", "failed", "invalid", "unable", "cannot", "no such"]
        .iter()
        .any(|marker| lower.contains(marker))
}

async fn finish_file(
    state: &SharedState,
    handle: &Arc<JobHandle>,
    file_id: Uuid,
    status: FileStatus,
    output_path: Option<PathBuf>,
    output_size: Option<u64>,
    error: Option<String>,
) {
    {
        let mut job = handle.job.write().await;
        if let Some(file) = job.file_mut(file_id) {
            file.status = status;
            file.finished_at = Some(OffsetDateTime::now_utc());
            file.output_path = output_path;
            file.output_size_bytes = output_size;
            file.error = error.clone();
            if status == FileStatus::Completed || status == FileStatus::Skipped {
                file.progress.percent = 100.0;
            }
        }
    }
    persist(state, handle).await;
    state.events.publish(Event::FileStatusChanged {
        job_id: handle.id,
        file_id,
        status,
        error,
    });
    publish_snapshot(state, handle).await;
}

async fn mark_cancelled(state: &SharedState, handle: &Arc<JobHandle>, file_id: Uuid) {
    finish_file(
        state,
        handle,
        file_id,
        FileStatus::Cancelled,
        None,
        None,
        None,
    )
    .await;
}

async fn fail_file(state: &SharedState, handle: &Arc<JobHandle>, file_id: Uuid, message: String) {
    log_line(state, handle.id, file_id, &format!("ERRORE: {message}")).await;
    let file_name = {
        let job = handle.job.read().await;
        job.file(file_id)
            .map(|file| file.original_name.clone())
            .unwrap_or_default()
    };
    finish_file(
        state,
        handle,
        file_id,
        FileStatus::Failed,
        None,
        None,
        Some(message.clone()),
    )
    .await;

    let job = handle.job.read().await.clone();
    state
        .slack
        .notify_file_failed(&job, &file_name, &message)
        .await;
}

/// Calcola lo stato finale del job e invia la notifica Slack di chiusura.
async fn finalize_job(state: &SharedState, handle: &Arc<JobHandle>) {
    let job = {
        let mut job = handle.job.write().await;
        let counts = job.counts();
        job.status = if counts.failed > 0 {
            JobStatus::Failed
        } else if counts.cancelled > 0 || handle.is_stop_requested() {
            JobStatus::Stopped
        } else {
            JobStatus::Completed
        };
        job.finished_at = Some(OffsetDateTime::now_utc());
        job.clone()
    };

    tracing::info!(
        job_id = %job.id,
        status = ?job.status,
        completati = job.counts().completed,
        falliti = job.counts().failed,
        "job terminato"
    );
    state.persist_and_publish(&job).await;
    state.events.publish(Event::JobStatusChanged {
        job_id: job.id,
        status: job.status,
    });
    state.slack.notify_job_finished(&job).await;
}

async fn log_line(state: &SharedState, job_id: Uuid, file_id: Uuid, line: &str) {
    let entry = state.logs.append(job_id, file_id, line).await;
    state.events.publish(Event::FileLog {
        job_id,
        file_id,
        line: entry,
    });
}

async fn persist(state: &SharedState, handle: &Arc<JobHandle>) {
    let job: Job = handle.job.read().await.clone();
    if let Err(error) = state.store.save(&job).await {
        tracing::error!(job_id = %job.id, error = %format!("{error:#}"), "persistenza job fallita");
    }
}

async fn publish_snapshot(state: &SharedState, handle: &Arc<JobHandle>) {
    let job: Job = handle.job.read().await.clone();
    state.events.publish(Event::JobUpdated {
        job: Box::new((&job).into()),
    });
}

/// Rimuove i file temporanei della conversione, anche in caso di errore.
struct TempFiles {
    paths: Vec<PathBuf>,
    passlog_prefix: PathBuf,
}

impl TempFiles {
    fn new(paths: Vec<PathBuf>, passlog_prefix: PathBuf) -> Self {
        Self {
            paths,
            passlog_prefix,
        }
    }
}

impl Drop for TempFiles {
    fn drop(&mut self) {
        let mut paths = std::mem::take(&mut self.paths);
        // ffmpeg genera i log del two-pass aggiungendo un suffisso al prefisso.
        for suffix in ["-0.log", "-0.log.mbtree"] {
            paths.push(PathBuf::from(format!(
                "{}{suffix}",
                self.passlog_prefix.display()
            )));
        }
        tokio::spawn(async move {
            for path in paths {
                remove_file_if_present_quiet(&path).await;
            }
        });
    }
}

async fn remove_file_if_present_quiet(path: &Path) {
    if tokio::fs::metadata(path).await.is_ok() {
        remove_file_if_present(path).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, EncodingParams};
    use crate::state::AppState;

    async fn state_di_prova(dir: &Path) -> SharedState {
        let mut config = Config::default();
        config.paths.input_dir = dir.join("in");
        config.paths.output_dir = dir.join("out");
        config.paths.data_dir = dir.join("data");
        config.paths.log_dir = dir.join("logs");
        config.prepare_directories().unwrap();
        AppState::new(config).await.unwrap()
    }

    #[tokio::test]
    async fn un_job_senza_file_non_parte() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_di_prova(dir.path()).await;
        let view = state
            .create_job("vuoto".into(), EncodingParams::default(), 1)
            .await
            .unwrap();
        let error = start_job(state.clone(), view.job.id).await.unwrap_err();
        assert!(matches!(error, ApiError::BadRequest(_)));
    }

    #[tokio::test]
    async fn stop_su_job_fermo_lo_marca_interrotto() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_di_prova(dir.path()).await;
        let view = state
            .create_job("fermo".into(), EncodingParams::default(), 1)
            .await
            .unwrap();
        stop_job(&state, view.job.id).await.unwrap();
        assert_eq!(
            state.view(view.job.id).await.unwrap().job.status,
            JobStatus::Stopped
        );
    }

    #[test]
    fn le_righe_informative_di_ffmpeg_non_sono_errori() {
        assert!(!is_ffmpeg_error_line(
            "Svt[info]: SVT [version]:\tSVT-AV1 Encoder Lib v4.2.0"
        ));
        assert!(!is_ffmpeg_error_line("Svt[info]: -------------------"));
        assert!(is_ffmpeg_error_line(
            "Svt[error]: Instance 1: Invalid tune flag"
        ));
        assert!(is_ffmpeg_error_line(
            "Error opening output file /tmp/a.mkv."
        ));
        assert!(is_ffmpeg_error_line("prova.mp4: No such file or directory"));
    }

    #[tokio::test]
    async fn avviare_un_job_inesistente_e_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let state = state_di_prova(dir.path()).await;
        let error = start_job(state.clone(), Uuid::new_v4()).await.unwrap_err();
        assert!(matches!(error, ApiError::NotFound(_)));
    }
}
