//! Persistenza dei job su disco: un file JSON per job, scritto in modo atomico.
//!
//! Non serve un database: i job sono pochi, il volume di scritture è basso e uno
//! stato leggibile a mano è un vantaggio in fase di debug.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use uuid::Uuid;

use crate::models::Job;

#[derive(Debug, Clone)]
pub struct JobStore {
    dir: PathBuf,
}

impl JobStore {
    /// `data_dir` è la cartella dati; i job finiscono in `data_dir/jobs`.
    pub fn new(data_dir: &Path) -> Result<Self> {
        let dir = data_dir.join("jobs");
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("creazione cartella job {}", dir.display()))?;
        Ok(Self { dir })
    }

    fn path_for(&self, id: Uuid) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }

    /// Scrive su file temporaneo e rinomina: un'interruzione non lascia mai un
    /// JSON troncato al posto dello stato valido.
    pub async fn save(&self, job: &Job) -> Result<()> {
        let path = self.path_for(job.id);
        let temp = self.dir.join(format!(".{}.json.tmp", job.id));
        let json = serde_json::to_vec_pretty(job).context("serializzazione job")?;
        tokio::fs::write(&temp, &json)
            .await
            .with_context(|| format!("scrittura {}", temp.display()))?;
        tokio::fs::rename(&temp, &path)
            .await
            .with_context(|| format!("rinomina in {}", path.display()))?;
        Ok(())
    }

    pub async fn delete(&self, id: Uuid) -> Result<()> {
        match tokio::fs::remove_file(self.path_for(id)).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error).context("rimozione stato job"),
        }
    }

    /// Rilegge tutti i job presenti. Un file corrotto viene segnalato e saltato,
    /// senza impedire l'avvio dell'applicazione.
    pub async fn load_all(&self) -> Result<Vec<Job>> {
        let mut jobs = Vec::new();
        let mut entries = tokio::fs::read_dir(&self.dir)
            .await
            .with_context(|| format!("lettura cartella {}", self.dir.display()))?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with('.'))
            {
                continue;
            }
            match tokio::fs::read(&path).await {
                Ok(bytes) => match serde_json::from_slice::<Job>(&bytes) {
                    Ok(job) => jobs.push(job),
                    Err(error) => {
                        tracing::error!(path = %path.display(), %error, "stato job illeggibile, saltato")
                    }
                },
                Err(error) => {
                    tracing::error!(path = %path.display(), %error, "lettura stato job fallita")
                }
            }
        }
        jobs.sort_by_key(|job| job.created_at);
        Ok(jobs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::EncodingParams;

    #[tokio::test]
    async fn salva_rilegge_e_cancella() {
        let dir = tempfile::tempdir().unwrap();
        let store = JobStore::new(dir.path()).unwrap();

        let job = Job::new("primo".into(), EncodingParams::default(), 1);
        store.save(&job).await.unwrap();

        let loaded = store.load_all().await.unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, job.id);
        assert_eq!(loaded[0].name, "primo");

        store.delete(job.id).await.unwrap();
        assert!(store.load_all().await.unwrap().is_empty());
        // Cancellare due volte non è un errore.
        store.delete(job.id).await.unwrap();
    }

    #[tokio::test]
    async fn json_corrotto_non_blocca_il_caricamento() {
        let dir = tempfile::tempdir().unwrap();
        let store = JobStore::new(dir.path()).unwrap();
        let job = Job::new("valido".into(), EncodingParams::default(), 1);
        store.save(&job).await.unwrap();
        tokio::fs::write(dir.path().join("jobs/rotto.json"), b"{ non json")
            .await
            .unwrap();

        let loaded = store.load_all().await.unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "valido");
    }

    #[tokio::test]
    async fn i_job_sono_ordinati_per_creazione() {
        let dir = tempfile::tempdir().unwrap();
        let store = JobStore::new(dir.path()).unwrap();
        let mut primo = Job::new("primo".into(), EncodingParams::default(), 1);
        let mut secondo = Job::new("secondo".into(), EncodingParams::default(), 1);
        primo.created_at = time::OffsetDateTime::from_unix_timestamp(1_000).unwrap();
        secondo.created_at = time::OffsetDateTime::from_unix_timestamp(2_000).unwrap();
        store.save(&secondo).await.unwrap();
        store.save(&primo).await.unwrap();

        let loaded = store.load_all().await.unwrap();
        assert_eq!(loaded[0].name, "primo");
        assert_eq!(loaded[1].name, "secondo");
    }
}
