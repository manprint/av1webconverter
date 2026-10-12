//! Costruzione delle righe di comando ffmpeg ed esecuzione con lettura del
//! progresso in tempo reale.
//!
//! La catena riproduce `start_av1_docker.sh`: codifica in Matroska temporaneo,
//! poi remux in MP4 con `-tag:v av01 -movflags +faststart`. La parte SSH degli
//! script non serve: qui ffmpeg gira nello stesso container.

use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::Stdio;

use anyhow::{bail, Context, Result};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::watch;

use crate::config::{parse_bitrate, EncodingMode, EncodingParams};
use crate::ffmpeg::probe::MediaInfo;
use crate::ffmpeg::progress::ProgressParser;
use crate::models::Progress;

/// Esito di un processo ffmpeg completato senza errori.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunOutcome {
    Success,
    /// Interrotto su richiesta: il processo è stato terminato.
    Cancelled,
}

/// Filtro di scala: limita l'altezza, mantiene l'aspetto visivo del sorgente e
/// normalizza a pixel quadrati per evitare DAR errati con sorgenti anamorfiche.
///
/// `reset_sar=1` fa sì che `scale` compensi il SAR del sorgente quando calcola
/// la larghezza automatica (`-2`) e produca pixel quadrati. È disponibile nella
/// build FFmpeg 9.0.2 usata dall'immagine applicativa.
///
/// La virgola dentro `min()` va protetta, perché la virgola separa i filtri.
///
/// `scale_height == 0` significa dimensione originale: non serve alcun
/// filtro, quindi `-vf` non viene passato e il SAR del sorgente resta
/// intatto.
pub fn scale_filter(scale_height: u32) -> Option<String> {
    if scale_height == 0 {
        return None;
    }
    let height = format!("trunc(min({scale_height}\\,ih)/2)*2");
    Some(format!("scale=-2:{height}:reset_sar=1"))
}

/// Interpolazione dei frame richiesta da un job: porta la cadenza del sorgente
/// alla cadenza scelta con `minterpolate` a compensazione di movimento.
///
/// Esiste solo quando serve davvero: [`Interpolazione::per_job`] restituisce
/// `None` se la richiesta è `0`, se la cadenza del sorgente è ignota o se il
/// sorgente è già alla cadenza richiesta, perché l'interpolazione non rimuove frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Interpolazione {
    /// Cadenza di uscita in fps interi, 50 o 60.
    pub fps_uscita: u32,
    /// Cadenza del sorgente come frazione, es. `24000/1001` per 23.976 fps.
    pub fps_sorgente: (u64, u64),
}

impl Interpolazione {
    /// Sotto questa distanza dal target la cadenza conta come già raggiunta:
    /// 59.94 fps non va portato a 60 con un'interpolazione che non aggiunge nulla.
    const TOLLERANZA_FPS: f64 = 0.5;

    pub fn per_job(fps_target: u32, fps_sorgente: Option<(u64, u64)>) -> Option<Self> {
        let (numeratore, denominatore) = fps_sorgente?;
        if fps_target == 0 || numeratore == 0 || denominatore == 0 {
            return None;
        }
        let sorgente = numeratore as f64 / denominatore as f64;
        if sorgente >= f64::from(fps_target) - Self::TOLLERANZA_FPS {
            return None;
        }
        Some(Self {
            fps_uscita: fps_target,
            fps_sorgente: (numeratore, denominatore),
        })
    }

    /// Cadenza del sorgente in fps decimali.
    pub fn fps_sorgente_decimali(&self) -> f64 {
        self.fps_sorgente.0 as f64 / self.fps_sorgente.1 as f64
    }

    /// Filtro da accodare alla scala. `mi_mode=mci` è la modalità a compensazione
    /// di movimento; gli altri parametri restano ai default di ffmpeg.
    pub fn filtro(&self) -> String {
        format!("minterpolate=fps={}:mi_mode=mci", self.fps_uscita)
    }

    /// Distanza fra keyframe riscalata: `keyint` conta frame, quindi a cadenza più
    /// alta va moltiplicata per il rapporto, così il GOP dura gli stessi secondi.
    pub fn keyint(&self, keyint: u32) -> u32 {
        (f64::from(keyint) * f64::from(self.fps_uscita) / self.fps_sorgente_decimali()).round()
            as u32
    }

