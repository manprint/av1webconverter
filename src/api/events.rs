//! Flusso di eventi verso il frontend (Server-Sent Events).

use std::convert::Infallible;
use std::time::Duration;

use axum::extract::State;
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use futures::stream::Stream;
use futures::StreamExt;
use tokio_stream::wrappers::BroadcastStream;

use crate::state::SharedState;

pub async fn stream(
    State(state): State<SharedState>,
) -> Sse<impl Stream<Item = Result<SseEvent, Infallible>>> {
    let receiver = state.events.subscribe();

    let stream = BroadcastStream::new(receiver).filter_map(|event| async move {
        match event {
            Ok(event) => match SseEvent::default().json_data(&event) {
                Ok(sse) => Some(Ok(sse)),
                Err(error) => {
                    tracing::error!(%error, "serializzazione evento SSE fallita");
                    None
                }
            },
            // Un client lento salta gli eventi intermedi: i successivi snapshot
            // `job_updated` lo riportano allineato.
            Err(error) => {
                tracing::debug!(%error, "client SSE in ritardo, eventi saltati");
                None
            }
        }
    });

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}
