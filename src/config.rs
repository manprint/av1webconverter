//! Configurazione del backend, caricata da file TOML e sovrascrivibile
//! tramite variabili d'ambiente (prefisso `AV1WC_`).

use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Modalità di codifica supportate.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EncodingMode {
    /// Qualità costante: bitrate variabile, `tune` libero.
    #[default]
    Crf,
    /// Dimensione finale desiderata: VBR a due passaggi, impone `tune=0`.
    TargetSize,
}

/// Parametri di conversione. Sono i default di sistema letti dal file di
/// configurazione e, per ogni job, la copia editabile dal frontend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EncodingParams {
    pub mode: EncodingMode,
    /// Altezza massima dell'output; l'aspetto viene mantenuto e il SAR forzato
    /// a 1:1. Il valore `0` significa dimensione originale: nessun
    /// ridimensionamento e SAR del sorgente intatto.
    pub scale_height: u32,
    /// Constant Rate Factor, usato solo in modalità `crf`.
    pub crf: u8,
    pub preset: u8,
    pub audio_bitrate: String,
    /// Passato a `-svtav1-params`. In modalità `target_size` viene forzato `tune=0`.
    pub svtav1_params: String,
    pub pixel_format: String,
    /// Distanza massima fra keyframe (`-g`).
    pub keyint: u32,
    /// Numero di thread concessi a ffmpeg; `None` lascia decidere ffmpeg.
    pub cpus: Option<u32>,
    /// Periodo di aggiornamento del progresso, in secondi.
    pub progress_interval: f64,
    /// Dimensione finale desiderata in MB, obbligatoria in modalità `target_size`.
    pub target_size_mb: Option<f64>,
    /// Percentuale del target riservata all'overhead del contenitore MP4.
    pub size_margin_percent: f64,
    /// Testo aggiunto al nome del file convertito, prima dell'estensione MP4.
    pub suffix: String,
    /// Se falso, un output già presente viene saltato invece che riscritto.
    pub overwrite: bool,
}

impl Default for EncodingParams {
    fn default() -> Self {
        Self {
            mode: EncodingMode::Crf,
            scale_height: 480,
            crf: 47,
            preset: 1,
            audio_bitrate: "48k".to_string(),
            svtav1_params: "tune=5".to_string(),
            pixel_format: "yuv420p10le".to_string(),
            keyint: 240,
            cpus: None,
            progress_interval: 5.0,
            target_size_mb: None,
            size_margin_percent: 1.0,
            suffix: "_av1".to_string(),
            overwrite: false,
        }
    }
}

impl EncodingParams {
    /// Verifica i parametri prima di accettarli da config o dal frontend.
    pub fn validate(&self) -> Result<(), String> {
        // 0 è la sentinella di "dimensione originale": nessun ridimensionamento.
        if self.scale_height != 0 && (self.scale_height < 16 || self.scale_height > 4320) {
            return Err(
                "scale_height deve essere 0 (dimensione originale) oppure compreso fra 16 e 4320"
                    .into(),
            );
        }
        if self.crf > 63 {
            return Err("crf deve essere compreso fra 0 e 63".into());
        }
        if self.preset > 13 {
            return Err("preset deve essere compreso fra 0 e 13".into());
        }
        if parse_bitrate(&self.audio_bitrate).is_none() {
            return Err(format!(
                "audio_bitrate non valido: {} (attesi formati come 48k, 128k, 96000)",
                self.audio_bitrate
            ));
        }
        if !is_safe_ffmpeg_token(&self.svtav1_params) {
            return Err("svtav1_params contiene caratteri non ammessi".into());
        }
        if !is_safe_ffmpeg_token(&self.pixel_format) {
            return Err("pixel_format contiene caratteri non ammessi".into());
        }
        if self.keyint == 0 || self.keyint > 3000 {
            return Err("keyint deve essere compreso fra 1 e 3000".into());
        }
        if let Some(cpus) = self.cpus {
            if cpus == 0 || cpus > 256 {
                return Err("cpus deve essere compreso fra 1 e 256".into());
            }
        }
        if !(self.progress_interval.is_finite() && self.progress_interval > 0.0) {
            return Err("progress_interval deve essere un numero maggiore di zero".into());
        }
        if !(0.0..=50.0).contains(&self.size_margin_percent) {
            return Err("size_margin_percent deve essere compreso fra 0 e 50".into());
        }
        if !is_safe_output_suffix(&self.suffix) {
            return Err(
                "suffix può contenere al massimo 128 caratteri ASCII tra lettere, numeri, punto, trattino e underscore"
                    .into(),
            );
        }
        if self.mode == EncodingMode::TargetSize {
            match self.target_size_mb {
                Some(size) if size.is_finite() && size > 0.0 => {}
                _ => {
                    return Err(
                        "target_size_mb è obbligatorio e maggiore di zero in modalità target_size"
                            .into(),
                    )
                }
            }
        }
        Ok(())
    }

