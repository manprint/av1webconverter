//! Frontend embedded nel binario: nessun asset da distribuire a parte.

use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;

use crate::state::SharedState;

const INDEX_HTML: &str = include_str!("../web/index.html");
const APP_JS: &str = include_str!("../web/app.js");
const STYLE_CSS: &str = include_str!("../web/style.css");
const FAVICON_SVG: &str = include_str!("../web/favicon.svg");
const VERSION_PLACEHOLDER: &str = "{{APP_VERSION}}";

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/", get(index))
        .route("/app.js", get(app_js))
        .route("/style.css", get(style_css))
        .route("/favicon.svg", get(favicon))
}

fn asset(content: &'static str, content_type: &'static str, cacheable: bool) -> Response {
    let cache_control = if cacheable {
        "public, max-age=3600"
    } else {
        "no-store"
    };
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, HeaderValue::from_static(content_type)),
            (
                header::CACHE_CONTROL,
                HeaderValue::from_static(cache_control),
            ),
        ],
        content,
    )
        .into_response()
}

async fn index() -> Response {
    (
        StatusCode::OK,
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/html; charset=utf-8"),
            ),
            (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
        ],
        INDEX_HTML.replace(VERSION_PLACEHOLDER, env!("CARGO_PKG_VERSION")),
    )
        .into_response()
}

async fn app_js() -> Response {
    asset(APP_JS, "application/javascript; charset=utf-8", true)
}

async fn style_css() -> Response {
    asset(STYLE_CSS, "text/css; charset=utf-8", true)
}

async fn favicon() -> Response {
    asset(FAVICON_SVG, "image/svg+xml", true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gli_asset_incorporano_i_campi_svt_dedicati() {
        for key in ["sharpness", "luminance_qp_bias"] {
            assert!(
                INDEX_HTML.contains(&format!("name=\"{key}\"")),
                "campo {key} assente dal form"
            );
            assert!(
                APP_JS.contains(&format!("key: \"{key}\"")),
                "campo {key} assente dai parametri"
            );
        }
    }

    #[test]
    fn gli_asset_sono_versionati_contro_la_cache() {
        assert!(INDEX_HTML.contains("/app.js?v={{APP_VERSION}}"));
        assert!(INDEX_HTML.contains("/style.css?v={{APP_VERSION}}"));
    }
}
