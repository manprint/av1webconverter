//! Basic auth sul frontend e sulle API, con credenziali da configurazione.

use axum::extract::{Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use subtle::ConstantTimeEq;

use crate::config::AuthConfig;
use crate::state::SharedState;

const REALM: &str = "av1webconverter";

pub async fn basic_auth(
    State(state): State<SharedState>,
    request: Request,
    next: Next,
) -> Response {
    if !state.config.auth.enabled {
        return next.run(request).await;
    }

    let header_value = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok());

    match header_value.and_then(parse_basic_header) {
        Some((username, password))
            if credentials_match(&state.config.auth, &username, &password) =>
        {
            next.run(request).await
        }
        Some(_) => {
            tracing::warn!("tentativo di accesso con credenziali errate");
            unauthorized()
        }
        None => unauthorized(),
    }
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(
            header::WWW_AUTHENTICATE,
            format!("Basic realm=\"{REALM}\", charset=\"UTF-8\""),
        )],
        "autenticazione richiesta",
    )
        .into_response()
}

/// Estrae utente e password da un header `Authorization: Basic ...`.
pub fn parse_basic_header(value: &str) -> Option<(String, String)> {
    let encoded = value
        .strip_prefix("Basic ")
        .or_else(|| value.strip_prefix("basic "))?;
    let decoded = STANDARD.decode(encoded.trim()).ok()?;
    let decoded = String::from_utf8(decoded).ok()?;
    let (username, password) = decoded.split_once(':')?;
    Some((username.to_string(), password.to_string()))
}

/// Confronto a tempo costante: evita di distinguere le credenziali dal timing.
pub fn credentials_match(config: &AuthConfig, username: &str, password: &str) -> bool {
    let user_ok: bool = username.as_bytes().ct_eq(config.username.as_bytes()).into();
    let password_ok: bool = password.as_bytes().ct_eq(config.password.as_bytes()).into();
    user_ok & password_ok
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> AuthConfig {
        AuthConfig {
            enabled: true,
            username: "admin".into(),
            password: "segreta".into(),
        }
    }

    #[test]
    fn header_valido() {
        let encoded = STANDARD.encode("admin:segreta");
        let parsed = parse_basic_header(&format!("Basic {encoded}")).unwrap();
        assert_eq!(parsed, ("admin".to_string(), "segreta".to_string()));
    }

    #[test]
    fn password_con_due_punti() {
        let encoded = STANDARD.encode("admin:a:b:c");
        let parsed = parse_basic_header(&format!("Basic {encoded}")).unwrap();
        assert_eq!(parsed.1, "a:b:c");
    }

    #[test]
    fn header_non_valido() {
        assert!(parse_basic_header("Bearer abc").is_none());
        assert!(parse_basic_header("Basic !!!non-base64!!!").is_none());
        assert!(
            parse_basic_header(&format!("Basic {}", STANDARD.encode("senzaduepunti"))).is_none()
        );
    }

    #[test]
    fn credenziali_corrette_e_sbagliate() {
        assert!(credentials_match(&config(), "admin", "segreta"));
        assert!(!credentials_match(&config(), "admin", "sbagliata"));
        assert!(!credentials_match(&config(), "root", "segreta"));
        assert!(!credentials_match(&config(), "admin", "segret"));
    }
}
