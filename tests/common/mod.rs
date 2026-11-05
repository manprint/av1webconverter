//! Utilità condivise dai test di integrazione: server reale su porta effimera,
//! configurazione isolata in una cartella temporanea, generazione di sorgenti
//! video di prova con ffmpeg.
//!
//! Ogni target di test usa solo una parte di queste utilità: il resto
//! risulterebbe inutilizzato in quel target.
#![allow(dead_code)]

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::time::Duration;

use av1webconverter::config::{AuthConfig, Config, EncodingParams};
use av1webconverter::state::AppState;
use tempfile::TempDir;

pub struct TestServer {
    pub base_url: String,
    pub client: reqwest::Client,
    pub temp: TempDir,
    pub output_dir: PathBuf,
    pub input_dir: PathBuf,
}

impl TestServer {
    pub fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }
}

/// Avvia il server con la configurazione indicata su una porta libera.
pub async fn start_with(mut mutate: impl FnMut(&mut Config)) -> TestServer {
    let temp = tempfile::tempdir().expect("cartella temporanea");
    let mut config = Config::default();
    config.paths.input_dir = temp.path().join("in");
    config.paths.output_dir = temp.path().join("out");
    config.paths.data_dir = temp.path().join("data");
    config.paths.log_dir = temp.path().join("logs");
    config.logging.json_file = false;
    config.auth = AuthConfig {
        enabled: false,
        username: "admin".into(),
        password: "segreta".into(),
    };
    // I test devono restare rapidi: preset veloce e progresso frequente.
    // `tune=5` richiede SVT-AV1 4.x (quello dell'immagine custom): sull'ffmpeg
    // di sistema si usa `tune=0`, accettato da tutte le versioni.
    config.encoding = EncodingParams {
        preset: 12,
        crf: 50,
        scale_height: 120,
        progress_interval: 0.5,
        svtav1_params: "tune=0".into(),
        ..Default::default()
    };
    mutate(&mut config);
    config.prepare_directories().expect("preparazione cartelle");

    let input_dir = config.paths.input_dir.clone();
    let output_dir = config.paths.output_dir.clone();

    let state = AppState::new(config).await.expect("stato applicativo");
    let app = av1webconverter::api::router(state);

    let listener =
        tokio::net::TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
            .await
            .expect("bind su porta effimera");
    let address = listener.local_addr().expect("indirizzo locale");

    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    TestServer {
        base_url: format!("http://{address}"),
        client: reqwest::Client::builder()
            .timeout(Duration::from_secs(600))
            .build()
            .expect("client HTTP"),
        temp,
        output_dir,
        input_dir,
    }
}

pub async fn start() -> TestServer {
    start_with(|_| {}).await
}

/// Vero se ffmpeg e ffprobe offrono le funzionalità richieste dal runtime:
/// libsvtav1 e l'opzione `reset_sar` del filtro scale, presenti nell'immagine
/// custom basata su FFmpeg 9.0.2.
pub fn ffmpeg_with_svtav1() -> bool {
    let encoders = std::process::Command::new("ffmpeg")
        .args(["-hide_banner", "-encoders"])
        .output();
    let probe = std::process::Command::new("ffprobe")
        .arg("-version")
        .output();
    let scale_help = std::process::Command::new("ffmpeg")
        .args(["-hide_banner", "-h", "filter=scale"])
        .output();
    match (encoders, probe, scale_help) {
        (Ok(encoders), Ok(probe), Ok(scale_help)) => {
            probe.status.success()
                && String::from_utf8_lossy(&encoders.stdout).contains("libsvtav1")
                && String::from_utf8_lossy(&scale_help.stdout).contains("reset_sar")
        }
        _ => false,
    }
}

/// I test di misura richiedono un ffmpeg con il filtro `libvmaf`, assente in
/// molte build di sistema.
pub fn ffmpeg_with_libvmaf() -> bool {
    let filters = std::process::Command::new("ffmpeg")
        .args(["-hide_banner", "-filters"])
        .output();
    match filters {
        Ok(output) => {
            output.status.success() && String::from_utf8_lossy(&output.stdout).contains("libvmaf")
        }
        Err(_) => false,
    }
}

/// Genera un MP4 di prova con traccia video e audio.
pub fn make_test_video(path: &Path, seconds: u32) {
    make_test_video_at(path, seconds, 10, "640x360");
}

/// Come `make_test_video`, con frame rate e dimensione indicati. Il video
/// mpeg4 generato non dichiara tag colore.
pub fn make_test_video_at(path: &Path, seconds: u32, fps: u32, size: &str) {
    let status = std::process::Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .args([
            "-f",
            "lavfi",
            "-i",
            &format!("testsrc=duration={seconds}:size={size}:rate={fps}"),
        ])
        .args([
            "-f",
            "lavfi",
            "-i",
            &format!("sine=frequency=440:duration={seconds}"),
        ])
        .args(["-c:v", "mpeg4", "-q:v", "5", "-c:a", "aac", "-b:a", "64k"])
        .arg(path)
        .status()
        .expect("generazione video di prova");
    assert!(status.success(), "ffmpeg non ha generato il video di prova");
    assert!(path.exists());
}

/// Genera un MP4 anamorfico: 720x576 con DAR dichiarato 16:9, cioè pixel non
/// quadrati (SAR 64:45), come un PAL widescreen.
pub fn make_anamorphic_video(path: &Path, seconds: u32) {
    let status = std::process::Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .args([
            "-f",
            "lavfi",
            "-i",
            &format!("testsrc=duration={seconds}:size=720x576:rate=10"),
        ])
        .args(["-c:v", "mpeg4", "-q:v", "5", "-aspect", "16:9"])
        .arg(path)
        .status()
        .expect("generazione video anamorfico");
    assert!(
        status.success(),
        "ffmpeg non ha generato il video anamorfico"
    );
}

