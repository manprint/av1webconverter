//! Download dei file convertiti: singolo file oppure archivio ZIP di una
//! selezione o dell'intero job.

use std::collections::HashSet;
use std::path::PathBuf;

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use tokio_util::io::ReaderStream;
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::state::{remove_file_if_present, SharedState};

#[derive(Debug, Deserialize)]
pub struct DownloadSelection {
    /// File richiesti; se vuoto o assente si scarica tutto il job.
    #[serde(default)]
    pub file_ids: Vec<Uuid>,
}

/// Download di un singolo file convertito.
pub async fn single_file(
    State(state): State<SharedState>,
    Path((job_id, file_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Response> {
    let view = state.view(job_id).await?;
    let file = view
        .job
        .file(file_id)
        .ok_or_else(|| ApiError::NotFound(format!("file {file_id} non trovato")))?;
    let output_path = file
        .output_path
        .clone()
        .ok_or_else(|| ApiError::Conflict("il file non è ancora stato convertito".into()))?;

    let download_name = crate::state::output_name_for(&file.stored_name);
    stream_file(output_path, &download_name, "video/mp4", false).await
}

/// Download dell'intero job come archivio ZIP.
pub async fn zip_all(
    State(state): State<SharedState>,
    Path(job_id): Path<Uuid>,
) -> ApiResult<Response> {
    build_zip(state, job_id, Vec::new()).await
}

/// Download di una selezione di file come archivio ZIP.
pub async fn zip_selection(
    State(state): State<SharedState>,
    Path(job_id): Path<Uuid>,
    Json(selection): Json<DownloadSelection>,
) -> ApiResult<Response> {
    build_zip(state, job_id, selection.file_ids).await
}

async fn build_zip(state: SharedState, job_id: Uuid, file_ids: Vec<Uuid>) -> ApiResult<Response> {
    let view = state.view(job_id).await?;
    let wanted: HashSet<Uuid> = file_ids.iter().copied().collect();

    let mut entries: Vec<(String, PathBuf)> = Vec::new();
    let mut used_names: Vec<String> = Vec::new();
    for file in view.job.downloadable_files() {
        if !wanted.is_empty() && !wanted.contains(&file.id) {
            continue;
        }
        let Some(path) = &file.output_path else {
            continue;
        };
        // Nomi duplicati dentro lo ZIP renderebbero l'archivio ambiguo.
        let name = crate::state::unique_stored_name(
            &crate::state::output_name_for(&file.stored_name),
            &used_names,
        );
        used_names.push(name.clone());
        entries.push((name, path.clone()));
    }

    if entries.is_empty() {
        return Err(ApiError::Conflict(
            "nessun file convertito disponibile per il download".into(),
        ));
    }

    let temp_dir = state.temp_dir();
    tokio::fs::create_dir_all(&temp_dir)
        .await
        .map_err(|error| ApiError::Internal(error.into()))?;
    let zip_path = temp_dir.join(format!(".download-{}.zip", Uuid::new_v4()));

    let zip_path_for_task = zip_path.clone();
    let result = tokio::task::spawn_blocking(move || write_zip(&zip_path_for_task, &entries))
        .await
        .map_err(|error| ApiError::Internal(anyhow::anyhow!("task ZIP interrotto: {error}")))?;

    if let Err(error) = result {
        remove_file_if_present(&zip_path).await;
        return Err(ApiError::Internal(error));
    }

    let archive_name = format!("{}.zip", sanitize_filename::sanitize(&view.job.name));
    stream_file(zip_path, &archive_name, "application/zip", true).await
}

/// Scrive lo ZIP senza ricomprimere: i video AV1 sono già compressi.
fn write_zip(zip_path: &std::path::Path, entries: &[(String, PathBuf)]) -> anyhow::Result<()> {
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    let file = std::fs::File::create(zip_path)?;
    let mut writer = zip::ZipWriter::new(std::io::BufWriter::new(file));
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .large_file(true);

    for (name, path) in entries {
        let mut source = std::fs::File::open(path)
            .map_err(|error| anyhow::anyhow!("apertura {}: {error}", path.display()))?;
        writer.start_file(name.as_str(), options)?;
        std::io::copy(&mut source, &mut writer)?;
    }
    writer.finish()?.flush()?;
    Ok(())
}

/// Invia un file in streaming. Con `delete_after` il file viene rimosso subito
/// dopo l'apertura: su Linux resta leggibile dal descrittore già aperto e non
/// lascia residui nella cartella temporanea.
async fn stream_file(
    path: PathBuf,
    download_name: &str,
    content_type: &str,
    delete_after: bool,
) -> ApiResult<Response> {
    let file = tokio::fs::File::open(&path).await.map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ApiError::NotFound("file non presente sul disco".into())
        } else {
            ApiError::Internal(error.into())
        }
    })?;
    let size = file.metadata().await.ok().map(|meta| meta.len());

    if delete_after {
        remove_file_if_present(&path).await;
    }

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(content_type)
            .unwrap_or(HeaderValue::from_static("application/octet-stream")),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&content_disposition(download_name))
            .unwrap_or(HeaderValue::from_static("attachment")),
    );
    if let Some(size) = size {
        if let Ok(value) = HeaderValue::from_str(&size.to_string()) {
            headers.insert(header::CONTENT_LENGTH, value);
        }
    }

    let body = Body::from_stream(ReaderStream::new(file));
    Ok((StatusCode::OK, headers, body).into_response())
}

/// `filename` ASCII per i client vecchi, `filename*` per i nomi Unicode.
pub fn content_disposition(name: &str) -> String {
    let ascii: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || " .-_()".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    let ascii = if ascii.trim().is_empty() {
        "download".to_string()
    } else {
        ascii
    };
    format!(
        "attachment; filename=\"{}\"; filename*=UTF-8''{}",
        ascii.replace('"', ""),
        percent_encode(name)
    )
}

fn percent_encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(*byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disposizione_con_nome_semplice() {
        let value = content_disposition("film_av1.mp4");
        assert!(value.contains("filename=\"film_av1.mp4\""));
        assert!(value.contains("filename*=UTF-8''film_av1.mp4"));
    }

    #[test]
    fn disposizione_con_nome_unicode() {
        let value = content_disposition("città notturna.mp4");
        assert!(value.contains("filename=\"citt__ notturna.mp4\"") || value.contains("citt"));
        assert!(value.contains("%C3%A0"));
    }

    #[test]
    fn disposizione_neutralizza_le_virgolette() {
        let value = content_disposition("a\"b.mp4");
        assert!(!value.contains("a\"b"));
    }

    #[test]
    fn percent_encoding_di_base() {
        assert_eq!(percent_encode("a b"), "a%20b");
        assert_eq!(percent_encode("film.mp4"), "film.mp4");
    }

    #[test]
    fn zip_creato_con_i_file_indicati() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("a.mp4");
        std::fs::write(&source, b"contenuto").unwrap();
        let zip_path = dir.path().join("out.zip");

        write_zip(&zip_path, &[("a_av1.mp4".to_string(), source)]).unwrap();

        let file = std::fs::File::open(&zip_path).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        assert_eq!(archive.len(), 1);
        assert_eq!(archive.by_index(0).unwrap().name(), "a_av1.mp4");
    }

    #[test]
    fn zip_fallisce_se_manca_un_sorgente() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("out.zip");
        let missing = dir.path().join("assente.mp4");
        assert!(write_zip(&zip_path, &[("x.mp4".to_string(), missing)]).is_err());
    }
}