    /// Frame prodotti in uscita per una durata: è il totale usato dal progresso,
    /// perché `frame=` di ffmpeg conta i frame interpolati.
    pub fn frame_uscita(&self, durata_secondi: f64) -> u64 {
        (durata_secondi * f64::from(self.fps_uscita)).round() as u64
    }
}

fn common_input_args(params: &EncodingParams, input: &Path) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec![
        "-nostdin".into(),
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-nostats".into(),
        "-stats_period".into(),
        format!("{}", params.progress_interval).into(),
        "-progress".into(),
        "pipe:1".into(),
        "-y".into(),
    ];
    if let Some(cpus) = params.cpus {
        args.push("-threads".into());
        args.push(cpus.to_string().into());
    }
    args.push("-i".into());
    args.push(input.as_os_str().to_owned());
    args
}

pub(crate) fn video_encoder_args(
    params: &EncodingParams,
    interpolazione: Option<&Interpolazione>,
) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec!["-c:v".into(), "libsvtav1".into()];
    // Scala e interpolazione condividono un solo -vf, nell'ordine scala →
    // interpolazione: minterpolate lavora così su frame già ridotti.
    // Senza filtri -vf va omesso, non svuotato.
    let mut filtri: Vec<String> = Vec::new();
    filtri.extend(scale_filter(params.scale_height));
    filtri.extend(interpolazione.map(Interpolazione::filtro));
    if !filtri.is_empty() {
        args.push("-vf".into());
        args.push(filtri.join(",").into());
    }
    let keyint = interpolazione.map_or(params.keyint, |i| i.keyint(params.keyint));
    args.extend([
        "-preset".into(),
        params.preset.to_string().into(),
        "-svtav1-params".into(),
        params.effective_svtav1_params().into(),
        "-g".into(),
        keyint.to_string().into(),
        "-pix_fmt".into(),
        params.pixel_format.clone().into(),
    ]);
    args
}

/// Codifica a qualità costante, un solo passaggio.
pub fn build_crf_args(
    params: &EncodingParams,
    input: &Path,
    output_mkv: &Path,
    interpolazione: Option<&Interpolazione>,
) -> Vec<OsString> {
    let mut args = common_input_args(params, input);
    args.extend(["-map".into(), "0:v:0".into(), "-map".into(), "0:a?".into()]);
    args.extend(video_encoder_args(params, interpolazione));
    args.extend([
        "-b:v".into(),
        "0".into(),
        "-crf".into(),
        params.crf.to_string().into(),
        "-c:a".into(),
        "libopus".into(),
        "-b:a".into(),
        params.audio_bitrate.clone().into(),
        "-f".into(),
        "matroska".into(),
        output_mkv.as_os_str().to_owned(),
    ]);
    args
}

/// Un passaggio della codifica VBR a dimensione target. Il primo passaggio
/// scarta l'output e non tocca l'audio.
pub fn build_two_pass_args(
    params: &EncodingParams,
    input: &Path,
    output_mkv: &Path,
    passlogfile: &Path,
    video_bitrate_bps: u64,
    pass: u8,
    interpolazione: Option<&Interpolazione>,
) -> Vec<OsString> {
    let mut args = common_input_args(params, input);
    args.extend(["-map".into(), "0:v:0".into()]);
    if pass == 2 {
        args.extend(["-map".into(), "0:a?".into()]);
    }
    args.extend(video_encoder_args(params, interpolazione));
    args.extend([
        "-b:v".into(),
        format!("{video_bitrate_bps}").into(),
        "-pass".into(),
        pass.to_string().into(),
        "-passlogfile".into(),
        passlogfile.as_os_str().to_owned(),
    ]);
    if pass == 1 {
        args.extend([
            "-an".into(),
            "-f".into(),
            "null".into(),
            if cfg!(windows) {
                "NUL".into()
            } else {
                "/dev/null".into()
            },
        ]);
    } else {
        args.extend([
            "-c:a".into(),
            "libopus".into(),
            "-b:a".into(),
            params.audio_bitrate.clone().into(),
            "-f".into(),
            "matroska".into(),
            output_mkv.as_os_str().to_owned(),
        ]);
    }
    args
}

/// Remux finale senza ricodifica: MP4 con tag AV1 e indice in testa.
pub fn build_remux_args(input_mkv: &Path, output_mp4: &Path) -> Vec<OsString> {
    vec![
        "-nostdin".into(),
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-y".into(),
        "-i".into(),
        input_mkv.as_os_str().to_owned(),
        "-map".into(),
        "0".into(),
        "-c".into(),
        "copy".into(),
        "-tag:v".into(),
        "av01".into(),
        "-movflags".into(),
        "+faststart".into(),
        output_mp4.as_os_str().to_owned(),
    ]
}

