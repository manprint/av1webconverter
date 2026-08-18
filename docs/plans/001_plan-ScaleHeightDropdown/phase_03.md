# Phase 3 — Integrazione, test end-to-end e documentazione

> **Intent:** provare su conversione reale che `Dimensione originale` non
> ridimensiona nulla, coprire con test API il dominio ammesso e la coercizione
> numerica, e allineare la documentazione tecnica.
> **Shippable alone?** yes — non introduce comportamento nuovo, chiude la
> verifica e i documenti.
> **Preconditions:** phase_01 DONE, phase_02 DONE.

## State contract (mandatory)

1. Prima di toccare qualunque cosa: leggi [STATE.md](STATE.md) e verifica che
   punti a una sub-fase di questa fase. Esegui i comandi di gate elencati in
   `STATE.md` **§3** e confronta il risultato con quanto dichiarano §1 e §7. Il
   repo ha ragione: se divergono, correggi il file.
2. Dopo **ogni** sub-fase qui sotto: aggiorna `STATE.md` (posizione, ledger, file
   toccati, lavoro a metà, verifica, deviazioni, `Next action:`, timestamp) e
   allinea `resume.md`. Una sub-fase non è finita prima che questo sia scritto.
3. Se la sessione finisce a metà sub-fase, scrivi in `STATE.md` §6 esattamente
   cosa è rimasto a metà, prima di fermarti.

---

## Sub-phases

### 3.1 Test API: dominio ammesso e tipo del campo

- **Model:** `agent:gpt5.6-luna`
- **Assignment:** `agent:gpt5.6-luna` — implementazione e self-review.
- **Files:** `tests/api.rs:132-153` (`creazione_job_rifiuta_input_non_valido`),
  `tests/api.rs:155-195` (zona in cui aggiungere i test nuovi, accanto a
  `modifica_dei_parametri_del_job`)
- **Change:**
  1. **T-SH4** — nel vettore `cases` di `creazione_job_rifiuta_input_non_valido`
     (`tests/api.rs:135-141`) aggiungi due casi, dopo
     `json!({ "name": "x", "params": { "crf": 99 } })`:
     ```rust
     json!({ "name": "x", "params": { "scale_height": 8 } }),
     json!({ "name": "x", "params": { "scale_height": 5000 } }),
     ```
     Il loop esistente asserisce già `400` per ogni caso: entrambi finiscono in
     `EncodingParams::validate` e producono `ApiError::BadRequest`.
  2. **T-SH3** — aggiungi un test nuovo dopo `modifica_dei_parametri_del_job`
     (che finisce a `tests/api.rs:195`):
     ```rust
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
     ```
     Riusa l'helper `create_job` (`tests/api.rs:103`).
  3. **T-SH5** — aggiungi un test nuovo subito dopo T-SH3, che documenta perché
     il frontend deve inviare un numero (fase 2 § 2.1):
     ```rust
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

         // Il rifiuto arriva dall'estrattore Json prima della validazione, quindi
         // il codice esatto dipende da axum: basta che sia un errore del client.
         let status = response.status();
         assert!(
             status.is_client_error(),
             "un'altezza stringa deve essere rifiutata, ricevuto {status}"
         );
     }
     ```
     Non fissare il codice a `400`: una stringa al posto di un `u32` è un errore
     di deserializzazione dell'estrattore `Json`, non di `validate()`, e axum
     risponde con un codice diverso da quello degli errori semantici. Registra in
     `STATE.md` §7 il codice effettivamente osservato.
- **Unit tests:** i tre test sopra sono i test di integrazione HTTP di questa
  sub-fase; non serve altro a livello di unità.
- **e2e tests:** none in questa sub-fase.
- **Done:** gates verdi (`cargo fmt --all -- --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test --lib`,
  `cargo test --test api`) + `modifica_dei_parametri_del_job` resta verde senza
  modifiche (usa ancora `scale_height: 720`) + `STATE.md` e `resume.md` aggiornati.

