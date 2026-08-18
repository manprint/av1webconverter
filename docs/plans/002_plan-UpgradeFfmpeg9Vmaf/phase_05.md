# Phase 5 — Misura VMAF: API, frontend, end-to-end

> **Intent:** rendere la misura utilizzabile e visibile: documentare il
> contratto API, esporre i due parametri e il punteggio nel frontend, e provare
> il comportamento con test end-to-end reali.
> **Shippable alone?** sì — completa la feature introdotta dalla fase 4 senza
> toccare il percorso di conversione.
> **Preconditions:** fase 4 `DONE` (`JobFile.vmaf` viene popolato dal worker con
> `measure_vmaf=true`).

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
nessun emoji; nessuna nuova cartella (i test vivono in `tests/`, gli asset
frontend in `src/web/`).

---

## Sub-phases

### 5.1 Contratto API e test di superficie

- **Model:** `agent:gpt5.6-luna`
- **Assignment:** `agent:gpt5.6-luna` — implementazione dei test e aggiornamento
  del contratto.
- **Files:** `docs/api.md` (sezioni dei parametri di encoding, del corpo di
  `POST /api/jobs`, della rappresentazione di un file e di `GET /api/defaults`),
  `tests/api.rs` (nuovi test).
- **Change:**
  1. Nessuna modifica di codice API è necessaria: `Defaults`
     (`src/api/jobs.rs:17-24`) serializza `EncodingParams` per intero e
     `JobView` incorpora `Job` con i suoi `files`, quindi i due parametri nuovi
     e il campo `vmaf` compaiono automaticamente. Verificarlo con i test qui
     sotto invece di aggiungere codice.
  2. Aggiornare `docs/api.md`:
     - elenco dei parametri di encoding: `measure_vmaf` (`bool`, default
       `false`) e `vmaf_model` (`string`, default `vmaf_v1.0.16_5d0h`), con la
       nota che un modello con caratteri non ammessi viene rifiutato con
       `400`;
     - rappresentazione di un file: campo `vmaf`, `null` quando la misura non è
       stata richiesta o il file non è ancora stato convertito, altrimenti
       oggetto `{ score, min, harmonic_mean, model, duration_seconds, error }`
       con i primi tre eventualmente `null` in caso di fallimento;
     - nota che `progress.total_passes` include il passaggio di misura quando è
       attiva.
     Mantenere lo stile del documento esistente.
  3. **T-VMAF5** in `tests/api.rs`:
     `i_default_espongono_i_parametri_vmaf` — `GET /api/defaults` restituisce
     `params.measure_vmaf == false` e `params.vmaf_model == "vmaf_v1.0.16_5d0h"`;
     `un_file_appena_caricato_non_ha_misura_vmaf` — creato un job e caricato un
     file (usare gli helper già presenti nel file di test), `GET /api/jobs/{id}`
     restituisce `files[0].vmaf == null`;
     `un_modello_vmaf_non_valido_viene_rifiutato` — `POST /api/jobs` con
     `params.vmaf_model = "vmaf; rm -rf /"` restituisce `400`.
- **Unit tests:** nessuno (i test di questa sotto-fase sono di integrazione
  HTTP, in `tests/api.rs`).
- **e2e tests:** T-VMAF5 come sopra — non richiede ffmpeg, quindi gira sempre.
- **Done:** `cargo test --test api` verde con i tre test nuovi, `docs/api.md`
  descrive i due parametri e il campo `vmaf` + `STATE.md` e `resume.md`
  aggiornati.

### 5.2 Frontend: parametri e punteggio visibile

- **Model:** `agent:gpt5.6-luna`
- **Assignment:** `agent:gpt5.6-luna` — implementazione.
- **Files:** `src/web/app.js:13-52` (`PARAM_FIELDS`),
  `src/web/app.js:346-396` (`renderFileRow` e `fileProgressText`),
  `src/web/index.html:93-116` (campi avanzati del form "Nuovo job").
