//! Interrogazione del sorgente con `ffprobe`, replicando le informazioni che
//! gli script bash di riferimento ricavano prima di codificare.

use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use tokio::process::Command;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaInfo {
    pub duration_seconds: f64,
    /// `avg_frame_rate` del primo stream video, come frazione originale.
    pub frame_rate: Option<(u64, u64)>,
    /// Frame stimati: durata × frame rate, usato per raffinare il progresso.
    pub estimated_frames: u64,
    pub has_audio: bool,
    /// Bitrate audio dichiarato dal sorgente, quando disponibile.
    pub source_audio_bitrate_bps: Option<u64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// Esegue `ffprobe` in JSON e ne ricava durata, frame rate e presenza audio.
pub async fn probe(ffprobe_bin: &Path, input: &Path) -> Result<MediaInfo> {
    let output = Command::new(ffprobe_bin)
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-show_entries",
            "stream=index,codec_type,codec_name,avg_frame_rate,bit_rate,width,height",
            "-of",
            "json",
            "-i",
        ])
        .arg(input)
        .kill_on_drop(true)
        .output()
        .await
        .with_context(|| format!("esecuzione di {}", ffprobe_bin.display()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "ffprobe è terminato con stato {:?}: {}",
            output.status.code(),
            stderr.trim()
        );
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout)
        .context("output di ffprobe non interpretabile come JSON")?;

    parse_probe_json(&json)
}

/// Separata dall'esecuzione del processo per poter essere testata su JSON fissi.
pub fn parse_probe_json(json: &serde_json::Value) -> Result<MediaInfo> {
    let duration_seconds = json
        .get("format")
        .and_then(|format| format.get("duration"))
        .and_then(|duration| duration.as_str())
        .and_then(|duration| duration.parse::<f64>().ok())
        .unwrap_or(0.0);

    if !(duration_seconds.is_finite() && duration_seconds > 0.0) {
        bail!("durata video non valida o non rilevabile");
    }

    let streams = json
        .get("streams")
        .and_then(|streams| streams.as_array())
        .map(|streams| streams.as_slice())
        .unwrap_or(&[]);

    let video = streams
        .iter()
        .find(|stream| stream.get("codec_type").and_then(|t| t.as_str()) == Some("video"));
    let audio = streams
        .iter()
        .find(|stream| stream.get("codec_type").and_then(|t| t.as_str()) == Some("audio"));

    if video.is_none() {
        bail!("il sorgente non contiene stream video");
    }

    let frame_rate = video
        .and_then(|stream| stream.get("avg_frame_rate"))
        .and_then(|rate| rate.as_str())
        .and_then(parse_frame_rate);

    let estimated_frames = frame_rate
        .map(|(numerator, denominator)| {
            (duration_seconds * numerator as f64 / denominator as f64).round() as u64
        })
        .unwrap_or(0);

    Ok(MediaInfo {
        duration_seconds,
        frame_rate,
        estimated_frames,
        has_audio: audio.is_some(),
        source_audio_bitrate_bps: audio
            .and_then(|stream| stream.get("bit_rate"))
            .and_then(|rate| rate.as_str())
            .and_then(|rate| rate.parse().ok()),
        width: video
            .and_then(|stream| stream.get("width"))
            .and_then(|value| value.as_u64())
            .map(|value| value as u32),
        height: video
            .and_then(|stream| stream.get("height"))
            .and_then(|value| value.as_u64())
            .map(|value| value as u32),
    })
}

/// `avg_frame_rate` arriva come "num/den"; "0/0" indica un valore assente.
fn parse_frame_rate(value: &str) -> Option<(u64, u64)> {
    let (numerator, denominator) = value.split_once('/')?;
    let numerator: u64 = numerator.trim().parse().ok()?;
    let denominator: u64 = denominator.trim().parse().ok()?;
    if numerator == 0 || denominator == 0 {
        return None;
    }
    Some((numerator, denominator))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_rate_valido() {
        assert_eq!(parse_frame_rate("30000/1001"), Some((30000, 1001)));
        assert_eq!(parse_frame_rate("25/1"), Some((25, 1)));
        assert_eq!(parse_frame_rate("0/0"), None);
        assert_eq!(parse_frame_rate("25"), None);
    }

    #[test]
    fn probe_json_completo() {
        let json = serde_json::json!({
            "format": { "duration": "120.5" },
            "streams": [
                { "codec_type": "video", "avg_frame_rate": "25/1", "width": 1920, "height": 1080 },
                { "codec_type": "audio", "bit_rate": "128000" }
            ]
        });
        let info = parse_probe_json(&json).unwrap();
        assert_eq!(info.duration_seconds, 120.5);
        assert_eq!(info.frame_rate, Some((25, 1)));
        assert_eq!(info.estimated_frames, 3013);
        assert!(info.has_audio);
        assert_eq!(info.source_audio_bitrate_bps, Some(128_000));
        assert_eq!(info.height, Some(1080));
    }

    #[test]
    fn probe_json_senza_audio() {
        let json = serde_json::json!({
            "format": { "duration": "10" },
            "streams": [{ "codec_type": "video", "avg_frame_rate": "30/1" }]
        });
        let info = parse_probe_json(&json).unwrap();
        assert!(!info.has_audio);
        assert_eq!(info.estimated_frames, 300);
    }

    #[test]
    fn probe_json_durata_assente_e_errore() {
        let json = serde_json::json!({
            "format": {},
            "streams": [{ "codec_type": "video", "avg_frame_rate": "30/1" }]
        });
        assert!(parse_probe_json(&json).is_err());
    }

    #[test]
    fn probe_json_senza_video_e_errore() {
        let json = serde_json::json!({
            "format": { "duration": "10" },
            "streams": [{ "codec_type": "audio" }]
        });
        assert!(parse_probe_json(&json).is_err());
    }
}