---

### 3.2 T-SH1 — end-to-end: `Dimensione originale` non ridimensiona

> **È l'assert di accettazione del piano.** Self-review obbligatoria sugli assert:
> un test che passa per la ragione sbagliata (per esempio perché il sorgente è già
> alto 480) non prova nulla.

- **Model:** `agent:gpt5.6-luna`
- **Assignment:** `agent:gpt5.6-luna` — implementazione; **self-review obbligatoria**
  sugli assert di accettazione.
- **Files:** `tests/e2e_conversion.rs` — nuovo test inserito dopo
  `un_sorgente_anamorfico_conserva_l_aspetto_visivo` (che finisce poco prima di
  `tests/e2e_conversion.rs:535`)
- **Change:** aggiungi il test seguente, modellato su
  `un_sorgente_anamorfico_conserva_l_aspetto_visivo`
  (`tests/e2e_conversion.rs:480-534`), da cui riusa lo stesso sorgente e gli stessi
  helper:
  ```rust
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
      assert_ne!(source_sar, "1:1", "il sorgente deve avere pixel non quadrati");

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
  ```
  Verifica prima di eseguire che gli import in testa al file coprano già
  `Duration`, `json!` e `upload_file`: sono gli stessi usati dal test anamorfico
  esistente, quindi non dovrebbero servire import nuovi. `_source_height` non
  serve separatamente perché la tupla del sorgente viene confrontata interamente.
- **Unit tests:** none (test di integrazione end-to-end).
- **e2e tests:** **T-SH1** — `la_dimensione_originale_non_ridimensiona`. Criterio
  osservabile: output `720x576` con SAR identico al sorgente e job `completed`.
  **T-SH2** — `un_sorgente_anamorfico_conserva_l_aspetto_visivo` (esistente, a
  480) deve restare verde senza modifiche: è la prova che il percorso con
  ridimensionamento non è regredito (I-2).
- **Done:** `cargo test --test e2e_conversion` verde con **T-SH1 e T-SH2
  effettivamente eseguiti** — se `common::ffmpeg_with_svtav1()`
  (`tests/common/mod.rs:94`) fa saltare i test, l'esito va registrato in
  `STATE.md` §7 come `could-not-run`, non come `pass`, e la sub-fase resta
  `IN_PROGRESS` finché non è stato possibile eseguirla almeno una volta su una
  macchina con `libsvtav1` + gates verdi + `STATE.md` e `resume.md` aggiornati.

---

### 3.3 Documentazione tecnica: API e configurazione di esempio

- **Model:** `agent:gpt5.6-luna`
- **Assignment:** `agent:gpt5.6-luna` — documentazione e self-review.
- **Files:** `docs/api.md:86-99` (`POST /api/jobs`), `config.example.toml:64`
- **Change:**
  1. In `docs/api.md`, sotto `### POST /api/jobs`, dopo il paragrafo su
     `params.suffix` (`docs/api.md:96-98`), aggiungi un paragrafo nuovo:
     ```markdown
     `params.scale_height` è l'altezza massima dell'output in pixel: `0` significa
     dimensione originale, cioè nessun ridimensionamento e rapporto dei pixel del
     sorgente conservato. Sono ammessi `0` e i valori fra 16 e 4320; l'interfaccia
     web propone un sottoinsieme di valori comuni, l'API accetta tutto
     l'intervallo.
     ```
  2. In `config.example.toml`, sostituisci il commento della riga 64:
     ```toml
     scale_height = 480          # AV1WC_SCALE_HEIGHT — altezza massima, aspetto mantenuto, SAR 1:1
     ```
     con:
     ```toml
     scale_height = 480          # AV1WC_SCALE_HEIGHT — altezza massima, aspetto mantenuto, SAR 1:1; 0 = dimensione originale
     ```
     Mantieni l'allineamento delle colonne del commento coerente con le righe
     vicine.
  3. `docs/architettura.md` non menziona il ridimensionamento (verificato con
     `grep -n "scala\|ridimension\|altezza\|-vf" docs/architettura.md`, nessun
     risultato): non va modificato. Se il grep dà risultati diversi da quando il
     piano è stato scritto, aggiorna il documento e registra la deviazione in
     `STATE.md` §8.
  4. `README.image.md` documenta le misure di qualità e script non presenti: non
     va toccato.