    /// `tune=5` è accettato da SVT-AV1 solo in CRF: in VBR a due passaggi si
    /// ricade su `tune=0`, l'unico compatibile.
    pub fn effective_svtav1_params(&self) -> String {
        match self.mode {
            EncodingMode::Crf => self.svtav1_params.clone(),
            EncodingMode::TargetSize => "tune=0".to_string(),
        }
    }
}

/// Converte "48k" / "48K" / "48000" in bit al secondo.
pub fn parse_bitrate(value: &str) -> Option<u64> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let (digits, multiplier) = match trimmed.chars().last()? {
        'k' | 'K' => (&trimmed[..trimmed.len() - 1], 1_000_u64),
        'm' | 'M' => (&trimmed[..trimmed.len() - 1], 1_000_000_u64),
        _ => (trimmed, 1),
    };
    let number: f64 = digits.parse().ok()?;
    if !number.is_finite() || number <= 0.0 {
        return None;
    }
    Some((number * multiplier as f64).round() as u64)
}

/// I valori che finiscono nella riga di comando di ffmpeg non passano da una
/// shell, ma restano comunque limitati a un alfabeto prevedibile.
fn is_safe_ffmpeg_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "=:,.-_+/ ".contains(c))
}

/// Il suffisso diventa parte di un nome file, quindi non può introdurre percorsi.
fn is_safe_output_suffix(value: &str) -> bool {
    value.len() <= 128
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServerConfig {
    pub bind: IpAddr,
    pub port: u16,
    /// Dimensione massima di una singola richiesta di upload, in MB.
    pub max_upload_mb: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            port: 8080,
            max_upload_mb: 8192,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AuthConfig {
    pub enabled: bool,
    pub username: String,
    pub password: String,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            username: "admin".to_string(),
            password: "changeme".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PathsConfig {
    /// Cartella dei file caricati, una sottocartella per job.
    pub input_dir: PathBuf,
    /// Cartella dei file convertiti, una sottocartella per job.
    pub output_dir: PathBuf,
    /// Stato dei job persistito su disco.
    pub data_dir: PathBuf,
    /// Log applicativi e log di conversione per file.
    pub log_dir: PathBuf,
}

impl Default for PathsConfig {
    fn default() -> Self {
        Self {
            input_dir: PathBuf::from("in"),
            output_dir: PathBuf::from("out"),
            data_dir: PathBuf::from("data"),
            log_dir: PathBuf::from("logs"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FfmpegConfig {
    pub ffmpeg_bin: PathBuf,
    pub ffprobe_bin: PathBuf,
}

impl Default for FfmpegConfig {
    fn default() -> Self {
        Self {
            ffmpeg_bin: PathBuf::from("ffmpeg"),
            ffprobe_bin: PathBuf::from("ffprobe"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct JobsConfig {
    /// Conversioni simultanee dentro un singolo job.
    pub default_concurrency: usize,
    /// Estensioni accettate in upload. Oggi MP4, la lista è estendibile.
    pub allowed_extensions: Vec<String>,
    /// Righe di log per file mantenute in memoria per la vista live.
    pub log_buffer_lines: usize,
}

impl Default for JobsConfig {
    fn default() -> Self {
        Self {
            default_concurrency: 1,
            allowed_extensions: vec![
                "mp4".into(),
                "mov".into(),
                "mkv".into(),
                "avi".into(),
                "webm".into(),
                "m4v".into(),
                "ts".into(),
                "mpg".into(),
                "mpeg".into(),
                "wmv".into(),
            ],
            log_buffer_lines: 500,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SlackConfig {
    pub enabled: bool,
    pub webhook_url: String,
    pub notify_on_job_completed: bool,
    pub notify_on_job_failed: bool,
    pub notify_on_file_failed: bool,
}

impl Default for SlackConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            webhook_url: String::new(),
            notify_on_job_completed: true,
            notify_on_job_failed: true,
            notify_on_file_failed: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LoggingConfig {
    /// Filtro `tracing` (es. `info`, `av1webconverter=debug,tower_http=info`).
    pub level: String,
    /// Log strutturati JSON su file, oltre al log leggibile su stdout.
    pub json_file: bool,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            json_file: true,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub server: ServerConfig,
    pub auth: AuthConfig,
    pub paths: PathsConfig,
    pub ffmpeg: FfmpegConfig,
    pub jobs: JobsConfig,
    pub slack: SlackConfig,
    pub logging: LoggingConfig,
    /// Parametri di conversione proposti a ogni nuovo job.
    pub encoding: EncodingParams,
}

impl Config {
    /// Carica la configurazione dal percorso indicato; se il file non esiste
    /// vengono usati i default.
    pub fn load(path: Option<&Path>) -> Result<Self> {
        let mut config = match path {
            Some(path) if path.exists() => {
                let text = std::fs::read_to_string(path)
                    .with_context(|| format!("lettura configurazione {}", path.display()))?;
                toml::from_str(&text)
                    .with_context(|| format!("parsing configurazione {}", path.display()))?
            }
            Some(path) => {
                tracing::warn!(path = %path.display(), "file di configurazione assente, uso i default");
                Self::default()
            }
            None => Self::default(),
        };
        config.apply_env_overrides();
        config.validate()?;
        Ok(config)
    }

    /// Le variabili d'ambiente hanno la precedenza sul file: servono a
    /// configurare il container senza montare un TOML.
    fn apply_env_overrides(&mut self) {
        use std::env::var;

        if let Ok(value) = var("AV1WC_BIND") {
            if let Ok(parsed) = value.parse() {
                self.server.bind = parsed;
            }
        }
        if let Ok(value) = var("AV1WC_PORT") {
            if let Ok(parsed) = value.parse() {
                self.server.port = parsed;
            }
        }
        if let Ok(value) = var("AV1WC_MAX_UPLOAD_MB") {
            if let Ok(parsed) = value.parse() {
                self.server.max_upload_mb = parsed;
            }
        }
        if let Ok(value) = var("AV1WC_AUTH_ENABLED") {
            self.auth.enabled = matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes");
        }
        if let Ok(value) = var("AV1WC_AUTH_USERNAME") {
            self.auth.username = value;
        }
        if let Ok(value) = var("AV1WC_AUTH_PASSWORD") {
            self.auth.password = value;
        }
        if let Ok(value) = var("AV1WC_INPUT_DIR") {
            self.paths.input_dir = PathBuf::from(value);
        }
        if let Ok(value) = var("AV1WC_OUTPUT_DIR") {
            self.paths.output_dir = PathBuf::from(value);
        }
        if let Ok(value) = var("AV1WC_DATA_DIR") {
            self.paths.data_dir = PathBuf::from(value);
        }
        if let Ok(value) = var("AV1WC_LOG_DIR") {
            self.paths.log_dir = PathBuf::from(value);
        }
        if let Ok(value) = var("AV1WC_FFMPEG_BIN") {
            self.ffmpeg.ffmpeg_bin = PathBuf::from(value);
        }
        if let Ok(value) = var("AV1WC_FFPROBE_BIN") {
            self.ffmpeg.ffprobe_bin = PathBuf::from(value);
        }
        if let Ok(value) = var("AV1WC_DEFAULT_CONCURRENCY") {
            if let Ok(parsed) = value.parse() {
                self.jobs.default_concurrency = parsed;
            }
        }
        if let Ok(value) = var("AV1WC_SLACK_WEBHOOK_URL") {
            self.slack.webhook_url = value;
            self.slack.enabled = !self.slack.webhook_url.is_empty();
        }
        if let Ok(value) = var("AV1WC_SLACK_ENABLED") {
            self.slack.enabled =
                matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes");
        }
        if let Ok(value) = var("AV1WC_LOG_LEVEL") {
            self.logging.level = value;
        }
        if let Ok(value) = var("AV1WC_CRF") {
            if let Ok(parsed) = value.parse() {
                self.encoding.crf = parsed;
            }
        }
        if let Ok(value) = var("AV1WC_PRESET") {
            if let Ok(parsed) = value.parse() {
                self.encoding.preset = parsed;
            }
        }
        if let Ok(value) = var("AV1WC_SCALE_HEIGHT") {
            if let Ok(parsed) = value.parse() {
                self.encoding.scale_height = parsed;
            }
        }
        if let Ok(value) = var("AV1WC_AUDIO_BITRATE") {
            self.encoding.audio_bitrate = value;
        }
        if let Ok(value) = var("AV1WC_SVTAV1_PARAMS") {
            self.encoding.svtav1_params = value;
        }
        if let Ok(value) = var("AV1WC_PROGRESS_INTERVAL") {
            if let Ok(parsed) = value.parse() {
                self.encoding.progress_interval = parsed;
            }
        }
    }

    fn validate(&self) -> Result<()> {
        self.encoding.validate().map_err(|message| {
            anyhow::anyhow!("parametri di codifica di default non validi: {message}")
        })?;
        if self.jobs.default_concurrency == 0 || self.jobs.default_concurrency > 64 {
            anyhow::bail!("jobs.default_concurrency deve essere compreso fra 1 e 64");
        }
        if self.auth.enabled && (self.auth.username.is_empty() || self.auth.password.is_empty()) {
            anyhow::bail!("auth abilitata ma username o password sono vuoti");
        }
        if self.slack.enabled && !self.slack.webhook_url.starts_with("https://") {
            anyhow::bail!("slack.webhook_url deve essere un URL https quando slack è abilitato");
        }
        Ok(())
    }

    /// Crea le cartelle di lavoro e le riporta in forma assoluta.
    pub fn prepare_directories(&mut self) -> Result<()> {
        for dir in [
            &mut self.paths.input_dir,
            &mut self.paths.output_dir,
            &mut self.paths.data_dir,
            &mut self.paths.log_dir,
        ] {
            std::fs::create_dir_all(&*dir)
                .with_context(|| format!("creazione cartella {}", dir.display()))?;
            *dir = dir
                .canonicalize()
                .with_context(|| format!("risoluzione cartella {}", dir.display()))?;
        }
        std::fs::create_dir_all(self.paths.data_dir.join("jobs"))?;
        std::fs::create_dir_all(self.paths.data_dir.join("tmp"))?;
        std::fs::create_dir_all(self.paths.log_dir.join("jobs"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bitrate_accetta_suffissi() {
        assert_eq!(parse_bitrate("48k"), Some(48_000));
        assert_eq!(parse_bitrate("128K"), Some(128_000));
        assert_eq!(parse_bitrate("1.5M"), Some(1_500_000));
        assert_eq!(parse_bitrate("96000"), Some(96_000));
        assert_eq!(parse_bitrate(""), None);
        assert_eq!(parse_bitrate("abc"), None);
        assert_eq!(parse_bitrate("0k"), None);
    }

    #[test]
    fn parametri_di_default_sono_validi() {
        assert!(EncodingParams::default().validate().is_ok());
    }

    #[test]
    fn target_size_richiede_una_dimensione() {
        let mut params = EncodingParams {
            mode: EncodingMode::TargetSize,
            ..Default::default()
        };
        assert!(params.validate().is_err());
        params.target_size_mb = Some(300.0);
        assert!(params.validate().is_ok());
    }

    #[test]
    fn target_size_forza_tune_zero() {
        let params = EncodingParams {
            mode: EncodingMode::TargetSize,
            target_size_mb: Some(300.0),
            svtav1_params: "tune=5".into(),
            ..Default::default()
        };
        assert_eq!(params.effective_svtav1_params(), "tune=0");
    }

    #[test]
    fn parametri_svtav1_rifiutano_iniezioni() {
        let params = EncodingParams {
            svtav1_params: "tune=5; rm -rf /".into(),
            ..Default::default()
        };
        assert!(params.validate().is_err());
    }

    #[test]
    fn suffix_di_output_e_validato() {
        let mut params = EncodingParams::default();
        assert_eq!(params.suffix, "_av1");
        params.suffix = "_convertito-v2.1".into();
        assert!(params.validate().is_ok());
        params.suffix = "../fuori".into();
        assert!(params.validate().is_err());
    }

    #[test]
    fn altezza_zero_significa_dimensione_originale() {
        let params = EncodingParams {
            scale_height: 0,
            ..Default::default()
        };
        assert!(params.validate().is_ok());
    }

    #[test]
    fn altezza_fra_uno_e_quindici_e_rifiutata() {
        let params = EncodingParams {
            scale_height: 8,
            ..Default::default()
        };
        let error = params.validate().expect_err("8 deve essere rifiutato");
        assert!(error.contains("0 (dimensione originale)"));
    }

    #[test]
    fn altezza_oltre_il_massimo_e_rifiutata() {
        let params = EncodingParams {
            scale_height: 4321,
            ..Default::default()
        };
        assert!(params.validate().is_err());
    }

    #[test]
    fn altezza_zero_da_ambiente_e_accettata() {
        let mut config = Config::default();
        config.encoding.scale_height = 0;
        assert!(config.validate().is_ok());
    }

    #[test]
    fn config_di_default_e_valida() {
        assert!(Config::default().validate().is_ok());
    }
}
