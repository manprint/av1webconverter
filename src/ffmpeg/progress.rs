//! Parser del flusso `-progress` di ffmpeg.
//!
//! ffmpeg emette blocchi di righe `chiave=valore` chiusi da `progress=continue`
//! oppure `progress=end`. Questa è la stessa logica della funzione awk
//! `show_progress` negli script bash di riferimento: la percentuale è basata sul
//! tempo elaborato, raffinata con il conteggio frame quando disponibile, e l'ETA
//! deriva dalla velocità dichiarata da ffmpeg.

use crate::models::Progress;

#[derive(Debug, Clone)]
pub struct ProgressParser {
    total_seconds: f64,
    total_frames: u64,
    pass: u8,
    total_passes: u8,
    frame: u64,
    written_bytes: u64,
    bitrate: String,
    speed: String,
    elapsed_seconds: f64,
}

impl ProgressParser {
    pub fn new(total_seconds: f64, total_frames: u64, pass: u8, total_passes: u8) -> Self {
        Self {
            total_seconds,
            total_frames,
            pass,
            total_passes,
            frame: 0,
            written_bytes: 0,
            bitrate: String::new(),
            speed: String::new(),
            elapsed_seconds: 0.0,
        }
    }

    /// Consuma una riga. Restituisce `Some` solo quando il blocco è completo,
    /// ovvero su `progress=`, così da emettere un aggiornamento coerente.
    pub fn push_line(&mut self, line: &str) -> Option<Progress> {
        let (key, value) = line.split_once('=')?;
        let key = key.trim();
        let value = value.trim();

        match key {
            "frame" => {
                if let Ok(parsed) = value.parse::<u64>() {
                    self.frame = parsed;
                }
                None
            }
            "total_size" => {
                if let Ok(parsed) = value.parse::<u64>() {
                    self.written_bytes = parsed;
                }
                None
            }
            "bitrate" => {
                if value != "N/A" {
                    self.bitrate = value.to_string();
                }
                None
            }
            "out_time_us" | "out_time_ms" => {
                // out_time_ms di ffmpeg è in realtà in microsecondi: entrambe le
                // chiavi vanno divise per 1e6. Il valore non torna indietro.
                if let Ok(parsed) = value.parse::<i64>() {
                    if parsed > 0 {
                        let seconds = parsed as f64 / 1_000_000.0;
                        if seconds > self.elapsed_seconds {
                            self.elapsed_seconds = seconds;
                        }
                    }
                }
                None
            }
            "speed" => {
                let trimmed = value.trim_end_matches('x');
                if trimmed.parse::<f64>().is_ok() {
                    self.speed = value.to_string();
                }
                None
            }
            "progress" => Some(self.snapshot(value == "end")),
            _ => None,
        }
    }

    fn snapshot(&self, ended: bool) -> Progress {
        let mut displayed_elapsed = self.elapsed_seconds;

        // Con il conteggio frame noto, il rapporto frame/frame_totali è spesso
        // più avanti del timestamp riportato: si prende il maggiore dei due.
        if self.total_frames > 0 && self.frame > 0 && self.total_seconds > 0.0 {
            let frame_elapsed = self.frame as f64 * self.total_seconds / self.total_frames as f64;
            if frame_elapsed > displayed_elapsed {
                displayed_elapsed = frame_elapsed;
            }
        }

        let mut percent = if self.total_seconds > 0.0 {
            displayed_elapsed * 100.0 / self.total_seconds
        } else {
            0.0
        };
        if ended {
            percent = 100.0;
        } else if percent > 99.9 {
            // Mai 100% prima della fine effettiva del processo.
            percent = 99.9;
        }
        percent = percent.clamp(0.0, 100.0);

        let speed_number = self.speed.trim_end_matches('x').parse::<f64>().ok();
        let eta_seconds = match speed_number {
            Some(speed) if speed > 0.0 && !ended => {
                Some(((self.total_seconds - displayed_elapsed) / speed).max(0.0))
            }
            _ => None,
        };

        Progress {
            percent,
            elapsed_seconds: displayed_elapsed.min(self.total_seconds.max(displayed_elapsed)),
            total_seconds: self.total_seconds,
            frame: self.frame,
            total_frames: self.total_frames,
            written_bytes: self.written_bytes,
            bitrate: if self.bitrate.is_empty() {
                "N/D".to_string()
            } else {
                self.bitrate.clone()
            },
            speed: if self.speed.is_empty() {
                "N/D".to_string()
            } else {
                self.speed.clone()
            },
            eta_seconds,
            pass: self.pass,
            total_passes: self.total_passes,
        }
    }
}

/// Riga di log leggibile con lo stesso formato degli script bash:
/// `[Codifica] 42.3% | 00:00:51 / 00:02:00 | frame 1275 | file 12.40 MB | 1234kbits/s | 2.1x | ETA 00:00:33`
pub fn format_progress_line(label: &str, progress: &Progress) -> String {
    let pass_suffix = if progress.total_passes > 1 {
        format!(" (passaggio {}/{})", progress.pass, progress.total_passes)
    } else {
        String::new()
    };
    format!(
        "[{label}{pass_suffix}] {:5.1}% | {} / {} | frame {} | file {} | {} | {} | ETA {}",
        progress.percent,
        format_clock(progress.elapsed_seconds),
        format_clock(progress.total_seconds),
        progress.frame,
        format_size(progress.written_bytes),
        progress.bitrate,
        progress.speed,
        progress
            .eta_seconds
            .map(format_clock)
            .unwrap_or_else(|| "--:--:--".to_string()),
    )
}