/// Genera un MP4 con audio AC3 5.1(side), il layout di DVD e broadcast che
/// libopus non accetta così com'è.
pub fn make_surround_video(path: &Path, seconds: u32) {
    let status = std::process::Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .args([
            "-f",
            "lavfi",
            "-i",
            &format!("testsrc=duration={seconds}:size=640x360:rate=10"),
        ])
        .args([
            "-f",
            "lavfi",
            "-i",
            &format!("sine=frequency=440:duration={seconds}"),
        ])
        .args(["-af", "pan=5.1(side)|FL=c0|FR=c0|FC=c0|LFE=c0|SL=c0|SR=c0"])
        .args(["-c:v", "mpeg4", "-q:v", "5", "-c:a", "ac3", "-b:a", "384k"])
        .arg(path)
        .status()
        .expect("generazione video con audio 5.1");
    assert!(
        status.success(),
        "ffmpeg non ha generato il video con audio 5.1"
    );
}

/// Tracce audio di un file come "codec,canali,layout", secondo ffprobe.
pub fn audio_streams(path: &Path) -> Vec<String> {
    let output = std::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "a",
            "-show_entries",
            "stream=codec_name,channels,channel_layout",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .expect("ffprobe sulle tracce audio");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.trim().trim_end_matches(',').to_string())
        .filter(|line| !line.is_empty())
        .collect()
}

/// Tag colore del primo stream video: "range,matrice,primari,trasferimento".
pub fn color_tags(path: &Path) -> String {
    let output = std::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=color_range,color_space,color_primaries,color_transfer",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .expect("ffprobe sui tag colore");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// VMAF medio calcolato direttamente con ffmpeg, con i filtri indicati in
/// testa a ciascun ramo.
pub fn vmaf_score(
    distorted: &Path,
    distorted_filters: &str,
    reference: &Path,
    reference_filters: &str,
    model: &str,
) -> f64 {
    let log = distorted.with_extension("verifica.json");
    // Abbina i frame per indice, senza dipendere dai timestamp: è il confronto
    // di riferimento per la sincronizzazione usata dall'applicazione.
    let graph = format!(
        "[0:v]settb=AVTB,setpts=N,{distorted_filters}format=yuv420p10le[dist];\
         [1:v]settb=AVTB,setpts=N,{reference_filters}format=yuv420p10le[ref];\
         [dist][ref]libvmaf=model=version={model}:log_fmt=json:log_path={}",
        log.display()
    );
    let status = std::process::Command::new("ffmpeg")
        .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(distorted)
        .arg("-i")
        .arg(reference)
        .args(["-lavfi", &graph, "-an", "-f", "null", "-"])
        .status()
        .expect("misura VMAF di verifica");
    assert!(status.success(), "misura VMAF di verifica fallita");
    let json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&log).expect("log VMAF di verifica")).unwrap();
    json["pooled_metrics"]["vmaf"]["mean"].as_f64().unwrap()
}

/// Istanti dei keyframe del primo stream video, dai flag dei pacchetti: non
/// serve decodificare.
pub fn keyframe_times(path: &Path) -> Vec<f64> {
    let output = std::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "packet=pts_time,flags",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .expect("ffprobe sui pacchetti");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let (time, flags) = line.split_once(',')?;
            flags.contains('K').then(|| time.parse().ok())?
        })
        .collect()
}

/// Geometria del primo stream video: larghezza, altezza, SAR, DAR.
pub fn video_geometry(path: &Path) -> (u32, u32, String, String) {
    let output = std::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height,sample_aspect_ratio,display_aspect_ratio",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .expect("ffprobe sulla geometria");
    let text = String::from_utf8_lossy(&output.stdout);
    let fields: Vec<&str> = text.trim().split(',').collect();
    assert!(fields.len() >= 4, "output ffprobe inatteso: {text:?}");
    (
        fields[0].parse().expect("larghezza"),
        fields[1].parse().expect("altezza"),
        fields[2].to_string(),
        fields[3].to_string(),
    )
}

/// Rapporto numerico da una frazione "a:b" di ffprobe.
pub fn ratio(value: &str) -> f64 {
    let (left, right) = value.split_once(':').expect("frazione a:b");
    left.parse::<f64>().expect("numeratore") / right.parse::<f64>().expect("denominatore")
}

/// Codec del primo stream video di un file, secondo ffprobe.
pub fn video_codec(path: &Path) -> String {
    let output = std::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=codec_name",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(path)
        .output()
        .expect("ffprobe sull'output");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// Corpo multipart con un solo file, costruito a mano per non dipendere da
/// helper esterni.
pub fn multipart_body(field: &str, filename: &str, content: &[u8]) -> (String, Vec<u8>) {
    let boundary = "----av1webconverterboundary";
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        format!("Content-Disposition: form-data; name=\"{field}\"; filename=\"{filename}\"\r\n")
            .as_bytes(),
    );
    body.extend_from_slice(b"Content-Type: application/octet-stream\r\n\r\n");
    body.extend_from_slice(content);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

/// Attende che il job raggiunga uno stato terminale.
pub async fn wait_for_job_status(
    server: &TestServer,
    job_id: &str,
    timeout: Duration,
) -> serde_json::Value {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let job: serde_json::Value = server
            .client
            .get(server.url(&format!("/api/jobs/{job_id}")))
            .send()
            .await
            .expect("lettura job")
            .json()
            .await
            .expect("json del job");
        let status = job["status"].as_str().unwrap_or_default().to_string();
        if matches!(status.as_str(), "completed" | "failed" | "stopped") {
            return job;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "il job è rimasto in stato {status} oltre il tempo massimo"
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}