- **Change:**
  1. `PARAM_FIELDS` (`src/web/app.js:13-52`): aggiungere due voci, dopo
     `progress_interval` e prima di `overwrite`, così l'ordine resta coerente
     con il form:
     ```javascript
     { key: "measure_vmaf", label: "Misura VMAF dopo la conversione", type: "checkbox" },
     { key: "vmaf_model", label: "Modello VMAF", type: "text" },
     ```
     `readParams` e `writeParams` funzionano già per chiave e tipo: nessun'altra
     modifica alla logica di lettura/scrittura.
  2. `src/web/index.html`: aggiungere due campi statici nella stessa griglia dei
     parametri avanzati, con `name` identici alle chiavi (è così che il
     JavaScript li trova, `src/web/app.js:137`):
     - una checkbox `id="param-measure-vmaf" name="measure_vmaf"` nello stesso
       blocco della checkbox `overwrite` (`src/web/index.html:113-115`), con
       etichetta "Misura VMAF dopo la conversione" e testo di aiuto che avverte
       del costo di un passaggio in più;
     - un input testuale `id="param-vmaf-model" name="vmaf_model"` accanto ai
       campi testuali avanzati (dopo `param-svtav1`,
       `src/web/index.html:93`), con `placeholder="vmaf_v1.0.16_5d0h"`.
  3. `renderFileRow` (`src/web/app.js:346`): nella colonna delle dimensioni
     (quella che mostra `in`/`out`) aggiungere una terza riga con il punteggio,
     solo quando presente:
     ```javascript
     ${file.vmaf ? `<span class="file-detail">${vmafText(file.vmaf)}</span>` : ""}
     ```
     e aggiungere la funzione accanto a `fileProgressText`
     (`src/web/app.js:383`):
     ```javascript
     /** Punteggio VMAF del file, o il motivo per cui manca. */
     function vmafText(vmaf) {
       if (vmaf.error) return `VMAF non disponibile: ${escapeHtml(vmaf.error)}`;
       if (typeof vmaf.score !== "number") return "VMAF non disponibile";
       return `VMAF ${vmaf.score.toFixed(2)} (${escapeHtml(vmaf.model)})`;
     }
     ```
     Usare `escapeHtml` (`src/web/app.js:397`) su tutto ciò che viene dal
     backend, come fa il resto del file.
  4. Non toccare `passLabel` (`src/web/app.js:353`): mostra già
     `passaggio X/Y` in modo generico e quindi copre da sé il passaggio di
     misura.
- **Unit tests:** nessuno (il frontend non ha suite di test nel progetto).
- **e2e tests:** verifica manuale registrata in `STATE.md` §7: avviato il server
  con `cargo run`, il form "Nuovo job" mostra i due campi nuovi, un job con
  misura attiva mostra `VMAF <punteggio> (<modello>)` nella riga del file, e un
  job senza misura non mostra nulla di aggiuntivo.
- **Done:** i due campi sono presenti e leggibili dal form e dall'editor dei
  parametri di un job esistente, il punteggio compare nella riga del file, il
  binario compila (il frontend è incluso con `include_str!`,
  `src/api/web.rs:10-13`), gates verdi + `STATE.md` e `resume.md` aggiornati.

### 5.3 Test end-to-end della misura

- **Model:** `agent:gpt5.6-luna`
- **Assignment:** `agent:gpt5.6-luna` — implementazione; approvare in autorevisione le
  asserzioni, in particolare quella di identità byte per byte dell'output.
- **Files:** `tests/common/mod.rs:94-113` (accanto a `ffmpeg_with_svtav1`, nuovo
  helper), `tests/e2e_conversion.rs` (nuovi test in coda al file).
- **Change:**
  1. Aggiungere in `tests/common/mod.rs`, nello stesso stile di
     `ffmpeg_with_svtav1`:
     ```rust
     /// I test di misura richiedono un ffmpeg con il filtro `libvmaf`, assente
     /// in molte build di sistema.
     pub fn ffmpeg_with_libvmaf() -> bool
     ```
     Implementazione: eseguire `ffmpeg -hide_banner -filters`, verificare che
     l'output contenga `libvmaf`; restituire `false` se il comando non parte.
  2. **T-VMAF1** in `tests/e2e_conversion.rs`:
     `la_misura_vmaf_produce_un_punteggio_e_non_altera_l_output`.
     - salta se `!common::ffmpeg_with_svtav1() || !common::ffmpeg_with_libvmaf()`,
       con lo stesso `eprintln!` di avviso usato dagli altri test;
     - prepara un sorgente con `common::make_test_video` (5 secondi bastano);
     - primo job: parametri con `measure_vmaf: false`, `scale_height: 720`,
       `cpus: Some(1)` — `cpus` fissato a 1 perché la codifica multi-thread non
       è garantita bit-esatta e l'asserzione di identità richiede
       determinismo; attende `completed`, legge i byte dell'MP4 prodotto;
     - secondo job: identici parametri ma `measure_vmaf: true`, sullo stesso
       sorgente, con suffisso o cartella diversi per non incappare nello
       `skipped` da output già presente;
     - asserzioni: `files[0].status == "completed"`; `files[0].vmaf` presente;
       `vmaf.model == "vmaf_v1.0.16_5d0h"`; `vmaf.score` presente e compreso in
       `(0.0, 100.0]`; `vmaf.error.is_none()`; `vmaf.duration_seconds > 0.0`;
       i byte dei due MP4 sono uguali (I-1);
     - la riga di log del file contiene `VMAF` (usare l'endpoint di log già
       coperto da `tests/api.rs`, oppure leggere il file di log del job).
  3. **T-VMAF6** in `tests/e2e_conversion.rs`:
     `una_misura_vmaf_fallita_non_fa_fallire_il_file` — stesso schema, con
     `vmaf_model: "vmaf_modello_inesistente"` (nome valido per il validatore ma
     ignoto a libvmaf): il file resta `completed`, l'MP4 esiste ed è AV1,
     `files[0].vmaf` è presente con `score == None` e `error` valorizzato
     (I-5).
  4. **T-VMAF7** in `tests/e2e_conversion.rs`:
     `la_misura_vmaf_gestisce_un_sorgente_anamorfico` — sorgente creato con
     `common::make_anamorphic_video` (`tests/common/mod.rs:140`),
     `scale_height: 480`, `measure_vmaf: true`: il file arriva a `completed`
     con `vmaf.score` presente, a prova che la riscalatura condizionale del
     riferimento e `reset_sar=1` producono due input confrontabili.
  5. Rieseguire l'intera suite anche con i binari della nuova immagine, come in
     fase 3:
     ```
     PATH="$PWD/target/ffmpeg9/bin:$PATH" cargo test --test e2e_conversion
     ```
     Asserzione: nessun test salta per assenza di `libvmaf`, perché l'immagine
     lo include (T-IMG1).