/// Bitrate video per centrare la dimensione richiesta: dal target si sottrae il
/// margine per l'overhead del contenitore e lo spazio delle tracce audio.
pub fn compute_video_bitrate(params: &EncodingParams, info: &MediaInfo) -> Result<u64> {
    let target_mb = params
        .target_size_mb
        .context("target_size_mb assente in modalità target_size")?;
    if info.duration_seconds <= 0.0 {
        bail!("durata non valida per il calcolo del bitrate");
    }

    let target_bits = target_mb * 1_000_000.0 * 8.0;
    let usable_bits = target_bits * (1.0 - params.size_margin_percent / 100.0);
    let audio_bits = if info.has_audio {
        parse_bitrate(&params.audio_bitrate).unwrap_or(0) as f64 * info.duration_seconds
    } else {
        0.0
    };
    let video_bits = usable_bits - audio_bits;
    if video_bits <= 0.0 {
        bail!(
            "dimensione target troppo piccola: {target_mb} MB non bastano nemmeno per l'audio a {}",
            params.audio_bitrate
        );
    }

    let bitrate = (video_bits / info.duration_seconds).floor() as u64;
    if bitrate < 1_000 {
        bail!("bitrate video calcolato troppo basso ({bitrate} bit/s): aumentare la dimensione target");
    }
    Ok(bitrate)
}

/// Numero di passaggi ffmpeg richiesti dalla modalità scelta.
pub fn total_passes(mode: EncodingMode) -> u8 {
    match mode {
        EncodingMode::Crf | EncodingMode::VmafAnalysis => 1,
        EncodingMode::TargetSize => 2,
    }
}

/// Passaggi mostrati all'utente: la misura VMAF è un passaggio in più.
pub fn total_passes_with_measure(mode: EncodingMode, measure_vmaf: bool) -> u8 {
    total_passes(mode) + u8::from(measure_vmaf)
}

/// Eventi prodotti da un processo ffmpeg in esecuzione.
pub enum FfmpegEvent {
    Progress(Progress),
    /// Riga proveniente da stderr, quindi diagnostica o errore.
    Log(String),
}

