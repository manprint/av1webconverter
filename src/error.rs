//! Errori dell'API con la loro traduzione in risposta HTTP JSON.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("{0}")]
    NotFound(String),

    #[error("{0}")]
    BadRequest(String),

    #[error("{0}")]
    Conflict(String),

    #[error("{0}")]
    Unauthorized(String),

    #[error("{0}")]
    PayloadTooLarge(String),

    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl ApiError {
    pub fn status(&self) -> StatusCode {
        match self {
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            Self::PayloadTooLarge(_) => StatusCode::PAYLOAD_TOO_LARGE,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status();
        // Il dettaglio interno resta nel log: al client va un messaggio generico.
        let message = match &self {
            Self::Internal(error) => {
                tracing::error!(error = %format!("{error:#}"), "errore interno");
                "errore interno del server".to_string()
            }
            other => other.to_string(),
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mappatura_degli_stati() {
        assert_eq!(
            ApiError::NotFound("x".into()).status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            ApiError::BadRequest("x".into()).status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            ApiError::Conflict("x".into()).status(),
            StatusCode::CONFLICT
        );
        assert_eq!(
            ApiError::Unauthorized("x".into()).status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            ApiError::Internal(anyhow::anyhow!("boom")).status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[test]
    fn gli_errori_interni_non_trapelano_nel_corpo() {
        let response = ApiError::Internal(anyhow::anyhow!("dettaglio riservato")).into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
