# Phase 7 — Modalità di analisi: API, frontend, end-to-end

> **Intent:** rendere la modalità utilizzabile dall'interfaccia, con i suoi tre
> campi visibili solo quando è attiva e l'esito dell'analisi leggibile nella
> riga del file, e provarla con test end-to-end.
> **Shippable alone?** sì — completa la modalità introdotta dalla fase 6 senza
> toccare il percorso di conversione.
> **Preconditions:** fase 6 `DONE` (la modalità funziona via API e § 6.5 ha
> registrato un esito reale).

## State contract (mandatory)

1. Prima di toccare qualsiasi cosa: leggere [STATE.md](STATE.md) e verificare
   che punti a una sotto-fase di questa fase. Eseguire i comandi di gate
   elencati in STATE.md **§3** e confrontare il risultato con quanto dichiarato
   in §1 e §7; vince il repository, quindi correggere il file se divergono.
2. Dopo **ogni** sotto-fase: aggiornare `STATE.md` (posizione, ledger, file
   toccati, lavoro in sospeso, verifiche, deviazioni, `Next action:`,
   timestamp) e sincronizzare `resume.md`.
3. Se la sessione termina a metà di una sotto-fase, scrivere in `STATE.md` §6
   esattamente cosa resta a metà prima di fermarsi.

Regole di progetto: italiano per identificatori, commenti e testi visibili;
nessun emoji; nessuna nuova cartella (test in `tests/`, asset in `src/web/`).

---

## Sub-phases

### 7.1 Contratto API e test di superficie

- **Model:** `agent:gpt5.6-luna`
- **Assignment:** `agent:gpt5.6-luna` — implementazione dei test e aggiornamento
  del contratto.
- **Files:** `docs/api.md` (parametri di encoding, corpo di `POST /api/jobs`,
  rappresentazione del file, `GET /api/defaults`), `tests/api.rs`.
- **Change:**
  1. Nessuna modifica di codice API: i parametri viaggiano dentro
     `EncodingParams` e il campo `vmaf_analysis` dentro `JobFile`, entrambi già
     serializzati. Verificarlo con i test invece di aggiungere codice.
  2. Aggiornare `docs/api.md`:
     - valore `vmaf_analysis` fra quelli ammessi per `mode`, con una riga su
       cosa comporta (analisi preliminare, poi codifica CRF a un passaggio);
     - i tre parametri `vmaf_segments` (1–10, default 3), `crf_test_steps`
       (1–10, default 3), `vmaf_target` (0–100, default 93), con la nota che
       fuori intervallo la creazione del job risponde `400`;
     - il campo `vmaf_analysis` nella rappresentazione del file: `null` se il
       file non è stato convertito in quella modalità, altrimenti
       `{ crf_scelto, crf_di_partenza, target, modello, motivo, segmenti[], risultati[], duration_seconds, error }`
       con i valori ammessi di `motivo` (`sopra_soglia`, `tutti_sopra_soglia`,
       `nessuno_sopra_soglia`, `analisi_non_eseguibile`) e la forma di
       `risultati[]` (`crf`, `vmaf_medio`, `bitrate_medio_bps`,
       `vmaf_per_segmento[]`);
     - nota che in questa modalità `progress.total_passes` comprende anche le
       sonde, due passaggi per sonda.
     Mantenere lo stile del documento.
  3. **T-AN9** in `tests/api.rs`:
     `i_default_espongono_i_parametri_di_analisi` — `GET /api/defaults`
     restituisce `params.vmaf_segments == 3`, `params.crf_test_steps == 3`,
     `params.vmaf_target == 93.0`;
     `un_job_in_modalita_analisi_viene_accettato` — `POST /api/jobs` con
     `params.mode = "vmaf_analysis"` risponde `2xx` e la lettura del job
     restituisce la stessa modalità;
     `i_parametri_di_analisi_fuori_intervallo_sono_rifiutati` — `POST /api/jobs`
     con `vmaf_segments = 0`, con `crf_test_steps = 11` e con
     `vmaf_target = 150` risponde `400` in tutti e tre i casi;
     `un_file_appena_caricato_non_ha_analisi` — `files[0].vmaf_analysis == null`.
- **Unit tests:** nessuno (test di integrazione HTTP).
- **e2e tests:** T-AN9 come sopra — non richiede ffmpeg.
- **Done:** `cargo test --test api` verde con i quattro test nuovi, `docs/api.md`
  descrive modalità, parametri e campo `vmaf_analysis` + `STATE.md` e
  `resume.md` aggiornati.