/// Esegue ffmpeg leggendo `-progress` da stdout e la diagnostica da stderr.
///
/// `cancel` viene osservato durante l'attesa: alla richiesta di stop il processo
/// figlio viene terminato e la funzione restituisce `RunOutcome::Cancelled`.
pub async fn run_ffmpeg<F>(
    ffmpeg_bin: &Path,
    args: &[OsString],
    mut parser: Option<ProgressParser>,
    cancel: &mut watch::Receiver<bool>,
    mut on_event: F,
) -> Result<RunOutcome>
where
    F: FnMut(FfmpegEvent),
{
    if *cancel.borrow_and_update() {
        return Ok(RunOutcome::Cancelled);
    }

    let mut child = Command::new(ffmpeg_bin)
        .args(args.iter().map(OsStr::new))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("avvio di {}", ffmpeg_bin.display()))?;

    let mut stdout = BufReader::new(child.stdout.take().expect("stdout richiesto")).lines();
    let mut stderr = BufReader::new(child.stderr.take().expect("stderr richiesto")).lines();

    // Le ultime righe di stderr sono la parte utile del messaggio d'errore.
    let mut stderr_tail: Vec<String> = Vec::new();
    let mut stdout_open = true;
    let mut stderr_open = true;
    let mut cancelled = false;

    while stdout_open || stderr_open {
        tokio::select! {
            line = stdout.next_line(), if stdout_open => match line {
                Ok(Some(line)) => {
                    if let Some(parser) = parser.as_mut() {
                        if let Some(progress) = parser.push_line(&line) {
                            on_event(FfmpegEvent::Progress(progress));
                        }
                    }
                }
                Ok(None) => stdout_open = false,
                Err(error) => {
                    stderr_tail.push(format!("lettura stdout ffmpeg interrotta: {error}"));
                    stdout_open = false;
                }
            },
            line = stderr.next_line(), if stderr_open => match line {
                Ok(Some(line)) => {
                    let line = line.trim().to_string();
                    if !line.is_empty() {
                        if stderr_tail.len() >= 40 {
                            stderr_tail.remove(0);
                        }
                        stderr_tail.push(line.clone());
                        on_event(FfmpegEvent::Log(line));
                    }
                }
                Ok(None) => stderr_open = false,
                Err(error) => {
                    stderr_tail.push(format!("lettura stderr ffmpeg interrotta: {error}"));
                    stderr_open = false;
                }
            },
            changed = cancel.changed(), if !cancelled => {
                if changed.is_err() || *cancel.borrow() {
                    cancelled = true;
                    let _ = child.start_kill();
                }
            }
        }
    }

    let status = child.wait().await.context("attesa del processo ffmpeg")?;

    if cancelled || *cancel.borrow() {
        return Ok(RunOutcome::Cancelled);
    }
    if !status.success() {
        bail!(
            "ffmpeg è terminato con stato {}: {}",
            status
                .code()
                .map(|code| code.to_string())
                .unwrap_or_else(|| "interrotto da segnale".to_string()),
            if stderr_tail.is_empty() {
                "nessun messaggio su stderr".to_string()
            } else {
                stderr_tail.join(" | ")
            }
        );
    }
    Ok(RunOutcome::Success)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn as_strings(args: &[OsString]) -> Vec<String> {
        args.iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    fn info(duration: f64, has_audio: bool) -> MediaInfo {
        MediaInfo {
            duration_seconds: duration,
            frame_rate: Some((25, 1)),
            estimated_frames: (duration * 25.0) as u64,
            has_audio,
            source_audio_bitrate_bps: None,
            width: Some(1920),
            height: Some(1080),
        }
    }

    #[test]
    fn filtro_di_scala_calcola_la_larghezza_dal_dar() {
        assert_eq!(
            scale_filter(480).unwrap(),
            "scale=-2:trunc(min(480\\,ih)/2)*2:reset_sar=1"
        );
    }

    #[test]
    fn filtro_di_scala_resetta_il_sar() {
        let filter = scale_filter(720).unwrap();
        assert!(filter.ends_with(":reset_sar=1"));
        assert!(filter.starts_with("scale=-2:"));
        assert!(filter.contains("min(720\\,ih)"));
    }

    #[test]
    fn filtro_di_scala_assente_con_altezza_originale() {
        assert_eq!(scale_filter(0), None);
    }

    #[test]
    fn comando_crf_omette_vf_con_altezza_originale() {
        let params = EncodingParams {
            scale_height: 0,
            ..EncodingParams::default()
        };
        let args = as_strings(&build_crf_args(
            &params,
            Path::new("/in/a.mp4"),
            Path::new("/tmp/a.mkv"),
            None,
        ));
        assert!(!args.iter().any(|arg| arg == "-vf"));
        assert!(!args.iter().any(|arg| arg.contains("scale=-2")));
        assert!(args.join(" ").contains("-c:v libsvtav1 -preset 1"));
    }

    #[test]
    fn comando_due_passaggi_omette_vf_con_altezza_originale() {
        let params = EncodingParams {
            mode: EncodingMode::TargetSize,
            scale_height: 0,
            target_size_mb: Some(300.0),
            ..EncodingParams::default()
        };
        let args = as_strings(&build_two_pass_args(
            &params,
            Path::new("/in/a.mp4"),
            Path::new("/tmp/a.mkv"),
            Path::new("/tmp/a.log"),
            1_000_000,
            1,
            None,
        ));
        assert!(!args.iter().any(|arg| arg == "-vf"));
        assert!(!args.iter().any(|arg| arg.contains("scale=-2")));
    }

    #[test]
    fn comando_crf_riproduce_lo_script_di_riferimento() {
        let params = EncodingParams::default();
        let args = build_crf_args(
            &params,
            &PathBuf::from("/in/film.mp4"),
            &PathBuf::from("/tmp/out.mkv"),
            None,
        );
        let args = as_strings(&args);
        let joined = args.join(" ");
        assert!(joined.contains("-c:v libsvtav1"));
        assert!(joined.contains("-vf scale=-2:trunc(min(480\\,ih)/2)*2:reset_sar=1"));
        assert!(joined.contains("-b:v 0 -crf 47"));
        assert!(joined.contains("-preset 1"));
        assert!(joined.contains("-svtav1-params tune=5"));
        assert!(joined.contains("-g 240"));
        assert!(joined.contains("-pix_fmt yuv420p10le"));
        assert!(joined.contains("-c:a libopus -b:a 48k"));
        assert!(joined.contains("-map 0:v:0 -map 0:a?"));
        assert!(joined.contains("-progress pipe:1"));
        assert!(joined.ends_with("-f matroska /tmp/out.mkv"));
    }

    #[test]
    fn comando_crf_include_i_thread_solo_se_richiesti() {
        let mut params = EncodingParams::default();
        assert!(!as_strings(&build_crf_args(
            &params,
            Path::new("/in/a.mp4"),
            Path::new("/tmp/a.mkv"),
            None,
        ))
        .contains(&"-threads".to_string()));
        params.cpus = Some(4);
        let args = as_strings(&build_crf_args(
            &params,
            Path::new("/in/a.mp4"),
            Path::new("/tmp/a.mkv"),
            None,
        ));
        let index = args.iter().position(|arg| arg == "-threads").unwrap();
        assert_eq!(args[index + 1], "4");
    }

    #[test]
    fn primo_passaggio_scarta_output_e_audio() {
        let params = EncodingParams {
            mode: EncodingMode::TargetSize,
            target_size_mb: Some(300.0),
            ..Default::default()
        };
        let args = as_strings(&build_two_pass_args(
            &params,
            Path::new("/in/a.mp4"),
            Path::new("/tmp/a.mkv"),
            Path::new("/tmp/a.log"),
            1_000_000,
            1,
            None,
        ));
        let joined = args.join(" ");
        assert!(joined.contains("-pass 1"));
        assert!(joined.contains("-an"));
        assert!(joined.contains("-f null"));
        assert!(!joined.contains("libopus"));
        assert!(!joined.contains("0:a?"));
        // tune=5 non è compatibile con il VBR a due passaggi.
        assert!(joined.contains("-svtav1-params tune=0"));
    }

    #[test]
    fn secondo_passaggio_scrive_il_matroska_con_audio() {
        let params = EncodingParams {
            mode: EncodingMode::TargetSize,
            target_size_mb: Some(300.0),
            ..Default::default()
        };
        let args = as_strings(&build_two_pass_args(
            &params,
            Path::new("/in/a.mp4"),
            Path::new("/tmp/a.mkv"),
            Path::new("/tmp/a.log"),
            1_000_000,
            2,
            None,
        ));
        let joined = args.join(" ");
        assert!(joined.contains("-pass 2"));
        assert!(joined.contains("-c:a libopus"));
        assert!(joined.ends_with("-f matroska /tmp/a.mkv"));
    }

    #[test]
    fn remux_finale_tagga_av1_e_sposta_l_indice() {
        let joined = as_strings(&build_remux_args(
            Path::new("/tmp/a.mkv"),
            Path::new("/tmp/a.mp4"),
        ))
        .join(" ");
        assert!(joined.contains("-c copy"));
        assert!(joined.contains("-tag:v av01"));
        assert!(joined.contains("-movflags +faststart"));
    }

    #[test]
    fn bitrate_target_sottrae_audio_e_margine() {
        let params = EncodingParams {
            mode: EncodingMode::TargetSize,
            target_size_mb: Some(100.0),
            size_margin_percent: 1.0,
            audio_bitrate: "48k".into(),
            ..Default::default()
        };
        // 100 MB -> 800.000.000 bit, meno 1% -> 792.000.000, meno audio
        // (48.000 * 100 = 4.800.000) -> 787.200.000 su 100 s.
        let bitrate = compute_video_bitrate(&params, &info(100.0, true)).unwrap();
        assert_eq!(bitrate, 7_872_000);
    }

    #[test]
    fn bitrate_target_ignora_audio_assente() {
        let params = EncodingParams {
            mode: EncodingMode::TargetSize,
            target_size_mb: Some(100.0),
            size_margin_percent: 0.0,
            ..Default::default()
        };
        let bitrate = compute_video_bitrate(&params, &info(100.0, false)).unwrap();
        assert_eq!(bitrate, 8_000_000);
    }

    #[test]
    fn bitrate_target_rifiuta_dimensioni_impossibili() {
        let params = EncodingParams {
            mode: EncodingMode::TargetSize,
            target_size_mb: Some(0.001),
            ..Default::default()
        };
        assert!(compute_video_bitrate(&params, &info(3600.0, true)).is_err());
    }

    #[test]
    fn passaggi_per_modalita() {
        assert_eq!(total_passes(EncodingMode::Crf), 1);
        assert_eq!(total_passes(EncodingMode::TargetSize), 2);
        assert_eq!(total_passes(EncodingMode::VmafAnalysis), 1);
    }

    #[test]
    fn i_passaggi_includono_la_misura_vmaf() {
        assert_eq!(total_passes_with_measure(EncodingMode::Crf, true), 2);
        assert_eq!(total_passes_with_measure(EncodingMode::TargetSize, true), 3);
        assert_eq!(total_passes_with_measure(EncodingMode::Crf, false), 1);
        assert_eq!(
            total_passes_with_measure(EncodingMode::TargetSize, false),
            2
        );
    }

    /// Valore che segue `-vf` negli argomenti, se presente.
    fn valore_vf(args: &[String]) -> Option<&str> {
        let index = args.iter().position(|arg| arg == "-vf")?;
        args.get(index + 1).map(String::as_str)
    }

    #[test]
    fn interpolazione_aggiunge_minterpolate_dopo_la_scala() {
        let params = EncodingParams {
            scale_height: 480,
            ..EncodingParams::default()
        };
        let interp = Interpolazione::per_job(60, Some((24, 1))).expect("interpolazione attiva");
        let args = as_strings(&build_crf_args(
            &params,
            Path::new("/in/a.mp4"),
            Path::new("/tmp/a.mkv"),
            Some(&interp),
        ));
        assert_eq!(
            valore_vf(&args),
            Some("scale=-2:trunc(min(480\\,ih)/2)*2:reset_sar=1,minterpolate=fps=60:mi_mode=mci")
        );
    }

    #[test]
    fn interpolazione_senza_scala_usa_solo_minterpolate() {
        let params = EncodingParams {
            scale_height: 0,
            ..EncodingParams::default()
        };
        let interp = Interpolazione::per_job(50, Some((25, 1))).expect("interpolazione attiva");
        let args = as_strings(&build_crf_args(
            &params,
            Path::new("/in/a.mp4"),
            Path::new("/tmp/a.mkv"),
            Some(&interp),
        ));
        assert_eq!(valore_vf(&args), Some("minterpolate=fps=50:mi_mode=mci"));
    }

    #[test]
    fn interpolazione_riscala_il_gop_per_mantenere_i_secondi() {
        let params = EncodingParams::default();
        let interp = Interpolazione::per_job(60, Some((24, 1))).expect("interpolazione attiva");
        let args = as_strings(&build_crf_args(
            &params,
            Path::new("/in/a.mp4"),
            Path::new("/tmp/a.mkv"),
            Some(&interp),
        ));
        let index = args.iter().position(|arg| arg == "-g").unwrap();
        // 240 frame a 24 fps sono 10 s: a 60 fps servono 600 frame.
        assert_eq!(args[index + 1], "600");
    }

    #[test]
    fn interpolazione_non_si_attiva_alla_cadenza_richiesta_o_oltre() {
        // 59.94 fps è già il 60 dei video NTSC: nessuna interpolazione.
        assert!(Interpolazione::per_job(60, Some((60000, 1001))).is_none());
        assert!(Interpolazione::per_job(50, Some((50, 1))).is_none());
        // Un sorgente più veloce non viene rallentato: l'interpolazione non rimuove frame.
        assert!(Interpolazione::per_job(50, Some((60, 1))).is_none());
        assert!(Interpolazione::per_job(0, Some((24, 1))).is_none());
        assert!(Interpolazione::per_job(60, None).is_none());
        assert!(Interpolazione::per_job(60, Some((24000, 1001))).is_some());
    }

    #[test]
    fn frame_di_uscita_seguono_la_cadenza_richiesta() {
        let interp = Interpolazione::per_job(60, Some((24, 1))).expect("interpolazione attiva");
        assert_eq!(interp.frame_uscita(10.0), 600);
        assert_eq!(interp.keyint(240), 600);
    }

    #[test]
    fn il_secondo_passaggio_applica_l_interpolazione() {
        let params = EncodingParams {
            mode: EncodingMode::TargetSize,
            target_size_mb: Some(300.0),
            ..Default::default()
        };
        let interp = Interpolazione::per_job(50, Some((25, 1))).expect("interpolazione attiva");
        let args = as_strings(&build_two_pass_args(
            &params,
            Path::new("/in/a.mp4"),
            Path::new("/tmp/a.mkv"),
            Path::new("/tmp/a.log"),
            1_000_000,
            2,
            Some(&interp),
        ));
        assert!(valore_vf(&args)
            .unwrap()
            .contains("minterpolate=fps=50:mi_mode=mci"));
    }
}
