//! Upload di file e cartelle dentro un job.
//!
//! Ogni parte del multipart viene scritta in streaming su un file temporaneo e
//! solo a trasferimento completo viene spostata nella cartella `in` del job.

use axum::extract::{Multipart, Path, State};
use axum::Json;
use serde::Serialize;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::models::JobView;
use crate::state::{remove_file_if_present, SharedState};

#[derive(Debug, Serialize)]
pub struct UploadResponse {
    pub uploaded: usize,
    pub rejected: Vec<RejectedFile>,
    pub job: JobView,
}

#[derive(Debug, Serialize)]
pub struct RejectedFile {
    pub name: String,
    pub reason: String,
}

pub async fn upload(
    State(state): State<SharedState>,
    Path(job_id): Path<Uuid>,
    mut multipart: Multipart,
) -> ApiResult<Json<UploadResponse>> {
    // Verifica l'esistenza del job prima di scrivere qualunque byte.
    state.handle(job_id).await?;

    let temp_dir = state.temp_dir();
    tokio::fs::create_dir_all(&temp_dir)
        .await
        .map_err(|error| ApiError::Internal(error.into()))?;

    let mut uploaded = 0usize;
    let mut rejected = Vec::new();

    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|error| ApiError::BadRequest(format!("multipart non valido: {error}")))?
    {
        let Some(original_name) = field.file_name().map(str::to_string) else {
            continue;
        };
        if original_name.trim().is_empty() {
            continue;
        }

        if let Err(reason) = check_extension(&original_name, &state.config.jobs.allowed_extensions)
        {
            // Il corpo della parte va comunque consumato per poter proseguire.
            while field.chunk().await.ok().flatten().is_some() {}
            rejected.push(RejectedFile {
                name: original_name,
                reason,
            });
            continue;
        }

        let temp_path = temp_dir.join(format!(".upload-{}", Uuid::new_v4()));
        let mut written: u64 = 0;
        let mut file = tokio::fs::File::create(&temp_path)
            .await
            .map_err(|error| ApiError::Internal(error.into()))?;

        let mut failure: Option<String> = None;
        loop {
            match field.chunk().await {
                Ok(Some(chunk)) => {
                    if let Err(error) = file.write_all(&chunk).await {
                        failure = Some(format!("scrittura fallita: {error}"));
                        break;
                    }
                    written += chunk.len() as u64;
                }
                Ok(None) => break,
                Err(error) => {
                    failure = Some(format!("trasferimento interrotto: {error}"));
                    break;
                }
            }
        }
        let flushed = file.flush().await;
        drop(file);

        if let Err(error) = flushed {
            failure.get_or_insert(format!("chiusura del file fallita: {error}"));
        }
        if written == 0 {
            failure.get_or_insert("file vuoto".to_string());
        }

        if let Some(reason) = failure {
            remove_file_if_present(&temp_path).await;
            rejected.push(RejectedFile {
                name: original_name,
                reason,
            });
            continue;
        }

        match state
            .add_uploaded_file(job_id, &original_name, &temp_path, written)
            .await
        {
            Ok(file) => {
                uploaded += 1;
                tracing::info!(%job_id, file_id = %file.id, name = %file.original_name, bytes = written, "file caricato");
            }
            Err(error) => {
                remove_file_if_present(&temp_path).await;
                // Un job in esecuzione rifiuta l'intera richiesta.
                if matches!(error, ApiError::Conflict(_)) {
                    return Err(error);
                }
                rejected.push(RejectedFile {
                    name: original_name,
                    reason: error.to_string(),
                });
            }
        }
    }

    if uploaded == 0 && rejected.is_empty() {
        return Err(ApiError::BadRequest(
            "nessun file presente nella richiesta".into(),
        ));
    }

    Ok(Json(UploadResponse {
        uploaded,
        rejected,
        job: state.view(job_id).await?,
    }))
}

/// Oggi si accetta MP4; la lista di estensioni è configurabile per aprire ad
/// altri contenitori senza toccare il codice.
pub fn check_extension(name: &str, allowed: &[String]) -> Result<(), String> {
    let extension = name
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase());
    match extension {
        Some(extension)
            if allowed
                .iter()
                .any(|item| item.eq_ignore_ascii_case(&extension)) =>
        {
            Ok(())
        }
        Some(extension) => Err(format!(
            "estensione .{extension} non ammessa (ammesse: {})",
            allowed.join(", ")
        )),
        None => Err("il file non ha un'estensione riconoscibile".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allowed() -> Vec<String> {
        vec!["mp4".into(), "mkv".into()]
    }

    #[test]
    fn estensioni_ammesse() {
        assert!(check_extension("film.mp4", &allowed()).is_ok());
        assert!(check_extension("film.MP4", &allowed()).is_ok());
        assert!(check_extension("cartella/film.mkv", &allowed()).is_ok());
    }

    #[test]
    fn estensioni_rifiutate() {
        assert!(check_extension("archivio.zip", &allowed()).is_err());
        assert!(check_extension("senza_estensione", &allowed()).is_err());
        assert!(check_extension("script.sh", &allowed()).is_err());
    }
}