- **Unit tests:** nessuno in questa sotto-fase.
- **e2e tests:** T-VMAF1, T-VMAF6, T-VMAF7 come sopra, più T-E2E-ALL rieseguito.
- **Done:** i tre test nuovi passano con i binari dell'immagine, la suite
  completa resta verde, gli esiti sono in `STATE.md` §7 + `STATE.md` e
  `resume.md` aggiornati.

### 5.4 Update README.md

- **Model:** `agent:gpt5.6-luna`
- **Assignment:** `agent:gpt5.6-luna` — documentazione; eseguire in autorevisione la
  lettura finale della documentazione dell'intero piano.
- **Files:** `README.md` (sezione dell'interfaccia web, sezione dei parametri,
  sezione dei limiti noti e della risoluzione dei problemi).
- **Change:** completare la documentazione utente per ciò che questa fase rende
  usabile:
  - interfaccia web: dove si attiva la misura nel form "Nuovo job" e nell'editor
    dei parametri di un job esistente, e dove appare il punteggio (riga del
    file, accanto alle dimensioni);
  - come si legge il valore mostrato (`VMAF <punteggio> (<modello>)`) e cosa
    significa "VMAF non disponibile";
  - richiamo del fatto che durante la misura la barra di avanzamento mostra un
    passaggio in più;
  - risoluzione dei problemi: se il punteggio manca sempre, verificare che
    l'ffmpeg in uso includa il filtro `libvmaf`
    (`ffmpeg -hide_banner -filters | grep libvmaf`), presente
    nell'immagine fornita ma non in molte build di sistema; se il nome del
    modello è ignoto a libvmaf, il file resta convertito e il motivo compare nel
    campo di errore della misura;
  - limiti noti: la misura non modifica l'output; il confronto è valido solo fra
    file misurati con lo stesso modello; con più conversioni in parallelo la
    misura concorre per la CPU con le codifiche.
  Preservare struttura, tono e lingua; modificare, non riscrivere. Nessun nome
  di file, modulo o funzione, nessun riferimento al piano o alle fasi.
- **Unit tests:** nessuno (documentazione).
- **e2e tests:** nessuno — i comandi e i passaggi documentati vanno eseguiti
  almeno una volta.
- **Done:** un utente attiva la misura dall'interfaccia, legge il punteggio e
  diagnostica un punteggio mancante usando solo il README; nessun dettaglio
  implementativo; gates verdi; `STATE.md` e `resume.md` aggiornati.

---

## Phase gates

- **Fmt:** `cargo fmt --all -- --check`
- **Lint:** `cargo clippy --all-targets -- -D warnings`
- **Test subset:** `cargo test --lib`, `cargo test --test api`,
  `PATH="$PWD/target/ffmpeg9/bin:$PATH" cargo test --test e2e_conversion`
- **Regression guard:** T-E2E-ALL, I-1 (output identico con e senza misura),
  I-5 (misura fallita non fa fallire il file).
- **README:** aggiornato con l'uso dall'interfaccia, la lettura del punteggio, i
  limiti e la risoluzione dei problemi.

## Phase done criterion

Lo scenario di riferimento del piano è verificabile end-to-end: un job creato
con `measure_vmaf=true` restituisce via API un `vmaf.score` in `(0, 100]` con
modello `vmaf_v1.0.16_5d0h`, il frontend mostra il punteggio nella riga del
file, l'MP4 prodotto è identico byte per byte a quello dello stesso job senza
misura, e un modello ignoto lascia il file convertito con l'errore riportato
nella misura. `docs/api.md` e `README.md` descrivono la feature.
