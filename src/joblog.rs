//! Log di conversione per singolo file: persistiti su disco e tenuti in un
//! buffer circolare in memoria per la vista live del frontend.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Debug)]
pub struct JobLogs {
    root: PathBuf,
    capacity: usize,
    buffers: Mutex<HashMap<Uuid, VecDeque<String>>>,
}

impl JobLogs {
    /// `log_dir` è la cartella log; i log di conversione finiscono in
    /// `log_dir/jobs/<job_id>/<file_id>.log`.
    pub fn new(log_dir: &Path, capacity: usize) -> Arc<Self> {
        Arc::new(Self {
            root: log_dir.join("jobs"),
            capacity: capacity.max(1),
            buffers: Mutex::new(HashMap::new()),
        })
    }

    pub fn file_log_path(&self, job_id: Uuid, file_id: Uuid) -> PathBuf {
        self.root
            .join(job_id.to_string())
            .join(format!("{file_id}.log"))
    }

    /// Aggiunge una riga con marca temporale, in memoria e su file.
    /// Un errore di scrittura non deve interrompere la conversione: viene
    /// registrato nel log applicativo e la riga resta comunque in memoria.
    pub async fn append(&self, job_id: Uuid, file_id: Uuid, line: &str) -> String {
        let timestamp = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_else(|_| "-".to_string());
        let entry = format!("{timestamp} {line}");

        {
            let mut buffers = self.buffers.lock().await;
            let buffer = buffers.entry(file_id).or_insert_with(VecDeque::new);
            if buffer.len() >= self.capacity {
                buffer.pop_front();
            }
            buffer.push_back(entry.clone());
        }

        let path = self.file_log_path(job_id, file_id);
        if let Err(error) = self.write_line(&path, &entry).await {
            tracing::error!(path = %path.display(), %error, "scrittura log di conversione fallita");
        }
        entry
    }

    async fn write_line(&self, path: &Path, entry: &str) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await?;
        file.write_all(entry.as_bytes()).await?;
        file.write_all(b"\n").await?;
        Ok(())
    }

    /// Righe disponibili per un file: dal buffer in memoria se presente,
    /// altrimenti rilette dal file (job ripresi dopo un riavvio).
    pub async fn lines(&self, job_id: Uuid, file_id: Uuid) -> Vec<String> {
        if let Some(buffer) = self.buffers.lock().await.get(&file_id) {
            if !buffer.is_empty() {
                return buffer.iter().cloned().collect();
            }
        }
        match tokio::fs::read_to_string(self.file_log_path(job_id, file_id)).await {
            Ok(content) => {
                let all: Vec<String> = content.lines().map(str::to_string).collect();
                let start = all.len().saturating_sub(self.capacity);
                all[start..].to_vec()
            }
            Err(_) => Vec::new(),
        }
    }

    /// Elimina i log di un intero job, memoria e disco.
    pub async fn purge_job(&self, job_id: Uuid, file_ids: &[Uuid]) {
        let mut buffers = self.buffers.lock().await;
        for file_id in file_ids {
            buffers.remove(file_id);
        }
        drop(buffers);
        let dir = self.root.join(job_id.to_string());
        if let Err(error) = tokio::fs::remove_dir_all(&dir).await {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(path = %dir.display(), %error, "rimozione log job fallita");
            }
        }
    }

    /// Elimina i log di un singolo file.
    pub async fn purge_file(&self, job_id: Uuid, file_id: Uuid) {
        self.buffers.lock().await.remove(&file_id);
        let path = self.file_log_path(job_id, file_id);
        if let Err(error) = tokio::fs::remove_file(&path).await {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(path = %path.display(), %error, "rimozione log file fallita");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn le_righe_finiscono_in_memoria_e_su_disco() {
        let dir = tempfile::tempdir().unwrap();
        let logs = JobLogs::new(dir.path(), 10);
        let job_id = Uuid::new_v4();
        let file_id = Uuid::new_v4();

        logs.append(job_id, file_id, "prima riga").await;
        logs.append(job_id, file_id, "seconda riga").await;

        let lines = logs.lines(job_id, file_id).await;
        assert_eq!(lines.len(), 2);
        assert!(lines[0].ends_with("prima riga"));

        let on_disk = tokio::fs::read_to_string(logs.file_log_path(job_id, file_id))
            .await
            .unwrap();
        assert_eq!(on_disk.lines().count(), 2);
    }

    #[tokio::test]
    async fn il_buffer_e_circolare() {
        let dir = tempfile::tempdir().unwrap();
        let logs = JobLogs::new(dir.path(), 3);
        let job_id = Uuid::new_v4();
        let file_id = Uuid::new_v4();
        for index in 0..10 {
            logs.append(job_id, file_id, &format!("riga {index}")).await;
        }
        let lines = logs.lines(job_id, file_id).await;
        assert_eq!(lines.len(), 3);
        assert!(lines[2].ends_with("riga 9"));
        // Su disco restano tutte.
        let on_disk = tokio::fs::read_to_string(logs.file_log_path(job_id, file_id))
            .await
            .unwrap();
        assert_eq!(on_disk.lines().count(), 10);
    }

    #[tokio::test]
    async fn le_righe_si_rileggono_dal_disco_senza_buffer() {
        let dir = tempfile::tempdir().unwrap();
        let job_id = Uuid::new_v4();
        let file_id = Uuid::new_v4();
        {
            let logs = JobLogs::new(dir.path(), 10);
            logs.append(job_id, file_id, "persistita").await;
        }
        let logs = JobLogs::new(dir.path(), 10);
        let lines = logs.lines(job_id, file_id).await;
        assert_eq!(lines.len(), 1);
        assert!(lines[0].ends_with("persistita"));
    }

    #[tokio::test]
    async fn purge_rimuove_tutto() {
        let dir = tempfile::tempdir().unwrap();
        let logs = JobLogs::new(dir.path(), 10);
        let job_id = Uuid::new_v4();
        let file_id = Uuid::new_v4();
        logs.append(job_id, file_id, "riga").await;
        logs.purge_job(job_id, &[file_id]).await;
        assert!(logs.lines(job_id, file_id).await.is_empty());
    }
}
