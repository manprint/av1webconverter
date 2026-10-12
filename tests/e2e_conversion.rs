//! Test end-to-end: conversione reale con ffmpeg, dall'upload al download.
//!
//! Richiedono `ffmpeg`/`ffprobe` con SVT-AV1 nel PATH; in assenza vengono
//! saltati con un messaggio, per non far fallire una build senza codec.

mod common;

use std::time::Duration;

use serde_json::json;

/// Carica un file dal disco nel job indicato.
async fn upload_file(
    server: &common::TestServer,
    job_id: &str,
    name: &str,
    path: &std::path::Path,
) -> serde_json::Value {
    let content = std::fs::read(path).expect("lettura sorgente di prova");
    let (content_type, body) = common::multipart_body("files", name, &content);
    let response = server
        .client
        .post(server.url(&format!("/api/jobs/{job_id}/files")))
        .header("content-type", content_type)
        .body(body)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    response.json().await.unwrap()
}

async fn start_analysis_job(
    server: &common::TestServer,
    source: &std::path::Path,
    name: &str,
    vmaf_segments: u32,
    crf_test_steps: u32,
    vmaf_target: f64,
) -> (String, String) {
    let job: serde_json::Value = server
        .client
        .post(server.url("/api/jobs"))
        .json(&json!({
            "name": name,
            "params": {
                "mode": "vmaf_analysis",
                "crf": 40,
                "crf_test_steps": crf_test_steps,
                "vmaf_segments": vmaf_segments,
                "vmaf_target": vmaf_target,
                "preset": 12,
                "scale_height": 480,
                "cpus": 1
            }
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let job_id = job["id"].as_str().unwrap().to_string();
    let uploaded = upload_file(server, &job_id, "sorgente.mp4", source).await;
    let file_id = uploaded["job"]["files"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let response = server
        .client
        .post(server.url(&format!("/api/jobs/{job_id}/start")))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    (job_id, file_id)
}

async fn file_log_text(server: &common::TestServer, job_id: &str, file_id: &str) -> String {
    let log: serde_json::Value = server
        .client
        .get(server.url(&format!("/api/jobs/{job_id}/files/{file_id}/log")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    log["lines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|line| line.as_str().unwrap())
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn conversione_crf_dall_upload_al_download() {
    if !common::ffmpeg_with_svtav1() {
        eprintln!("ffmpeg con SVT-AV1 e reset_sar non disponibile: test saltato");
        return;
    }

    let server = common::start().await;
    let source = server.temp.path().join("sorgente.mp4");
    common::make_test_video(&source, 2);

    let job: serde_json::Value = server
        .client
        .post(server.url("/api/jobs"))
        .json(&json!({ "name": "conversione crf", "params": { "suffix": "_web" } }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let job_id = job["id"].as_str().unwrap().to_string();

    let uploaded = upload_file(&server, &job_id, "sorgente.mp4", &source).await;
    assert_eq!(uploaded["uploaded"], 1);
    let file_id = uploaded["job"]["files"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let started = server
        .client
        .post(server.url(&format!("/api/jobs/{job_id}/start")))
        .send()
        .await
        .unwrap();
    assert_eq!(started.status(), 200);

    // Riavviare un job già in corso è un conflitto.
    let duplicate = server
        .client
        .post(server.url(&format!("/api/jobs/{job_id}/start")))
        .send()
        .await
        .unwrap();
    assert!(
        duplicate.status() == 409 || duplicate.status() == 200,
        "stato inatteso al secondo avvio: {}",
        duplicate.status()
    );

    let job = common::wait_for_job_status(&server, &job_id, Duration::from_secs(300)).await;
    assert_eq!(job["status"], "completed", "job non completato: {job}");
    assert_eq!(job["counts"]["completed"], 1);
    assert!((job["overall_progress"].as_f64().unwrap() - 100.0).abs() < 0.01);

    let file = &job["files"][0];
    assert_eq!(file["status"], "completed");
    assert!(file["output_size_bytes"].as_u64().unwrap() > 0);
    assert_eq!(file["progress"]["percent"], 100.0);

    // L'output è dove lo aspettiamo e contiene davvero AV1.
    let output_path = server.output_dir.join(&job_id).join("sorgente_web.mp4");
    assert!(
        output_path.exists(),
        "output assente: {}",
        output_path.display()
    );
    assert_eq!(common::video_codec(&output_path), "av1");

    // Il log di conversione contiene le righe di progresso in formato script.
    let log: serde_json::Value = server
        .client
        .get(server.url(&format!("/api/jobs/{job_id}/files/{file_id}/log")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let lines = log["lines"].as_array().unwrap();
    assert!(lines
        .iter()
        .any(|line| line.as_str().unwrap().contains("Avvio conversione")));
    assert!(lines
        .iter()
        .any(|line| line.as_str().unwrap().contains("[Codifica]")));
    assert!(lines
        .iter()
        .any(|line| line.as_str().unwrap().contains("Completato:")));

    // Download del singolo file.
    let response = server
        .client
        .get(server.url(&format!("/api/jobs/{job_id}/files/{file_id}/download")))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert!(response
        .headers()
        .get("content-disposition")
        .unwrap()
        .to_str()
        .unwrap()
        .contains("sorgente_web.mp4"));
    assert!(!response.bytes().await.unwrap().is_empty());

    // Download dell'intero job come ZIP.
    let response = server
        .client
        .get(server.url(&format!("/api/jobs/{job_id}/download")))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let zip_bytes = response.bytes().await.unwrap();
    assert_eq!(&zip_bytes[..2], b"PK");

    // Download della selezione.
    let response = server
        .client
        .post(server.url(&format!("/api/jobs/{job_id}/download")))
        .json(&json!({ "file_ids": [file_id] }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let selection = response.bytes().await.unwrap();
    let archive = zip::ZipArchive::new(std::io::Cursor::new(selection.to_vec()))
        .expect("archivio ZIP valido");
    assert_eq!(archive.len(), 1);
    assert_eq!(
        archive.file_names().next().unwrap().unwrap(),
        "sorgente_web.mp4"
    );

    // La cartella temporanea non deve accumulare residui.
    let temp_entries: Vec<_> = std::fs::read_dir(server.temp.path().join("data/tmp"))
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    assert!(
        temp_entries.is_empty(),
        "file temporanei rimasti: {:?}",
        temp_entries
            .iter()
            .map(|entry| entry.path())
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn i_file_vengono_convertiti_in_sequenza() {
    if !common::ffmpeg_with_svtav1() {
        eprintln!("ffmpeg con SVT-AV1 e reset_sar non disponibile: test saltato");
        return;
    }

    let server = common::start().await;
    let source = server.temp.path().join("sorgente.mp4");
    common::make_test_video(&source, 1);

    let job: serde_json::Value = server
        .client
        .post(server.url("/api/jobs"))
        .json(&json!({ "name": "sequenza", "concurrency": 1 }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let job_id = job["id"].as_str().unwrap().to_string();

    for name in ["primo.mp4", "secondo.mp4", "terzo.mp4"] {
        upload_file(&server, &job_id, name, &source).await;
    }

    server
        .client
        .post(server.url(&format!("/api/jobs/{job_id}/start")))
        .send()
        .await
        .unwrap();

    // Con concurrency = 1 non può esserci più di un file in conversione.
    let mut observed_running_max = 0usize;
    let deadline = std::time::Instant::now() + Duration::from_secs(300);
    loop {
        let current: serde_json::Value = server
            .client
            .get(server.url(&format!("/api/jobs/{job_id}")))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let running = current["counts"]["running"].as_u64().unwrap() as usize;
        observed_running_max = observed_running_max.max(running);
        assert!(running <= 1, "più di un file in conversione: {running}");
        if matches!(
            current["status"].as_str().unwrap(),
            "completed" | "failed" | "stopped"
        ) {
            assert_eq!(current["status"], "completed", "{current}");
            assert_eq!(current["counts"]["completed"], 3);
            break;
        }
        assert!(std::time::Instant::now() < deadline, "job troppo lento");
        tokio::time::sleep(Duration::from_millis(120)).await;
    }
    assert_eq!(
        observed_running_max, 1,
        "nessun file osservato in conversione"
    );

    for name in ["primo_av1.mp4", "secondo_av1.mp4", "terzo_av1.mp4"] {
        assert!(
            server.output_dir.join(&job_id).join(name).exists(),
            "{name}"
        );
    }
}

#[tokio::test]
async fn un_job_in_corso_puo_essere_fermato() {
    if !common::ffmpeg_with_svtav1() {
        eprintln!("ffmpeg con SVT-AV1 e reset_sar non disponibile: test saltato");
        return;
    }

    // Un preset lento e una sorgente lunga danno il tempo di premere stop.
    let server = common::start_with(|config| {
        config.encoding.preset = 2;
        config.encoding.scale_height = 720;
    })
    .await;
    let source = server.temp.path().join("lungo.mp4");
    common::make_test_video(&source, 40);

    let job: serde_json::Value = server
        .client
        .post(server.url("/api/jobs"))
        .json(&json!({ "name": "da fermare" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let job_id = job["id"].as_str().unwrap().to_string();
    upload_file(&server, &job_id, "lungo.mp4", &source).await;

    server
        .client
        .post(server.url(&format!("/api/jobs/{job_id}/start")))
        .send()
        .await
        .unwrap();

    // Si attende che la conversione sia davvero partita, altrimenti lo stop
    // arriverebbe prima dell'avvio di ffmpeg e il test non proverebbe nulla.
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    let mut observed_running = false;
    while std::time::Instant::now() < deadline {
        let current: serde_json::Value = server
            .client
            .get(server.url(&format!("/api/jobs/{job_id}")))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        if current["files"][0]["status"] == "running"
            && current["files"][0]["progress"]["percent"]
                .as_f64()
                .unwrap_or(0.0)
                > 0.0
        {
            observed_running = true;
            break;
        }
        if current["status"] != "running" && current["status"] != "pending" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        observed_running,
        "la conversione non è mai stata osservata in corso: impossibile testare lo stop"
    );

    let stopped: serde_json::Value = server
        .client
        .post(server.url(&format!("/api/jobs/{job_id}/stop")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert_eq!(
        stopped["status"], "stopped",
        "stato inatteso dopo lo stop: {stopped}"
    );
    assert_eq!(stopped["files"][0]["status"], "cancelled");
    // Nessun output parziale resta sul disco: il file finale compare solo dopo
    // il remux completato.
    assert!(!server
        .output_dir
        .join(&job_id)
        .join("lungo_av1.mp4")
        .exists());

    // Nemmeno i temporanei devono sopravvivere all'interruzione.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let leftovers: Vec<_> = std::fs::read_dir(server.temp.path().join("data/tmp"))
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect();
    assert!(leftovers.is_empty(), "temporanei rimasti: {leftovers:?}");
}

#[tokio::test]
async fn un_output_gia_presente_viene_saltato() {
    if !common::ffmpeg_with_svtav1() {
        eprintln!("ffmpeg con SVT-AV1 e reset_sar non disponibile: test saltato");
        return;
    }

    let server = common::start().await;
    let source = server.temp.path().join("sorgente.mp4");
    common::make_test_video(&source, 1);

    let job: serde_json::Value = server
        .client
        .post(server.url("/api/jobs"))
        .json(&json!({ "name": "salto" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let job_id = job["id"].as_str().unwrap().to_string();
    upload_file(&server, &job_id, "sorgente.mp4", &source).await;

    // Output preesistente, con `overwrite` disattivo per default.
    let output_path = server.output_dir.join(&job_id).join("sorgente_av1.mp4");
    std::fs::write(&output_path, b"output preesistente").unwrap();

    server
        .client
        .post(server.url(&format!("/api/jobs/{job_id}/start")))
        .send()
        .await
        .unwrap();
    let job = common::wait_for_job_status(&server, &job_id, Duration::from_secs(120)).await;

    assert_eq!(job["status"], "completed");
    assert_eq!(job["files"][0]["status"], "skipped");
    assert_eq!(job["counts"]["skipped"], 1);
    assert_eq!(
        std::fs::read(&output_path).unwrap(),
        b"output preesistente",
        "l'output preesistente è stato sovrascritto"
    );
}

#[tokio::test]
async fn conversione_a_dimensione_target() {
    if !common::ffmpeg_with_svtav1() {
        eprintln!("ffmpeg con SVT-AV1 e reset_sar non disponibile: test saltato");
        return;
    }

    let server = common::start().await;
    let source = server.temp.path().join("sorgente.mp4");
    common::make_test_video(&source, 2);

    let job: serde_json::Value = server
        .client
        .post(server.url("/api/jobs"))
        .json(&json!({
            "name": "dimensione fissa",
            "params": {
                "mode": "target_size",
                "target_size_mb": 1.0,
                // I preset 10+ di SVT-AV1 4.2 sono riservati all'automazione e
                // il primo passaggio va in crash su questo clip minimo.
                "preset": 8,
                "scale_height": 120,
                "progress_interval": 0.5
            }
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let job_id = job["id"].as_str().unwrap().to_string();
    let file_id = upload_file(&server, &job_id, "sorgente.mp4", &source).await["job"]["files"][0]
        ["id"]
        .as_str()
        .unwrap()
        .to_string();

    server
        .client
        .post(server.url(&format!("/api/jobs/{job_id}/start")))
        .send()
        .await
        .unwrap();
    let job = common::wait_for_job_status(&server, &job_id, Duration::from_secs(300)).await;
    assert_eq!(job["status"], "completed", "{job}");

    let output_path = server.output_dir.join(&job_id).join("sorgente_av1.mp4");
    assert_eq!(common::video_codec(&output_path), "av1");
    let size = std::fs::metadata(&output_path).unwrap().len();
    assert!(size > 0 && size < 4_000_000, "dimensione inattesa: {size}");

    // Il log dichiara il bitrate calcolato e i due passaggi.
    let log: serde_json::Value = server
        .client
        .get(server.url(&format!("/api/jobs/{job_id}/files/{file_id}/log")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let text = log["lines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|line| line.as_str().unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("bitrate video calcolato"), "{text}");
    assert!(text.contains("passaggio 2/2"), "{text}");
}

/// Un sorgente anamorfico (pixel non quadrati) deve uscire a pixel quadrati
/// conservando l'aspetto visivo grazie a `reset_sar=1` sul filtro scale.
#[tokio::test]
async fn un_sorgente_anamorfico_conserva_l_aspetto_visivo() {
    if !common::ffmpeg_with_svtav1() {
        eprintln!("ffmpeg con SVT-AV1 e reset_sar non disponibile: test saltato");
        return;
    }

    let server = common::start_with(|config| config.encoding.scale_height = 480).await;
    let source = server.temp.path().join("anamorfico.mp4");
    common::make_anamorphic_video(&source, 2);

    let (source_width, source_height, source_sar, source_dar) = common::video_geometry(&source);
    assert_eq!((source_width, source_height), (720, 576));
    assert_ne!(
        source_sar, "1:1",
        "il sorgente deve avere pixel non quadrati"
    );
    let source_ratio = common::ratio(&source_dar);

    let job: serde_json::Value = server
        .client
        .post(server.url("/api/jobs"))
        .json(&json!({ "name": "anamorfico" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let job_id = job["id"].as_str().unwrap().to_string();
    upload_file(&server, &job_id, "anamorfico.mp4", &source).await;

    server
        .client
        .post(server.url(&format!("/api/jobs/{job_id}/start")))
        .send()
        .await
        .unwrap();
    let job = common::wait_for_job_status(&server, &job_id, Duration::from_secs(300)).await;
    assert_eq!(job["status"], "completed", "{job}");

    let output_path = server.output_dir.join(&job_id).join("anamorfico_av1.mp4");
    let (width, height, sar, dar) = common::video_geometry(&output_path);

    assert_eq!(height, 480, "altezza non limitata a scale_height");
    assert_eq!(sar, "1:1", "l'output deve avere pixel quadrati");
    // 720x576 con DAR 16:9 a 480 di altezza dà 854x480.
    assert_eq!(width, 854, "larghezza non ricavata dal DAR del sorgente");
    let output_ratio = common::ratio(&dar);
    assert!(
        (output_ratio - source_ratio).abs() < 0.01,
        "aspetto visivo alterato: sorgente {source_dar} ({source_ratio:.4}) -> output {dar} ({output_ratio:.4})"
    );
}

#[tokio::test]
async fn la_dimensione_originale_non_ridimensiona() {
    if !common::ffmpeg_with_svtav1() {
        eprintln!("ffmpeg con SVT-AV1 non disponibile: test saltato");
        return;
    }

    // 0 = dimensione originale: nessun -vf, quindi nemmeno normalizzazione del SAR.
    let server = common::start_with(|config| config.encoding.scale_height = 0).await;
    let source = server.temp.path().join("anamorfico.mp4");
    common::make_anamorphic_video(&source, 2);

    let (source_width, source_height, source_sar, source_dar) = common::video_geometry(&source);
    assert_eq!((source_width, source_height), (720, 576));
    assert_ne!(
        source_sar, "1:1",
        "il sorgente deve avere pixel non quadrati"
    );

    let job: serde_json::Value = server
        .client
        .post(server.url("/api/jobs"))
        .json(&json!({ "name": "originale" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let job_id = job["id"].as_str().unwrap().to_string();
    assert_eq!(job["params"]["scale_height"], 0);
    upload_file(&server, &job_id, "anamorfico.mp4", &source).await;

    server
        .client
        .post(server.url(&format!("/api/jobs/{job_id}/start")))
        .send()
        .await
        .unwrap();
    let job = common::wait_for_job_status(&server, &job_id, Duration::from_secs(300)).await;
    assert_eq!(job["status"], "completed", "{job}");

    let output_path = server.output_dir.join(&job_id).join("anamorfico_av1.mp4");
    let (width, height, sar, dar) = common::video_geometry(&output_path);

    assert_eq!(
        (width, height),
        (source_width, source_height),
        "la dimensione originale non deve essere toccata"
    );
    assert_eq!(sar, source_sar, "il SAR del sorgente deve restare intatto");
    let source_ratio = common::ratio(&source_dar);
    let output_ratio = common::ratio(&dar);
    assert!(
        (output_ratio - source_ratio).abs() < 0.01,
        "aspetto visivo alterato: {source_dar} -> {dar}"
    );
    assert_eq!(common::video_codec(&output_path), "av1");
}

#[tokio::test]
async fn un_sorgente_non_valido_fa_fallire_solo_il_suo_file() {
    if !common::ffmpeg_with_svtav1() {
        eprintln!("ffmpeg con SVT-AV1 e reset_sar non disponibile: test saltato");
        return;
    }

    let server = common::start().await;
    let source = server.temp.path().join("sorgente.mp4");
    common::make_test_video(&source, 1);

    let job: serde_json::Value = server
        .client
        .post(server.url("/api/jobs"))
        .json(&json!({ "name": "misto" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let job_id = job["id"].as_str().unwrap().to_string();

    // Il primo file non è un video, il secondo sì.
    let (content_type, body) = common::multipart_body("files", "rotto.mp4", b"non e' un video");
    server
        .client
        .post(server.url(&format!("/api/jobs/{job_id}/files")))
        .header("content-type", content_type)
        .body(body)
        .send()
        .await
        .unwrap();
    upload_file(&server, &job_id, "buono.mp4", &source).await;

    server
        .client
        .post(server.url(&format!("/api/jobs/{job_id}/start")))
        .send()
        .await
        .unwrap();
    let job = common::wait_for_job_status(&server, &job_id, Duration::from_secs(180)).await;

    assert_eq!(job["status"], "failed", "{job}");
    assert_eq!(job["counts"]["failed"], 1);
    assert_eq!(job["counts"]["completed"], 1);

    let broken = job["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|file| file["original_name"] == "rotto.mp4")
        .unwrap();
    assert_eq!(broken["status"], "failed");
    assert!(broken["error"].as_str().unwrap().contains("ffprobe"));

    // Il file valido resta scaricabile nonostante l'errore dell'altro.
    assert!(server
        .output_dir
        .join(&job_id)
        .join("buono_av1.mp4")
        .exists());
}

#[tokio::test]
async fn la_misura_vmaf_produce_un_punteggio_e_non_altera_l_output() {
    if !common::ffmpeg_with_svtav1() || !common::ffmpeg_with_libvmaf() {
        eprintln!("ffmpeg con SVT-AV1, reset_sar e libvmaf non disponibile: test saltato");
        return;
    }

    let server = common::start().await;
    let source = server.temp.path().join("sorgente-vmaf.mp4");
    common::make_test_video(&source, 5);

    let create_client = server.client.clone();
    let create_base_url = server.base_url.clone();
    let create = move |name: String, measure_vmaf| {
        let client = create_client.clone();
        let url = format!("{create_base_url}/api/jobs");
        async move {
            client
                .post(url)
                .json(&json!({
                    "name": name,
                    "params": {
                        "measure_vmaf": measure_vmaf,
                        "scale_height": 720,
                        "cpus": 1
                    }
                }))
                .send()
                .await
                .unwrap()
                .json::<serde_json::Value>()
                .await
                .unwrap()
        }
    };

    let without = create("senza misura".into(), false).await;
    let without_id = without["id"].as_str().unwrap().to_string();
    upload_file(&server, &without_id, "senza.mp4", &source).await;
    server
        .client
        .post(server.url(&format!("/api/jobs/{without_id}/start")))
        .send()
        .await
        .unwrap();
    let without_job =
        common::wait_for_job_status(&server, &without_id, Duration::from_secs(300)).await;
    assert_eq!(without_job["status"], "completed", "{without_job}");
    let without_output = server.output_dir.join(&without_id).join("senza_av1.mp4");
    let without_bytes = std::fs::read(&without_output).expect("output senza misura");

    let with = create("con misura".into(), true).await;
    let with_id = with["id"].as_str().unwrap().to_string();
    let uploaded = upload_file(&server, &with_id, "con.mp4", &source).await;
    let file_id = uploaded["job"]["files"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    server
        .client
        .post(server.url(&format!("/api/jobs/{with_id}/start")))
        .send()
        .await
        .unwrap();
    let with_job = common::wait_for_job_status(&server, &with_id, Duration::from_secs(300)).await;

    assert_eq!(with_job["status"], "completed", "{with_job}");
    assert_eq!(with_job["files"][0]["status"], "completed");
    let vmaf = &with_job["files"][0]["vmaf"];
    assert_eq!(vmaf["model"], "vmaf_v1.0.16_5d0h");
    let score = vmaf["score"]
        .as_f64()
        .unwrap_or_else(|| panic!("punteggio VMAF assente: {with_job}"));
    assert!(score > 0.0 && score <= 100.0, "punteggio inatteso: {score}");
    assert!(vmaf["error"].is_null());
    assert!(vmaf["duration_seconds"].as_f64().unwrap() > 0.0);

    let with_output = server.output_dir.join(&with_id).join("con_av1.mp4");
    assert_eq!(without_bytes, std::fs::read(&with_output).unwrap());

    let log: serde_json::Value = server
        .client
        .get(server.url(&format!("/api/jobs/{with_id}/files/{file_id}/log")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(log["lines"]
        .as_array()
        .unwrap()
        .iter()
        .any(|line| line.as_str().unwrap().contains("VMAF")));
}

#[tokio::test]
async fn una_misura_vmaf_fallita_non_fa_fallire_il_file() {
    if !common::ffmpeg_with_svtav1() || !common::ffmpeg_with_libvmaf() {
        eprintln!("ffmpeg con SVT-AV1, reset_sar e libvmaf non disponibile: test saltato");
        return;
    }

    let server = common::start().await;
    let source = server.temp.path().join("sorgente-vmaf-errore.mp4");
    common::make_test_video(&source, 2);
    let job: serde_json::Value = server
        .client
        .post(server.url("/api/jobs"))
        .json(&json!({
            "name": "modello VMAF inesistente",
            "params": {
                "measure_vmaf": true,
                "vmaf_model": "vmaf_modello_inesistente"
            }
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let job_id = job["id"].as_str().unwrap().to_string();
    upload_file(&server, &job_id, "errore.mp4", &source).await;
    server
        .client
        .post(server.url(&format!("/api/jobs/{job_id}/start")))
        .send()
        .await
        .unwrap();

    let job = common::wait_for_job_status(&server, &job_id, Duration::from_secs(300)).await;
    assert_eq!(job["status"], "completed", "{job}");
    assert_eq!(job["files"][0]["status"], "completed");
    assert!(server
        .output_dir
        .join(&job_id)
        .join("errore_av1.mp4")
        .exists());
    assert!(common::video_codec(&server.output_dir.join(&job_id).join("errore_av1.mp4")) == "av1");
    assert!(job["files"][0]["vmaf"]["score"].is_null());
    assert!(job["files"][0]["vmaf"]["error"].as_str().is_some());
}

#[tokio::test]
async fn la_misura_vmaf_gestisce_un_sorgente_anamorfico() {
    if !common::ffmpeg_with_svtav1() || !common::ffmpeg_with_libvmaf() {
        eprintln!("ffmpeg con SVT-AV1, reset_sar e libvmaf non disponibile: test saltato");
        return;
    }

    let server = common::start().await;
    let source = server.temp.path().join("sorgente-anamorfico-vmaf.mp4");
    common::make_anamorphic_video(&source, 2);
    let job: serde_json::Value = server
        .client
        .post(server.url("/api/jobs"))
        .json(&json!({
            "name": "anamorfico VMAF",
            "params": { "scale_height": 480, "measure_vmaf": true }
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let job_id = job["id"].as_str().unwrap().to_string();
    upload_file(&server, &job_id, "anamorfico.mp4", &source).await;
    server
        .client
        .post(server.url(&format!("/api/jobs/{job_id}/start")))
        .send()
        .await
        .unwrap();

    let job = common::wait_for_job_status(&server, &job_id, Duration::from_secs(300)).await;
    assert_eq!(job["status"], "completed", "{job}");
    assert_eq!(job["files"][0]["status"], "completed");
    let score = job["files"][0]["vmaf"]["score"]
        .as_f64()
        .expect("punteggio VMAF anamorfico");
    assert!(score > 0.0 && score <= 100.0, "punteggio inatteso: {score}");
}

#[tokio::test]
async fn la_modalita_di_analisi_sceglie_il_crf_e_lo_usa() {
    if !common::ffmpeg_with_svtav1() || !common::ffmpeg_with_libvmaf() {
        eprintln!("ffmpeg con SVT-AV1, reset_sar e libvmaf non disponibile: test saltato");
        return;
    }

    let server = common::start().await;
    let source = server.temp.path().join("analisi-120s.mp4");
    common::make_test_video(&source, 120);
    let (job_id, file_id) =
        start_analysis_job(&server, &source, "analisi completa", 3, 3, 93.0).await;

    let job = common::wait_for_job_status(&server, &job_id, Duration::from_secs(900)).await;
    assert_eq!(job["status"], "completed", "job non completato: {job}");
    assert_eq!(job["files"][0]["status"], "completed");
    let output_path = server.output_dir.join(&job_id).join("sorgente_av1.mp4");
    assert!(output_path.exists());
    assert_eq!(common::video_codec(&output_path), "av1");

    let analisi = &job["files"][0]["vmaf_analysis"];
    assert_eq!(analisi["crf_di_partenza"], 40);
    let risultati = analisi["risultati"].as_array().unwrap();
    assert_eq!(risultati.len(), 3);
    assert_eq!(risultati[0]["crf"], 42);
    assert_eq!(risultati[1]["crf"], 44);
    assert_eq!(risultati[2]["crf"], 46);
    for risultato in risultati {
        assert_eq!(risultato["vmaf_per_segmento"].as_array().unwrap().len(), 3);
    }

    let target = analisi["target"].as_f64().unwrap();
    let validi: Vec<&serde_json::Value> = risultati
        .iter()
        .filter(|risultato| risultato["vmaf_medio"].as_f64().unwrap() >= target)
        .collect();
    let crf_scelto = analisi["crf_scelto"].as_u64().unwrap();
    let motivo = analisi["motivo"].as_str().unwrap();
    if let Some(expected) = validi
        .iter()
        .map(|risultato| risultato["crf"].as_u64().unwrap())
        .max()
    {
        assert_eq!(crf_scelto, expected);
        assert_eq!(
            motivo,
            if validi.len() == risultati.len() {
                "tutti_sopra_soglia"
            } else {
                "sopra_soglia"
            }
        );
    } else {
        assert_eq!(crf_scelto, 40);
        assert_eq!(motivo, "nessuno_sopra_soglia");
    }

    let log = file_log_text(&server, &job_id, &file_id).await;
    let riga_scelta = log
        .lines()
        .find(|line| line.contains("Analisi VMAF - CRF scelto:"))
        .expect("riga CRF scelto assente nel log");
    assert!(riga_scelta.contains("Analisi VMAF - CRF scelto:"));
    assert!(
        riga_scelta.contains(&format!("CRF scelto: {crf_scelto}")),
        "{riga_scelta}"
    );
    let riga_codifica = log
        .lines()
        .find(|line| line.contains("Codifica finale Analisi VMAF con CRF"))
        .expect("CRF codifica finale assente nel log");
    assert!(
        riga_codifica.ends_with(&format!("CRF {crf_scelto}")),
        "{riga_codifica}"
    );
}

#[tokio::test]
async fn un_file_corto_viene_analizzato_per_intero() {
    if !common::ffmpeg_with_svtav1() || !common::ffmpeg_with_libvmaf() {
        eprintln!("ffmpeg con SVT-AV1, reset_sar e libvmaf non disponibile: test saltato");
        return;
    }

    let server = common::start().await;
    let source = server.temp.path().join("analisi-corta.mp4");
    common::make_test_video(&source, 10);
    let (job_id, _file_id) =
        start_analysis_job(&server, &source, "analisi file corto", 3, 1, 93.0).await;

    let job = common::wait_for_job_status(&server, &job_id, Duration::from_secs(600)).await;
    assert_eq!(job["status"], "completed", "job non completato: {job}");
    let segmenti = job["files"][0]["vmaf_analysis"]["segmenti"]
        .as_array()
        .unwrap();
    assert_eq!(segmenti.len(), 1);
    assert_eq!(segmenti[0]["inizio_secondi"], 0.0);
    let durata = segmenti[0]["durata_secondi"].as_f64().unwrap();
    assert!(
        (9.0..=10.5).contains(&durata),
        "durata segmento inattesa: {durata}"
    );
}

#[tokio::test]
async fn un_target_inarrivabile_ripiega_sul_crf_impostato() {
    if !common::ffmpeg_with_svtav1() || !common::ffmpeg_with_libvmaf() {
        eprintln!("ffmpeg con SVT-AV1, reset_sar e libvmaf non disponibile: test saltato");
        return;
    }

    let server = common::start().await;
    let source = server.temp.path().join("analisi-fallback-120s.mp4");
    common::make_test_video(&source, 120);
    let (job_id, file_id) =
        start_analysis_job(&server, &source, "target irraggiungibile", 3, 3, 99.9).await;

    let job = common::wait_for_job_status(&server, &job_id, Duration::from_secs(900)).await;
    assert_eq!(job["status"], "completed", "job non completato: {job}");
    let analisi = &job["files"][0]["vmaf_analysis"];
    assert_eq!(analisi["motivo"], "nessuno_sopra_soglia");
    assert_eq!(analisi["crf_scelto"], 40);

    let log = file_log_text(&server, &job_id, &file_id).await;
    let riga_scelta = log
        .lines()
        .find(|line| line.contains("Analisi VMAF - CRF scelto:"))
        .expect("riga CRF scelto assente nel log");
    assert!(riga_scelta.contains("CRF scelto: 40"), "{riga_scelta}");
    assert!(
        riga_scelta.contains("si usa il CRF impostato dall'utente"),
        "{riga_scelta}"
    );
}
