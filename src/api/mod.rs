//! Composizione del router HTTP: frontend embedded, API dei job, SSE.

pub mod download;
pub mod events;
pub mod jobs;
pub mod upload;
pub mod web;

use axum::extract::DefaultBodyLimit;
use axum::routing::{delete, get, post};
use axum::{middleware, Json, Router};
use serde_json::json;
use tower_http::trace::TraceLayer;

use crate::auth::basic_auth;
use crate::state::SharedState;

pub fn router(state: SharedState) -> Router {
    let upload_limit = state
        .config
        .server
        .max_upload_mb
        .saturating_mul(1024 * 1024) as usize;

    let api = Router::new()
        .route("/defaults", get(jobs::defaults))
        .route("/jobs", get(jobs::list).post(jobs::create))
        .route(
            "/jobs/{job_id}",
            get(jobs::get).patch(jobs::update).delete(jobs::delete),
        )
        .route("/jobs/{job_id}/start", post(jobs::start))
        .route("/jobs/{job_id}/stop", post(jobs::stop))
        .route(
            "/jobs/{job_id}/files",
            post(upload::upload).layer(DefaultBodyLimit::max(upload_limit)),
        )
        .route("/jobs/{job_id}/files/{file_id}", delete(jobs::delete_file))
        .route("/jobs/{job_id}/files/{file_id}/log", get(jobs::file_log))
        .route(
            "/jobs/{job_id}/files/{file_id}/download",
            get(download::single_file),
        )
        .route(
            "/jobs/{job_id}/download",
            get(download::zip_all).post(download::zip_selection),
        )
        .route("/events", get(events::stream));

    let protected = Router::new()
        .merge(web::router())
        .nest("/api", api)
        .layer(middleware::from_fn_with_state(state.clone(), basic_auth));

    Router::new()
        // Il probe di salute resta accessibile senza credenziali.
        .route("/healthz", get(health))
        .merge(protected)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok", "version": env!("CARGO_PKG_VERSION") }))
}