- **Unit tests:** none (documentazione).
- **e2e tests:** none.
- **Done:** gates verdi + `docs/api.md` e `config.example.toml` descrivono il
  dominio `{0} ∪ 16..=4320` con la semantica di `0` + `STATE.md` e `resume.md`
  aggiornati.

---

### 3.4 Update README.md

Sub-fase di chiusura obbligatoria dell'ultima fase: qui la lettura finale del
README è un gate esplicito.

- **Model:** `agent:gpt5.6-luna`
- **Assignment:** `agent:gpt5.6-luna` — documentazione; **self-review obbligatoria**
  come lettura finale dei documenti utente.
- **Files:** `README.md` (radice del repo)
- **Change:** questa fase non aggiunge comportamento utente nuovo: verifica e
  chiudi ciò che le fasi 1 e 2 hanno scritto.
  1. Rileggi da capo le sezioni toccate — `## Funzionalità`,
     `## Configurazione` (tabella env), `## Parametri di conversione` — e controlla:
     coerenza fra la lista di valori documentata e le nove voci effettivamente
     presenti in `src/web/index.html`; presenza del default 480px; spiegazione di
     `Dimensione originale`; menzione del fatto che l'API accetta anche altezze
     fuori dalla lista.
  2. Rimuovi eventuali dettagli implementativi che si fossero infiltrati nelle
     fasi precedenti (nomi di funzioni, `-vf`, nomi di file sorgente, riferimenti
     a fasi o a questo piano).
  3. Aggiungi, se manca, una riga fra i limiti noti: una conversione con
     `Dimensione originale` produce file più grandi e richiede più tempo, perché
     l'altezza del sorgente non viene ridotta.
  Preserva struttura, tono e lingua (italiano). Nessuna emoji.
- **Unit tests:** none (documentazione).
- **e2e tests:** none — gli esempi citati nel README sono stati eseguiti nelle fasi
  1 e 2.
- **Done:** un utente nuovo installa e usa la funzione dal solo README, senza
  leggere il codice; nessun dettaglio implementativo presente; la lista di valori
  nel README coincide con quella in `src/web/index.html`; gates verdi; `STATE.md` e
  `resume.md` aggiornati.

---

## Phase gates

- **Fmt:** `cargo fmt --all -- --check`
- **Lint:** `cargo clippy --all-targets -- -D warnings`
- **Test:** suite completa, gli stessi tre comandi di `STATE.md` §3 —
  `cargo test --lib`, `cargo test --test api`, `cargo test --test e2e_conversion`
- **Regression guard:** T-SH2 (`un_sorgente_anamorfico_conserva_l_aspetto_visivo`),
  `conversione_crf_dall_upload_al_download`, `conversione_a_dimensione_target`,
  `modifica_dei_parametri_del_job`, `comando_crf_riproduce_lo_script_di_riferimento`
- **README:** riletto integralmente sulle sezioni toccate, coerente con la UI,
  privo di dettagli implementativi

## Phase done criterion

`cargo test` è verde con **T-SH1 eseguito e non saltato**, e l'output della
conversione con `scale_height = 0` misura `720x576` con il SAR del sorgente
(scenario di riferimento di `overview.md`). `docs/api.md`, `config.example.toml` e
`README.md` descrivono il dominio ammesso e la semantica di `0`. `README.md`
riflette il comportamento rilasciato dall'intero piano.