pub fn format_clock(seconds: f64) -> String {
    let seconds = if seconds.is_finite() && seconds > 0.0 {
        seconds
    } else {
        0.0
    };
    let total = seconds as u64;
    format!(
        "{:02}:{:02}:{:02}",
        total / 3600,
        (total / 60) % 60,
        total % 60
    )
}

pub fn format_size(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.2} GB", bytes as f64 / 1_000_000_000.0)
    } else if bytes >= 1_000_000 {
        format!("{:.2} MB", bytes as f64 / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{:.1} kB", bytes as f64 / 1_000.0)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(parser: &mut ProgressParser, block: &str) -> Option<Progress> {
        let mut last = None;
        for line in block.lines() {
            if let Some(progress) = parser.push_line(line.trim()) {
                last = Some(progress);
            }
        }
        last
    }

    #[test]
    fn blocco_emesso_solo_su_progress() {
        let mut parser = ProgressParser::new(100.0, 2500, 1, 1);
        assert!(parser.push_line("frame=100").is_none());
        assert!(parser.push_line("speed=2.0x").is_none());
        assert!(parser.push_line("progress=continue").is_some());
    }

    #[test]
    fn percentuale_da_tempo_elaborato() {
        let mut parser = ProgressParser::new(100.0, 0, 1, 1);
        let progress = feed(
            &mut parser,
            "frame=0\nout_time_us=25000000\nspeed=2.0x\ntotal_size=1048576\nbitrate=512kbits/s\nprogress=continue",
        )
        .unwrap();
        assert!((progress.percent - 25.0).abs() < 0.001);
        assert_eq!(progress.written_bytes, 1_048_576);
        assert_eq!(progress.bitrate, "512kbits/s");
        // 75 secondi rimanenti a 2x.
        assert!((progress.eta_seconds.unwrap() - 37.5).abs() < 0.001);
    }

    #[test]
    fn conteggio_frame_raffina_la_percentuale() {
        let mut parser = ProgressParser::new(100.0, 1000, 1, 1);
        let progress = feed(
            &mut parser,
            "frame=500\nout_time_us=10000000\nspeed=1.0x\nprogress=continue",
        )
        .unwrap();
        // Il tempo direbbe 10%, i frame dicono 50%: vince il maggiore.
        assert!((progress.percent - 50.0).abs() < 0.001);
    }

    #[test]
    fn il_tempo_elaborato_non_regredisce() {
        let mut parser = ProgressParser::new(100.0, 0, 1, 1);
        feed(&mut parser, "out_time_us=50000000\nprogress=continue");
        let progress = feed(&mut parser, "out_time_us=10000000\nprogress=continue").unwrap();
        assert!((progress.percent - 50.0).abs() < 0.001);
    }

    #[test]
    fn cento_percento_solo_alla_fine() {
        let mut parser = ProgressParser::new(100.0, 0, 1, 1);
        let quasi = feed(&mut parser, "out_time_us=99999000\nprogress=continue").unwrap();
        assert!((quasi.percent - 99.9).abs() < 0.001);
        let fine = feed(&mut parser, "progress=end").unwrap();
        assert_eq!(fine.percent, 100.0);
        assert!(fine.eta_seconds.is_none());
    }

    #[test]
    fn valori_assenti_diventano_nd() {
        let mut parser = ProgressParser::new(10.0, 0, 1, 1);
        let progress = feed(&mut parser, "bitrate=N/A\nprogress=continue").unwrap();
        assert_eq!(progress.bitrate, "N/D");
        assert_eq!(progress.speed, "N/D");
        assert!(progress.eta_seconds.is_none());
    }

    #[test]
    fn formattazione_orologio_e_dimensioni() {
        assert_eq!(format_clock(0.0), "00:00:00");
        assert_eq!(format_clock(3661.0), "01:01:01");
        assert_eq!(format_clock(-5.0), "00:00:00");
        assert_eq!(format_size(999), "999 B");
        assert_eq!(format_size(1_500), "1.5 kB");
        assert_eq!(format_size(12_400_000), "12.40 MB");
        assert_eq!(format_size(2_000_000_000), "2.00 GB");
    }

    #[test]
    fn riga_di_progresso_include_il_passaggio() {
        let progress = Progress {
            percent: 42.3,
            elapsed_seconds: 51.0,
            total_seconds: 120.0,
            frame: 1275,
            total_frames: 3000,
            written_bytes: 12_400_000,
            bitrate: "1234kbits/s".into(),
            speed: "2.1x".into(),
            eta_seconds: Some(33.0),
            pass: 2,
            total_passes: 2,
        };
        let line = format_progress_line("Codifica", &progress);
        assert!(line.contains("(passaggio 2/2)"));
        assert!(line.contains("42.3%"));
        assert!(line.contains("00:00:51 / 00:02:00"));
        assert!(line.contains("frame 1275"));
        assert!(line.contains("12.40 MB"));
        assert!(line.contains("ETA 00:00:33"));
    }
}
