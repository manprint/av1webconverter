//! API dei job: creazione, modifica parametri, avvio, stop, eliminazione.

use axum::extract::{Path, State};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::config::EncodingParams;
use crate::error::{ApiError, ApiResult};
use crate::models::{JobStatus, JobView};
use crate::state::SharedState;
use crate::worker;

/// Valori proposti dal backend a ogni nuovo job.
#[derive(Debug, Serialize)]
pub struct Defaults {
    pub params: EncodingParams,
    pub concurrency: usize,
    pub allowed_extensions: Vec<String>,
    pub max_upload_mb: u64,
}

pub async fn defaults(State(state): State<SharedState>) -> Json<Defaults> {
    Json(Defaults {
        params: state.config.encoding.clone(),
        concurrency: state.config.jobs.default_concurrency,
        allowed_extensions: state.config.jobs.allowed_extensions.clone(),
        max_upload_mb: state.config.server.max_upload_mb,
    })
}

#[derive(Debug, Deserialize)]
pub struct CreateJobRequest {
    pub name: String,
    /// Se assente si usano i default del backend.
    pub params: Option<EncodingParams>,
    pub concurrency: Option<usize>,
}

#[derive(Debug, Default, Deserialize)]
pub struct UpdateJobRequest {
    pub name: Option<String>,
    pub params: Option<EncodingParams>,
    pub concurrency: Option<usize>,
}

pub async fn list(State(state): State<SharedState>) -> Json<Vec<JobView>> {
    Json(state.list_views().await)
}

pub async fn get(
    State(state): State<SharedState>,
    Path(job_id): Path<Uuid>,
) -> ApiResult<Json<JobView>> {
    Ok(Json(state.view(job_id).await?))
}

pub async fn create(
    State(state): State<SharedState>,
    Json(request): Json<CreateJobRequest>,
) -> ApiResult<Json<JobView>> {
    let name = validate_name(&request.name)?;
    let params = request
        .params
        .unwrap_or_else(|| state.config.encoding.clone());
    params.validate().map_err(ApiError::BadRequest)?;
    let concurrency = validate_concurrency(
        request
            .concurrency
            .unwrap_or(state.config.jobs.default_concurrency),
    )?;

    Ok(Json(state.create_job(name, params, concurrency).await?))
}

pub async fn update(
    State(state): State<SharedState>,
    Path(job_id): Path<Uuid>,
    Json(request): Json<UpdateJobRequest>,
) -> ApiResult<Json<JobView>> {
    let name = request.name.as_deref().map(validate_name).transpose()?;
    if let Some(params) = &request.params {
        params.validate().map_err(ApiError::BadRequest)?;
    }
    let concurrency = request.concurrency.map(validate_concurrency).transpose()?;

    state
        .mutate(job_id, move |job| {
            // I parametri di un job in corso sono già in uso da ffmpeg.
            if job.status == JobStatus::Running
                && (request.params.is_some() || concurrency.is_some())
            {
                return Err(ApiError::Conflict(
                    "i parametri non sono modificabili mentre il job è in esecuzione".into(),
                ));
            }
            if let Some(name) = name {
                job.name = name;
            }
            if let Some(params) = request.params {
                job.params = params;
            }
            if let Some(concurrency) = concurrency {
                job.concurrency = concurrency;
            }
            Ok(())
        })
        .await?;

    Ok(Json(state.view(job_id).await?))
}

pub async fn start(
    State(state): State<SharedState>,
    Path(job_id): Path<Uuid>,
) -> ApiResult<Json<JobView>> {
    worker::start_job(state.clone(), job_id).await?;
    Ok(Json(state.view(job_id).await?))
}

pub async fn stop(
    State(state): State<SharedState>,
    Path(job_id): Path<Uuid>,
) -> ApiResult<Json<JobView>> {
    worker::stop_job(&state, job_id).await?;
    Ok(Json(state.view(job_id).await?))
}

pub async fn delete(
    State(state): State<SharedState>,
    Path(job_id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    state.delete_job(job_id).await?;
    Ok(Json(json!({ "deleted": job_id })))
}

pub async fn delete_file(
    State(state): State<SharedState>,
    Path((job_id, file_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<JobView>> {
    state.remove_file(job_id, file_id).await?;
    Ok(Json(state.view(job_id).await?))
}

/// Log di conversione di un singolo file.
pub async fn file_log(
    State(state): State<SharedState>,
    Path((job_id, file_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<serde_json::Value>> {
    let view = state.view(job_id).await?;
    if view.job.file(file_id).is_none() {
        return Err(ApiError::NotFound(format!("file {file_id} non trovato")));
    }
    let lines = state.logs.lines(job_id, file_id).await;
    Ok(Json(json!({ "file_id": file_id, "lines": lines })))
}

pub fn validate_name(name: &str) -> ApiResult<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(ApiError::BadRequest(
            "il nome del job è obbligatorio".into(),
        ));
    }
    if trimmed.chars().count() > 200 {
        return Err(ApiError::BadRequest(
            "il nome del job non può superare i 200 caratteri".into(),
        ));
    }
    Ok(trimmed.to_string())
}

pub fn validate_concurrency(value: usize) -> ApiResult<usize> {
    if value == 0 || value > 64 {
        return Err(ApiError::BadRequest(
            "la concorrenza deve essere compresa fra 1 e 64".into(),
        ));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nome_valido_viene_ripulito() {
        assert_eq!(validate_name("  serie tv  ").unwrap(), "serie tv");
    }

    #[test]
    fn nome_vuoto_rifiutato() {
        assert!(validate_name("   ").is_err());
        assert!(validate_name(&"x".repeat(201)).is_err());
    }

    #[test]
    fn concorrenza_nei_limiti() {
        assert_eq!(validate_concurrency(1).unwrap(), 1);
        assert_eq!(validate_concurrency(64).unwrap(), 64);
        assert!(validate_concurrency(0).is_err());
        assert!(validate_concurrency(65).is_err());
    }
}