### 7.2 Frontend: modalità, campi condizionali e esito dell'analisi

> **Attenzione, modifica strutturale del frontend:** oggi la visibilità per
> modalità ammette una sola modalità per campo (`field.mode` e `data-mode`,
> `src/web/app.js:193-198`). Con tre modalità serve poter dichiarare più
> modalità per campo, altrimenti attivando l'analisi spariscono `crf` e
> `svtav1_params`, che invece servono.

- **Model:** `agent:gpt5.6-luna`
- **Assignment:** `agent:gpt5.6-luna` — implementazione.
- **Files:** `src/web/app.js:13-52` (`PARAM_FIELDS`),
  `src/web/app.js:193-198` (`applyModeVisibility`),
  `src/web/app.js:216-260` (costruzione dei campi nell'editor del job),
  `src/web/app.js:346-396` (`renderFileRow`, testo di dettaglio),
  `src/web/index.html:50-120` (form "Nuovo job").
- **Change:**
  1. Estendere il meccanismo di visibilità a più modalità, senza rompere quello
     esistente:
     - in `PARAM_FIELDS` accettare `modes: ["crf", "vmaf_analysis"]` accanto al
       vecchio `mode: "crf"` (chi ha `mode` continua a funzionare: trattarlo
       come `modes: [mode]`);
     - nell'HTML l'attributo diventa una lista separata da spazi, per esempio
       `data-mode="crf vmaf_analysis"`;
     - `applyModeVisibility` deve mostrare il nodo se la modalità corrente è fra
       quelle dichiarate:
       ```javascript
       root.querySelectorAll("[data-mode]").forEach((node) => {
         const modes = node.dataset.mode.split(/\s+/).filter(Boolean);
         node.classList.toggle("hidden", !modes.includes(mode));
       });
       ```
     - dove l'editor del job costruisce i campi da `PARAM_FIELDS`, scrivere
       `data-mode` unendo le modalità dichiarate con uno spazio.
  2. Aggiungere la terza opzione alla tendina della modalità, in
     `PARAM_FIELDS` (voce `mode`, `src/web/app.js:14-22`) e nel `select`
     statico (`src/web/index.html:52-57`):
     `["vmaf_analysis", "Analisi VMAF + Qualità Costante"]`.
  3. Riassegnare le modalità dei campi esistenti:
     - `crf`: da `mode: "crf"` a `modes: ["crf", "vmaf_analysis"]` (in analisi è
       il punto di partenza; aggiornare l'etichetta di aiuto nell'HTML per
       dirlo);
     - `svtav1_params`: da `mode: "crf"` a `modes: ["crf", "vmaf_analysis"]`;
     - `target_size_mb` e `size_margin_percent` restano solo `target_size`.
  4. Aggiungere i tre campi nuovi, visibili solo in analisi
     (`modes: ["vmaf_analysis"]`), in `PARAM_FIELDS` subito dopo `crf`:
     ```javascript
     { key: "vmaf_segments", label: "VMAF Segments", type: "number", min: 1, max: 10, modes: ["vmaf_analysis"] },
     { key: "crf_test_steps", label: "CRF Test Steps", type: "number", min: 1, max: 10, modes: ["vmaf_analysis"] },
     { key: "vmaf_target", label: "VMAF Target", type: "number", min: 0, max: 100, step: 0.5, modes: ["vmaf_analysis"] },
     ```
     e i corrispondenti campi statici in `src/web/index.html`, dentro contenitori
     con `data-mode="vmaf_analysis"`, con `name` uguale alla chiave
     (`vmaf_segments`, `crf_test_steps`, `vmaf_target`) e un testo di aiuto che
     spieghi in una riga: quanti spezzoni da 30 secondi vengono sondati, quanti
     CRF si provano a passi di +2 partendo dal CRF impostato, e che il target è
     la qualità media minima da garantire.
  5. Mostrare l'esito nella riga del file, accanto al punteggio VMAF introdotto
     in fase 5:
     ```javascript
     ${file.vmaf_analysis ? `<span class="file-detail">${analisiText(file.vmaf_analysis)}</span>` : ""}
     ```
     con la funzione accanto a `vmafText`:
     ```javascript
     /** Esito dell'analisi: CRF scelto e perché. */
     function analisiText(analisi) {
       const nota = {
         sopra_soglia: "",
         tutti_sopra_soglia: " (tutti gli step sopra il target)",
         nessuno_sopra_soglia: " (nessuno step sopra il target: usato il CRF impostato)",
         analisi_non_eseguibile: " (analisi non eseguibile)",
       }[analisi.motivo] ?? "";
       return `analisi VMAF: CRF ${analisi.crf_scelto}, target ${analisi.target}${nota}`;
     }
     ```
     Aggiungere anche l'attributo `title` con la tabella per CRF, una riga per
     risultato (`CRF 44: VMAF 95.10`), così il dettaglio è consultabile senza
     aprire i log. Passare ogni valore proveniente dal backend attraverso
     `escapeHtml`, come fa il resto del file.
  6. Non toccare `passLabel`: mostra già `passaggio X/Y` in modo generico e
     copre anche i passaggi delle sonde.
- **Unit tests:** nessuno (il frontend non ha suite di test nel progetto).
- **e2e tests:** verifica manuale registrata in `STATE.md` §7: selezionando
  "Analisi VMAF + Qualità Costante" compaiono i tre campi e restano visibili
  `CRF` e `svtav1-params`, mentre spariscono i campi di `target_size`;
  selezionando `CRF` i tre campi spariscono; un job in analisi mostra nella riga
  del file `analisi VMAF: CRF <n>, target <t>` e il dettaglio nel tooltip.
- **Done:** le tre modalità funzionano nel form "Nuovo job" e nell'editor dei
  parametri di un job esistente, nessun campo utile viene nascosto per errore,
  l'esito compare nella riga del file, gates verdi + `STATE.md` e `resume.md`
  aggiornati.

### 7.3 Test end-to-end della modalità

- **Model:** `agent:gpt5.6-luna`
- **Assignment:** `agent:gpt5.6-luna` — implementazione; approvare in autorevisione le
  asserzioni, in particolare la coerenza fra CRF scelto e criterio.
- **Files:** `tests/common/mod.rs` (helper già presenti; eventuale helper per
  clip più lunghi), `tests/e2e_conversion.rs` (nuovi test in coda).
- **Change:**
  1. Se serve un clip più lungo di quelli prodotti da `make_test_video`
     (`tests/common/mod.rs:115`), estendere quella funzione con il parametro di
     durata già presente invece di crearne una nuova.
  2. **T-AN10** `la_modalita_di_analisi_scegle_il_crf_e_lo_usa` — salta senza
      `ffmpeg_with_svtav1()` o `ffmpeg_with_libvmaf()`; clip di 120 secondi;
      parametri `mode: VmafAnalysis`, `crf: 40`, `crf_test_steps: 3`,
      `vmaf_segments: 3`, `vmaf_target: 93.0`, `preset: 12`, `scale_height: 480`,
     `cpus: Some(1)` per tenere il test breve e stabile. Asserzioni:
     - il file arriva a `completed` e l'MP4 esiste, con codec `av1`;
     - `files[0].vmaf_analysis` è presente, `crf_di_partenza == 40`,
        `risultati` ha tre righe con `crf == 42`, `crf == 44` e `crf == 46`, ognuna
        con `vmaf_per_segmento` di lunghezza 3;
     - `crf_scelto` è coerente con il criterio applicato ai valori riportati:
       se esistono righe con `vmaf_medio >= 93` allora `crf_scelto` è il massimo
       `crf` fra quelle, altrimenti `crf_scelto == 40`; l'asserzione va scritta
       ricalcolando la regola dai dati restituiti, non fissando un numero
       atteso, perché il punteggio dipende dal contenuto del clip;
     - `motivo` è coerente con lo stesso ricalcolo;
      - il log del file contiene una riga che inizia con
        `Analisi VMAF - CRF scelto:` e riporta lo stesso CRF di `crf_scelto`
        (I-8).
      - il log contiene anche `Codifica finale Analisi VMAF con CRF <crf_scelto>`,
        prova indipendente che codifica finale usa valore persistito.
  3. **T-AN11** `un_file_corto_viene_analizzato_per_intero` — clip di 10
     secondi, `vmaf_segments: 3`: `vmaf_analysis.segmenti` ha un solo elemento
     con `inizio_secondi == 0.0` e durata pari a quella del clip, e il file
     arriva comunque a `completed` (D23).
   4. **T-AN12** `un_target_inarrivabile_ripiega_sul_crf_impostato` — stessi
      parametri di T-AN10 ma `vmaf_target: 99.9`: `motivo` è
     `nessuno_sopra_soglia`, `crf_scelto == 40`, e il log lo dichiara. Prova che
     il criterio non degrada mai la qualità sotto quella richiesta
     dall'utente.
  5. Rieseguire l'intera suite anche con i binari dell'immagine:
     ```
     PATH="$PWD/target/ffmpeg9/bin:$PATH" cargo test --test e2e_conversion
     ```
- **Unit tests:** nessuno in questa sotto-fase.
- **e2e tests:** T-AN10, T-AN11, T-AN12, più T-E2E-ALL e T-VMAF1 rieseguiti.
- **Done:** i tre test nuovi passano con i binari dell'immagine, la suite
  completa resta verde, gli esiti sono in `STATE.md` §7 + `STATE.md` e
  `resume.md` aggiornati.

### 7.4 Update README.md

- **Model:** `agent:gpt5.6-luna`
- **Assignment:** `agent:gpt5.6-luna` — documentazione; eseguire in autorevisione la
  lettura finale della documentazione dell'intero piano.
- **Files:** `README.md` (sezione dell'interfaccia web, sezione delle modalità,
  limiti noti, risoluzione dei problemi).
- **Change:** completare la documentazione per l'uso dall'interfaccia:
  - come si seleziona "Analisi VMAF + Qualità Costante" e quali campi compaiono
    (`VMAF Segments`, `CRF Test Steps`, `VMAF Target`), con il significato di
    ognuno e i default;
  - un esempio concreto e leggibile: CRF impostato 40, 3 step, 3 segmenti,
    target 93 significa provare 42, 44 e 46 su tre spezzoni da 30 secondi e
    usare il CRF più alto che resta sopra 93;
  - dove si legge l'esito: la riga del file mostra il CRF scelto e il target, il
    dettaglio per CRF è nel tooltip, il riepilogo completo nei log di
    conversione;
  - che durante l'analisi la barra di avanzamento conta anche le sonde, quindi
    il numero di passaggi è più alto del normale;
  - consiglio pratico per il caso d'uso tipico (480p/720p per la visione su
    cellulare): il modello di misura predefinito è quello tarato per lo schermo
    del telefono, quindi il target 93 corrisponde a una qualità già molto alta
    in quella condizione di visione; se tutti gli step restano sopra il target
    conviene aumentare "CRF Test Steps" per guadagnare ancora spazio;
  - risoluzione dei problemi: nessun risultato di analisi significa ffmpeg senza
    filtro `libvmaf`; analisi molto lenta significa preset troppo basso o troppi
    segmenti per step;
  - limiti noti: l'analisi vale per il singolo file; i risultati non sono
    confrontabili fra modelli diversi; il punteggio misura la perdita da
    compressione alla risoluzione di uscita, non quella dovuta al
    ridimensionamento.
  Preservare struttura, tono e lingua; modificare, non riscrivere. Nessun nome
  di file, modulo o funzione, nessun riferimento al piano o alle fasi.
