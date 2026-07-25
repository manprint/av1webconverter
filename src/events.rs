//! Eventi push verso il frontend, distribuiti via SSE.

use serde::Serialize;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::models::{FileStatus, JobStatus, JobView, Progress};

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    /// Snapshot completo del job: usato a ogni cambio di stato strutturale.
    JobUpdated {
        job: Box<JobView>,
    },
    JobDeleted {
        job_id: Uuid,
    },
    /// Aggiornamento leggero ad alta frequenza durante la codifica.
    FileProgress {
        job_id: Uuid,
        file_id: Uuid,
        progress: Progress,
        overall_progress: f64,
    },
    FileStatusChanged {
        job_id: Uuid,
        file_id: Uuid,
        status: FileStatus,
        error: Option<String>,
    },
    JobStatusChanged {
        job_id: Uuid,
        status: JobStatus,
    },
    /// Riga di log di conversione, la stessa mostrata dagli script bash.
    FileLog {
        job_id: Uuid,
        file_id: Uuid,
        line: String,
    },
}

/// Canale broadcast condiviso. I client lenti perdono eventi intermedi
/// (`RecvError::Lagged`) ma restano allineati grazie agli snapshot `JobUpdated`.
#[derive(Debug, Clone)]
pub struct EventBus {
    sender: broadcast::Sender<Event>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    pub fn publish(&self, event: Event) {
        // Nessun sottoscrittore non è un errore: il backend lavora comunque.
        let _ = self.sender.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.sender.subscribe()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(1024)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn gli_eventi_raggiungono_i_sottoscrittori() {
        let bus = EventBus::new(8);
        let mut receiver = bus.subscribe();
        let job_id = Uuid::new_v4();
        bus.publish(Event::JobDeleted { job_id });
        match receiver.recv().await.unwrap() {
            Event::JobDeleted { job_id: received } => assert_eq!(received, job_id),
            other => panic!("evento inatteso: {other:?}"),
        }
    }

    #[test]
    fn pubblicare_senza_sottoscrittori_non_panica() {
        EventBus::new(4).publish(Event::JobDeleted {
            job_id: Uuid::new_v4(),
        });
    }

    #[test]
    fn serializzazione_con_discriminante() {
        let json = serde_json::to_value(Event::JobStatusChanged {
            job_id: Uuid::nil(),
            status: JobStatus::Running,
        })
        .unwrap();
        assert_eq!(json["type"], "job_status_changed");
        assert_eq!(json["status"], "running");
    }
}
