//! Test di integrazione delle API: autenticazione, validazione, ciclo di vita
//! dei job e degli upload. Non richiedono una conversione reale.

mod common;

use std::time::Duration;

use serde_json::json;

#[tokio::test]
async fn healthz_risponde_senza_credenziali() {
    let server = common::start_with(|config| config.auth.enabled = true).await;
    let response = server
        .client
        .get(server.url("/healthz"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["status"], "ok");
}

#[tokio::test]
async fn basic_auth_protegge_frontend_e_api() {
    let server = common::start_with(|config| config.auth.enabled = true).await;

    for path in ["/", "/api/jobs", "/api/defaults"] {
        let response = server.client.get(server.url(path)).send().await.unwrap();
        assert_eq!(
            response.status(),
            401,
            "{path} dovrebbe richiedere le credenziali"
        );
        assert!(response.headers().contains_key("www-authenticate"));
    }

    let response = server
        .client
        .get(server.url("/api/jobs"))
        .basic_auth("admin", Some("segreta"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);

    let response = server
        .client
        .get(server.url("/api/jobs"))
        .basic_auth("admin", Some("sbagliata"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 401);
}

#[tokio::test]
async fn il_frontend_e_servito_dal_binario() {
    let server = common::start().await;

    let index = server.client.get(server.url("/")).send().await.unwrap();
    assert_eq!(index.status(), 200);
    let html = index.text().await.unwrap();
    assert!(html.contains("av1webconverter"));
    assert!(html.contains(concat!("v", env!("CARGO_PKG_VERSION"))));
    assert!(!html.contains("{{APP_VERSION}}"));
    assert!(html.contains("/app.js"));

    for (path, marker) in [("/app.js", "EventSource"), ("/style.css", "--accent")] {
        let response = server.client.get(server.url(path)).send().await.unwrap();
        assert_eq!(response.status(), 200, "{path}");
        assert!(response.text().await.unwrap().contains(marker), "{path}");
    }
}

#[tokio::test]
async fn i_default_arrivano_dalla_configurazione() {
    let server = common::start_with(|config| {
        config.encoding.crf = 33;
        config.jobs.default_concurrency = 2;
    })
    .await;

    let body: serde_json::Value = server
        .client
        .get(server.url("/api/defaults"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(body["params"]["crf"], 33);
    assert_eq!(body["params"]["suffix"], "_av1");
    assert_eq!(body["concurrency"], 2);
    assert!(body["allowed_extensions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value == "mp4"));
}

#[tokio::test]
async fn i_default_espongono_i_parametri_vmaf() {
    let server = common::start().await;
    let body: serde_json::Value = server
        .client
        .get(server.url("/api/defaults"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert_eq!(body["params"]["measure_vmaf"], false);
    assert_eq!(body["params"]["vmaf_model"], "vmaf_v1.0.16_5d0h");
}

#[tokio::test]
async fn i_default_espongono_i_parametri_di_analisi() {
    let server = common::start().await;
    let body: serde_json::Value = server
        .client
        .get(server.url("/api/defaults"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert_eq!(body["params"]["vmaf_segments"], 3);
    assert_eq!(body["params"]["crf_test_steps"], 3);
    assert_eq!(body["params"]["vmaf_target"], 93.0);
    assert_eq!(body["params"]["vmaf_segment_duration_seconds"], 30.0);
}

#[tokio::test]
async fn un_job_in_modalita_analisi_viene_accettato() {
    let server = common::start().await;
    let response = server
        .client
        .post(server.url("/api/jobs"))
        .json(&json!({
            "name": "analisi API",
            "params": { "mode": "vmaf_analysis" }
        }))
        .send()
        .await
        .unwrap();

    assert!(response.status().is_success());
    let job: serde_json::Value = response.json().await.unwrap();
    assert_eq!(job["params"]["mode"], "vmaf_analysis");
}

#[tokio::test]
async fn un_job_accetta_durata_segmento_vmaf_personalizzata() {
    let server = common::start().await;
    let response = server
        .client
        .post(server.url("/api/jobs"))
        .json(&json!({
            "name": "durata personalizzata",
            "params": {
                "mode": "vmaf_analysis",
                "vmaf_segment_duration_seconds": 15.0
            }
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let job: serde_json::Value = response.json().await.unwrap();
    assert_eq!(job["params"]["vmaf_segment_duration_seconds"], 15.0);
}

#[tokio::test]
async fn i_parametri_di_analisi_fuori_intervallo_sono_rifiutati() {
    let server = common::start().await;
    for params in [
        json!({ "vmaf_segments": 0 }),
        json!({ "crf_test_steps": 11 }),
        json!({ "vmaf_target": 150.0 }),
        json!({ "vmaf_segment_duration_seconds": 0.5 }),
    ] {
        let response = server
            .client
            .post(server.url("/api/jobs"))
            .json(&json!({ "name": "parametro non valido", "params": params }))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 400, "parametri accettati: {params}");
    }
}

#[tokio::test]
async fn un_file_appena_caricato_non_ha_analisi_vmaf() {
    let server = common::start().await;
    let job_id = create_job(&server, "upload senza analisi").await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (content_type, body) = common::multipart_body("files", "film.mp4", b"contenuto mp4");

    server
        .client
        .post(server.url(&format!("/api/jobs/{job_id}/files")))
        .header("content-type", content_type)
        .body(body)
        .send()
        .await
        .unwrap();

    let job: serde_json::Value = server
        .client
        .get(server.url(&format!("/api/jobs/{job_id}")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(job["files"][0]["vmaf_analysis"].is_null());
}

async fn create_job(server: &common::TestServer, name: &str) -> serde_json::Value {
    let response = server
        .client
        .post(server.url("/api/jobs"))
        .json(&json!({ "name": name }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    response.json().await.unwrap()
}

#[tokio::test]
async fn creazione_job_usa_i_default_e_crea_le_cartelle() {
    let server = common::start().await;
    let job = create_job(&server, "  serie tv  ").await;

    assert_eq!(job["name"], "serie tv");
    assert_eq!(job["status"], "pending");
    assert_eq!(job["params"]["crf"], 50);
    assert_eq!(job["params"]["suffix"], "_av1");
    assert_eq!(job["overall_progress"], 0.0);

    let job_id = job["id"].as_str().unwrap();
    assert!(server.input_dir.join(job_id).is_dir());
    assert!(server.output_dir.join(job_id).is_dir());
}

#[tokio::test]
async fn un_file_appena_caricato_non_ha_misura_vmaf() {
    let server = common::start().await;
    let job_id = create_job(&server, "upload senza vmaf").await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (content_type, body) = common::multipart_body("files", "film.mp4", b"contenuto mp4");

    server
        .client
        .post(server.url(&format!("/api/jobs/{job_id}/files")))
        .header("content-type", content_type)
        .body(body)
        .send()
        .await
        .unwrap();

    let job: serde_json::Value = server
        .client
        .get(server.url(&format!("/api/jobs/{job_id}")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(job["files"][0]["vmaf"].is_null());
}

#[tokio::test]
async fn un_modello_vmaf_non_valido_viene_rifiutato() {
    let server = common::start().await;
    let response = server
        .client
        .post(server.url("/api/jobs"))
        .json(&json!({
            "name": "modello non valido",
            "params": { "vmaf_model": "vmaf; rm -rf /" }
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 400);
}

#[tokio::test]
async fn creazione_job_rifiuta_input_non_valido() {
    let server = common::start().await;

    let cases = vec![
        json!({ "name": "" }),
        json!({ "name": "x", "concurrency": 0 }),
        json!({ "name": "x", "params": { "crf": 99 } }),
        json!({ "name": "x", "params": { "scale_height": 8 } }),
        json!({ "name": "x", "params": { "scale_height": 5000 } }),
        json!({ "name": "x", "params": { "mode": "target_size" } }),
        json!({ "name": "x", "params": { "svtav1_params": "tune=5; rm -rf /" } }),
        json!({ "name": "x", "params": { "suffix": "../fuori" } }),
    ];
    for case in cases {
        let response = server
            .client
            .post(server.url("/api/jobs"))
            .json(&case)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 400, "caso non rifiutato: {case}");
    }
}

#[tokio::test]
async fn modifica_dei_parametri_del_job() {
    let server = common::start().await;
    let job = create_job(&server, "job").await;
    let job_id = job["id"].as_str().unwrap();

    let updated: serde_json::Value = server
        .client
        .patch(server.url(&format!("/api/jobs/{job_id}")))
        .json(&json!({
            "name": "rinominato",
            "concurrency": 3,
            "params": { "crf": 40, "preset": 8, "scale_height": 720, "suffix": "_web" }
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert_eq!(updated["name"], "rinominato");
    assert_eq!(updated["concurrency"], 3);
    assert_eq!(updated["params"]["crf"], 40);
    assert_eq!(updated["params"]["scale_height"], 720);
    assert_eq!(updated["params"]["suffix"], "_web");

    // La modifica è persistita anche dopo una rilettura.
    let reread: serde_json::Value = server
        .client
        .get(server.url(&format!("/api/jobs/{job_id}")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(reread["params"]["preset"], 8);
    assert_eq!(reread["params"]["suffix"], "_web");
}

#[tokio::test]
async fn la_dimensione_originale_e_accettata_dall_api() {
    let server = common::start().await;
    let job = create_job(&server, "originale").await;
    let job_id = job["id"].as_str().unwrap();

    let response = server
        .client
        .patch(server.url(&format!("/api/jobs/{job_id}")))
        .json(&json!({ "params": { "scale_height": 0 } }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);

    let updated: serde_json::Value = response.json().await.unwrap();
    assert_eq!(updated["params"]["scale_height"], 0);
}

#[tokio::test]
async fn un_altezza_come_stringa_viene_rifiutata() {
    let server = common::start().await;

    let response = server
        .client
        .post(server.url("/api/jobs"))
        .json(&json!({ "name": "x", "params": { "scale_height": "480" } }))
        .send()
        .await
        .unwrap();

    // Il rifiuto arriva dall'estrattore Json prima della validazione.
    let status = response.status();
    assert!(
        status.is_client_error(),
        "un'altezza stringa deve essere rifiutata, ricevuto {status}"
    );
}

#[tokio::test]
async fn job_inesistente_risponde_404() {
    let server = common::start().await;
    let missing = uuid::Uuid::new_v4();
    for path in [
        format!("/api/jobs/{missing}"),
        format!("/api/jobs/{missing}/download"),
    ] {
        let response = server.client.get(server.url(&path)).send().await.unwrap();
        assert_eq!(response.status(), 404, "{path}");
    }
    let response = server
        .client
        .post(server.url(&format!("/api/jobs/{missing}/start")))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 404);
}

#[tokio::test]
async fn upload_accetta_mp4_e_rifiuta_il_resto() {
    let server = common::start().await;
    let job_id = create_job(&server, "upload").await["id"]
        .as_str()
        .unwrap()
        .to_string();

    let (content_type, body) =
        common::multipart_body("files", "cartella/film.mp4", b"contenuto mp4");
    let response = server
        .client
        .post(server.url(&format!("/api/jobs/{job_id}/files")))
        .header("content-type", content_type)
        .body(body)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["uploaded"], 1);
    assert_eq!(
        body["job"]["files"][0]["original_name"],
        "cartella/film.mp4"
    );
    // Il percorso relativo non finisce sul filesystem.
    assert_eq!(body["job"]["files"][0]["stored_name"], "film.mp4");
    assert!(server.input_dir.join(&job_id).join("film.mp4").exists());

    let (content_type, body) = common::multipart_body("files", "archivio.zip", b"non un video");
    let rejected: serde_json::Value = server
        .client
        .post(server.url(&format!("/api/jobs/{job_id}/files")))
        .header("content-type", content_type)
        .body(body)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(rejected["uploaded"], 0);
    assert_eq!(rejected["rejected"][0]["name"], "archivio.zip");
    assert_eq!(rejected["job"]["files"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn nomi_duplicati_restano_distinti_su_disco() {
    let server = common::start().await;
    let job_id = create_job(&server, "duplicati").await["id"]
        .as_str()
        .unwrap()
        .to_string();

    for _ in 0..2 {
        let (content_type, body) = common::multipart_body("files", "film.mp4", b"dati");
        server
            .client
            .post(server.url(&format!("/api/jobs/{job_id}/files")))
            .header("content-type", content_type)
            .body(body)
            .send()
            .await
            .unwrap();
    }

    let job: serde_json::Value = server
        .client
        .get(server.url(&format!("/api/jobs/{job_id}")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let stored: Vec<&str> = job["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| file["stored_name"].as_str().unwrap())
        .collect();
    assert_eq!(stored, vec!["film.mp4", "film_1.mp4"]);
}

#[tokio::test]
async fn rimozione_di_un_file_cancella_anche_il_sorgente() {
    let server = common::start().await;
    let job_id = create_job(&server, "rimozione").await["id"]
        .as_str()
        .unwrap()
        .to_string();

    let (content_type, body) = common::multipart_body("files", "film.mp4", b"dati");
    let uploaded: serde_json::Value = server
        .client
        .post(server.url(&format!("/api/jobs/{job_id}/files")))
        .header("content-type", content_type)
        .body(body)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let file_id = uploaded["job"]["files"][0]["id"].as_str().unwrap();
    let source = server.input_dir.join(&job_id).join("film.mp4");
    assert!(source.exists());

    let response = server
        .client
        .delete(server.url(&format!("/api/jobs/{job_id}/files/{file_id}")))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let job: serde_json::Value = response.json().await.unwrap();
    assert!(job["files"].as_array().unwrap().is_empty());
    assert!(!source.exists());
}

#[tokio::test]
async fn un_job_senza_file_non_puo_partire() {
    let server = common::start().await;
    let job_id = create_job(&server, "vuoto").await["id"]
        .as_str()
        .unwrap()
        .to_string();

    let response = server
        .client
        .post(server.url(&format!("/api/jobs/{job_id}/start")))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 400);
}

#[tokio::test]
async fn download_senza_file_convertiti_e_un_conflitto() {
    let server = common::start().await;
    let job_id = create_job(&server, "senza output").await["id"]
        .as_str()
        .unwrap()
        .to_string();

    let response = server
        .client
        .get(server.url(&format!("/api/jobs/{job_id}/download")))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 409);
}

#[tokio::test]
async fn eliminazione_job_pulisce_le_cartelle() {
    let server = common::start().await;
    let job_id = create_job(&server, "da eliminare").await["id"]
        .as_str()
        .unwrap()
        .to_string();

    let (content_type, body) = common::multipart_body("files", "film.mp4", b"dati");
    server
        .client
        .post(server.url(&format!("/api/jobs/{job_id}/files")))
        .header("content-type", content_type)
        .body(body)
        .send()
        .await
        .unwrap();

    let response = server
        .client
        .delete(server.url(&format!("/api/jobs/{job_id}")))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);

    assert!(!server.input_dir.join(&job_id).exists());
    assert!(!server.output_dir.join(&job_id).exists());
    let jobs: serde_json::Value = server
        .client
        .get(server.url("/api/jobs"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(jobs.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn lo_stream_sse_pubblica_gli_eventi_dei_job() {
    let server = common::start().await;

    let mut response = server
        .client
        .get(server.url("/api/events"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert!(response
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("text/event-stream"));

    // Il job creato dopo l'apertura dello stream deve comparire fra gli eventi.
    let client = server.client.clone();
    let url = server.url("/api/jobs");
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(150)).await;
        let _ = client
            .post(url)
            .json(&json!({ "name": "osservato" }))
            .send()
            .await;
    });

    let mut buffer = String::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(2), response.chunk()).await {
            Ok(Ok(Some(chunk))) => {
                buffer.push_str(&String::from_utf8_lossy(&chunk));
                if buffer.contains("job_updated") && buffer.contains("osservato") {
                    return;
                }
            }
            Ok(Ok(None)) => break,
            Ok(Err(_)) | Err(_) => continue,
        }
    }
    panic!("evento job_updated non ricevuto; ricevuto: {buffer}");
}