- **Unit tests:** nessuno (documentazione).
- **e2e tests:** nessuno — i passaggi documentati vanno eseguiti almeno una
  volta dall'interfaccia.
- **Done:** un utente attiva la modalità dall'interfaccia, capisce quali CRF
  verranno provati e legge il CRF scelto senza aprire il codice o i log; nessun
  dettaglio implementativo; gates verdi; `STATE.md` e `resume.md` aggiornati.

---

## Phase gates

- **Fmt:** `cargo fmt --all -- --check`
- **Lint:** `cargo clippy --all-targets -- -D warnings`
- **Test subset:** `cargo test --lib`, `cargo test --test api`,
  `PATH="$PWD/target/ffmpeg9/bin:$PATH" cargo test --test e2e_conversion`
- **Regression guard:** T-E2E-ALL, T-VMAF1, I-7 (nessuna sonda fuori dalla nuova
  modalità), I-8 (il CRF usato è quello registrato e compare nel log).
- **README:** aggiornato con l'uso dall'interfaccia, l'esempio numerico, i
  limiti e la risoluzione dei problemi.

## Phase done criterion

Lo scenario B del piano è verificabile end-to-end e dall'interfaccia: scelta la
modalità "Analisi VMAF + Qualità Costante" compaiono i tre campi, il job sonda
gli spezzoni, il CRF scelto compare nella riga del file e nel log, l'MP4 viene
codificato con quel CRF, e un target inarrivabile fa ripiegare sul CRF impostato
dall'utente dichiarandolo. `docs/api.md` e `README.md` descrivono la modalità.
