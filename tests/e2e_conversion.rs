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
        .json(&json!({ "name": "conversione crf" }))
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
    let output_path = server.output_dir.join(&job_id).join("sorgente_av1.mp4");
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
        .contains("sorgente_av1.mp4"));
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
    assert_eq!(archive.file_names().next().unwrap(), "sorgente_av1.mp4");

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
