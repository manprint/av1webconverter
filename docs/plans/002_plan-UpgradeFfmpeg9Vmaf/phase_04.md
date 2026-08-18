# Phase 4 — Misura VMAF: backend

> **Intent:** aggiungere una misura di qualità VMAF opzionale per file, calcolata
> dopo la conversione sull'output definitivo, persistita nel job e incapace di
> alterare o far fallire l'output.
> **Shippable alone?** sì — la misura è opt-in (`measure_vmaf` default `false`),
> quindi il comportamento esistente resta identico finché nessuno la attiva.
> **Preconditions:** fase 3 `DONE` (la nuova toolchain è verificata; T-IMG3 ha
> già provato che `vmaf_v1.0.16_5d0h` è caricabile nella build).

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

Regole di progetto valide per tutte le sotto-fase: identificatori, commenti,
messaggi di log e testi visibili all'utente in italiano; i commenti spiegano il
*perché*; nessun emoji; nessuna nuova cartella (il nuovo modulo va in
`src/ffmpeg/`, che già raccoglie l'integrazione ffmpeg).

---

## Sub-phases

### 4.1 Parametri di configurazione `measure_vmaf` e `vmaf_model`

- **Model:** `agent:gpt5.6-luna`
- **Assignment:** `agent:gpt5.6-luna` — implementazione.
- **Files:** `src/config.rs:25-51` (campi di `EncodingParams`),
  `src/config.rs:53-75` (blocco `Default`), `src/config.rs:77-110` (`validate`),
  `src/config.rs:431-455` (override da variabili d'ambiente),
  `config.example.toml` (sezione `[encoding]`).
- **Change:**
  1. Aggiungere a `EncodingParams` due campi, con commento in italiano che
     spiega il perché:
     ```rust
     /// Se attivo, dopo la conversione viene misurato il VMAF dell'output
     /// rispetto al sorgente. Costa un passaggio ffmpeg in più, quindi resta
     /// opt-in.
     pub measure_vmaf: bool,
     /// Modello passato a `libvmaf` come `model=version=<valore>`.
     pub vmaf_model: String,
     ```
  2. Nel blocco `Default` impostare `measure_vmaf: false` e
     `vmaf_model: "vmaf_v1.0.16_5d0h".to_string()`. È il **modello telefono** di
     VMAF v1, tarato su distanza di visione 5H e sulla minore visibilità degli
     artefatti su schermo piccolo (R16): è la condizione d'uso reale del
     materiale prodotto. Attenzione al nome: `vmaf_v1.0.16` senza suffisso di
     distanza di visione non esiste fra i modelli built-in (R11).
  3. In `validate` aggiungere il controllo del modello riusando il validatore
     esistente `is_safe_ffmpeg_token` (`src/config.rs:167`), nello stesso stile
     dei controlli su `svtav1_params` e `pixel_format`:
     ```rust
     if !is_safe_ffmpeg_token(&self.vmaf_model) {
         return Err("vmaf_model contiene caratteri non ammessi".into());
     }
     ```
     L'alfabeto ammesso (`alfanumerici` più `=:,.-_+/ `) copre
     `vmaf_v1.0.16_5d0h`.
  4. Aggiungere gli override da ambiente accanto agli altri della sezione
     encoding (`src/config.rs:431-455`), seguendo lo stile già presente:
     `AV1WC_MEASURE_VMAF` interpretato come booleano (`parse::<bool>()`, ignorato
     se non valido, come fa `AV1WC_AUTH_ENABLED` a `src/config.rs:388`) e
     `AV1WC_VMAF_MODEL` assegnato direttamente come stringa.
  5. Aggiungere le due chiavi a `config.example.toml` nella sezione
     `[encoding]`, con commento italiano e i valori di default. Necessario
     perché le struct di config usano `deny_unknown_fields`: un file di esempio
     incompleto resta valido, ma va tenuto allineato per non depistare.
- **Unit tests:** in `src/config.rs` (modulo `tests` esistente):
  `la_misura_vmaf_e_disattiva_per_default` — `EncodingParams::default().measure_vmaf == false` e
  `vmaf_model == "vmaf_v1.0.16_5d0h"`;
  `il_modello_vmaf_rifiuta_caratteri_non_ammessi` — `validate()` restituisce
  errore con `vmaf_model = "vmaf; rm -rf /"`;
  `il_modello_vmaf_accetta_i_nomi_built_in` — `validate()` è `Ok` con
  `vmaf_v1.0.16_5d0h` e con `vmaf_v0.6.1neg`.
- **e2e tests:** nessuno in questa sotto-fase.
- **Done:** `cargo test --lib config::` verde con i tre test nuovi,
  `cargo run -- --print-config` mostra le due chiavi nuove, gates verdi +
  `STATE.md` e `resume.md` aggiornati.

### 4.2 Struttura dati `VmafMeasurement` e campo su `JobFile`

- **Model:** `agent:gpt5.6-luna`
- **Assignment:** `agent:gpt5.6-luna` — implementazione.
- **Files:** `src/models.rs:71-90` (`JobFile`), `src/models.rs` (nuova struct
  accanto a `Progress`, `src/models.rs:55-68`).
- **Change:**
  1. Aggiungere la struct, serializzabile come il resto del modello:
     ```rust
     /// Esito della misura di qualità VMAF di un file convertito.
     #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
     pub struct VmafMeasurement {
         /// Media aggregata (`pooled_metrics.vmaf.mean`), assente se la misura è fallita.
         pub score: Option<f64>,
         pub min: Option<f64>,
         pub harmonic_mean: Option<f64>,
         /// Modello effettivamente usato, per rendere il numero confrontabile.
         pub model: String,
         /// Secondi impiegati dalla misura, utili a valutarne il costo.
         pub duration_seconds: f64,
         /// Motivo del fallimento quando la misura non ha prodotto un punteggio.
         pub error: Option<String>,
     }
     ```
  2. Aggiungere a `JobFile`, dopo `error: Option<String>`
     (`src/models.rs:83`):
     ```rust
     /// Assente sui job creati prima dell'introduzione della misura e su
     /// quelli che non l'hanno richiesta.
     #[serde(default)]
     pub vmaf: Option<VmafMeasurement>,
     ```
     Il `#[serde(default)]` è obbligatorio: i file `data_dir/jobs/<id>.json` già
     su disco non hanno il campo (I-4).
  3. Aggiornare i punti di costruzione di `JobFile` (upload:
     `src/api/upload.rs`, cerca la `JobFile {` letterale) aggiungendo
     `vmaf: None`.
- **Unit tests:** in `src/models.rs`:
  **T-VMAF4** `un_job_file_senza_campo_vmaf_resta_leggibile` — deserializza un letterale
  JSON di `JobFile` privo di `vmaf` (copiarne uno minimo con tutti gli altri
  campi obbligatori) e asserisce `vmaf.is_none()`;
  `la_misura_vmaf_viene_serializzata` — serializza un `JobFile` con
  `vmaf: Some(VmafMeasurement { score: Some(95.5), model: "vmaf_v1.0.16_5d0h".into(), .. })`
  e asserisce che il JSON contenga `"score":95.5` e il nome del modello.
- **e2e tests:** nessuno in questa sotto-fase.
- **Done:** `cargo test --lib models::` verde con i due test nuovi, il progetto
  compila senza warning, gates verdi + `STATE.md` e `resume.md` aggiornati.

### 4.3 Modulo `src/ffmpeg/vmaf.rs`: argomenti del filtro e lettura del log

- **Model:** `agent:gpt5.6-luna`
- **Assignment:** `agent:gpt5.6-luna` — implementazione; rileggere in autorevisione la
  stringa di filtro e il parsing, perché è il punto in cui un errore silenzioso
  produrrebbe numeri privi di senso.
- **Files:** nuovo file `src/ffmpeg/vmaf.rs` (la cartella `src/ffmpeg/` esiste
  già e raccoglie l'integrazione ffmpeg: non creare altre cartelle),
  `src/ffmpeg/mod.rs:1-10` (dichiarazione del modulo),
  `src/ffmpeg/encode.rs:218-223` (`total_passes`).
- **Change:**
  1. Dichiarare il modulo in `src/ffmpeg/mod.rs` accanto a `encode`, `probe`,
     `progress`, nello stesso stile (`pub mod vmaf;`).
  2. In `src/ffmpeg/encode.rs` aggiungere accanto a `total_passes` una funzione
     che tiene conto della misura, lasciando `total_passes` invariata per non
     rompere i chiamanti esistenti:
     ```rust
     /// Passaggi mostrati all'utente: la misura VMAF è un passaggio in più.
     pub fn total_passes_with_measure(mode: EncodingMode, measure_vmaf: bool) -> u8 {
         total_passes(mode) + u8::from(measure_vmaf)
     }
     ```
  3. In `src/ffmpeg/vmaf.rs` definire la richiesta e il costruttore di
     argomenti:
     ```rust
     pub struct VmafRequest<'a> {
         /// File convertito da valutare.
         pub distorted: &'a Path,
         /// Sorgente usato come riferimento.
         pub reference: &'a Path,
         /// Percorso del log JSON prodotto da libvmaf.
         pub log_json: &'a Path,
         pub model: &'a str,
         pub pixel_format: &'a str,
         pub threads: Option<u32>,
         /// Geometria del distorto, presente solo quando differisce da quella
         /// del sorgente: libvmaf confronta solo immagini della stessa
         /// dimensione.
         pub scale_to: Option<(u32, u32)>,
         pub progress_interval: f64,
     }

     pub fn build_vmaf_args(request: &VmafRequest) -> Vec<OsString>
     ```
     Gli argomenti prodotti, nell'ordine:
     `-nostdin`, `-hide_banner`, `-loglevel`, `error`, `-nostats`,
     `-stats_period`, `<progress_interval>`, `-progress`, `pipe:1`, `-y`,
     `-i`, `<distorted>`, `-i`, `<reference>`, `-lavfi`, `<filtro>`, `-an`,
     `-f`, `null`, `/dev/null` (su Windows `NUL`, come già fa
     `build_two_pass_args`, `src/ffmpeg/encode.rs:138-148`).
     Il filtro è una sola stringa, costruita così (nessuno spazio, nessun
     ritorno a capo):
     ```
     [0:v]settb=AVTB,setpts=PTS-STARTPTS,format=<pixel_format>[dist];[1:v]settb=AVTB,setpts=PTS-STARTPTS<,scale=w=<w>:h=<h>:flags=bicubic:reset_sar=1 se scale_to è presente>,format=<pixel_format>[ref];[dist][ref]libvmaf=model=version=<model>:log_fmt=json:log_path=<log_json><:n_threads=<threads> se presente>
     ```
     Motivi, da riportare come commenti nel codice:
     - `settb=AVTB,setpts=PTS-STARTPTS` allinea le basi tempi dei due input,
       che arrivano da contenitori diversi (MP4 e sorgente qualunque);
     - `format=<pixel_format>` porta entrambi i rami a un formato accettato dal
       filtro (`yuv420p/422p/444p` a 8/10/12/16 bit, R12) ed evita che i due
       input differiscano per profondità;
     - la riscalatura del riferimento serve solo quando la conversione ha
       ridimensionato l'immagine; `reset_sar=1` replica quanto fa il filtro di
       codifica (`src/ffmpeg/encode.rs:47`);
     - `log_fmt=json` è necessario perché il default del filtro è `xml`
       (R12).
  4. Aggiungere la lettura del log:
     ```rust
     #[derive(Debug, Clone, PartialEq)]
     pub struct VmafScores {
         pub mean: Option<f64>,
         pub min: Option<f64>,
         pub harmonic_mean: Option<f64>,
     }

     /// Legge le metriche aggregate dal log JSON di libvmaf.
     pub fn parse_vmaf_log(contenuto: &str) -> anyhow::Result<VmafScores>
     ```
     Percorso delle chiavi: `pooled_metrics` → `vmaf` → `mean`, `min`,
     `harmonic_mean` (R14). Se `pooled_metrics.vmaf` è assente, restituire un
     errore con `anyhow::bail!` e messaggio italiano
     (`"log VMAF senza metriche aggregate"`); i singoli campi mancanti restano
     `None`. Usare `serde_json`, già dipendenza del progetto
     (`src/ffmpeg/probe.rs:54`).
- **Unit tests:** in `src/ffmpeg/vmaf.rs` (modulo `tests` in fondo, come negli
  altri file `src/ffmpeg/*`):
  **T-VMAF2** `gli_argomenti_vmaf_contengono_i_due_input_e_il_filtro` — l'elenco contiene
  `-lavfi`, due `-i`, `-f null` e termina con il file nullo;
  `il_riferimento_non_viene_riscalato_se_la_geometria_coincide` — con
  `scale_to: None` il filtro non contiene `scale=`;
  `il_riferimento_viene_riscalato_alla_geometria_del_distorto` — con
  `scale_to: Some((1280, 720))` il filtro contiene
  `scale=w=1280:h=720:flags=bicubic:reset_sar=1`;
  `il_modello_e_il_log_finiscono_nel_filtro_libvmaf` — il filtro contiene
  `libvmaf=model=version=vmaf_v1.0.16_5d0h` e `log_fmt=json` e il percorso del
  log;
  `i_thread_vmaf_sono_opzionali` — con `threads: None` il filtro non contiene
  `n_threads`, con `Some(4)` contiene `n_threads=4`;
  **T-VMAF3** `il_log_json_di_vmaf_viene_interpretato` — su un letterale
  `{"pooled_metrics":{"vmaf":{"min":80.1,"max":99.0,"mean":95.5,"harmonic_mean":95.0}}}`
  restituisce `mean == Some(95.5)`, `min == Some(80.1)`,
  `harmonic_mean == Some(95.0)`;
  `un_log_json_senza_metriche_da_errore` — su `{"frames":[]}` restituisce
  errore.
  In `src/ffmpeg/encode.rs`: `i_passaggi_includono_la_misura_vmaf` —
  `total_passes_with_measure(EncodingMode::Crf, true) == 2` e
  `total_passes_with_measure(EncodingMode::TargetSize, true) == 3`, con
  `false` i valori restano 1 e 2.
- **e2e tests:** nessuno in questa sotto-fase (la misura reale è T-VMAF1, fase 5).
- **Done:** `cargo test --lib ffmpeg::` verde con gli otto test nuovi, gates
  verdi + `STATE.md` e `resume.md` aggiornati.

### 4.4 Innesto della misura nel worker

> **Cambio di comportamento:** con `measure_vmaf=true` il job esegue un
> passaggio ffmpeg in più e `Progress.total_passes` vale un'unità in più. Con
> `measure_vmaf=false` nulla cambia.

- **Model:** `agent:gpt5.6-luna`
- **Assignment:** `agent:gpt5.6-luna` — implementazione; rileggere in autorevisione
  ordine delle operazioni, cancellazione e tolleranza ai fallimenti.
- **Files:** `src/worker.rs:20-24` (import), `src/worker.rs:236-240`
  (temporanei e guardia), `src/worker.rs:244` (calcolo dei passaggi),
  `src/worker.rs:317-348` (dopo il rename finale, prima di `drop(guard)` e di
  `finish_file`), `src/worker.rs:433-463` (`finish_file`),
  `src/worker.rs:465-503` (`mark_cancelled`, `fail_file`: chiamanti di
  `finish_file`).
- **Change:**
  1. Import: aggiungere `total_passes_with_measure` da `crate::ffmpeg::encode` e
     `crate::ffmpeg::vmaf::{build_vmaf_args, parse_vmaf_log, VmafRequest}`, più
     `crate::models::VmafMeasurement`.
  2. Aggiungere il log JSON ai temporanei: costruire
     `let vmaf_json = temp_dir.join(format!(".av1-{file_id}.vmaf.json"));` e
     includerlo nel vettore passato a `TempFiles::new`
     (`src/worker.rs:239`), così viene rimosso anche in caso di errore o
     annullamento.
  3. Passaggi mostrati: sostituire `let passes = total_passes(params.mode);`
     con il numero di passaggi di codifica (invariato, usato dal ciclo `for
     pass in 1..=passes`) e un secondo valore per l'avanzamento:
     ```rust
     let passes = total_passes(params.mode);
     let passi_totali = total_passes_with_measure(params.mode, params.measure_vmaf);
     ```
     e usare `passi_totali` come ultimo argomento di `ProgressParser::new`
     (`src/worker.rs:270`), lasciando `pass` come indice del passaggio di
     codifica corrente.
  4. Dopo il rename finale e il calcolo di `final_size`
     (`src/worker.rs:317-323`), **prima** di `drop(guard)` e di `finish_file`,
     inserire la misura, solo se `params.measure_vmaf`:
     - `log_line(state, job_id, file_id, "Misura VMAF dell'output rispetto al sorgente").await;`
     - `probe` sull'output (`probe(&state.config.ffmpeg.ffprobe_bin, &output_path)`)
       per conoscerne geometria e durata; se fallisce, non far fallire il file:
       registrare `VmafMeasurement { error: Some(...), .. }` e proseguire.
     - `scale_to`: `Some((larghezza_output, altezza_output))` solo se la coppia
       differisce da quella del sorgente ricavata da `info`
       (`src/worker.rs:215`); se una delle quattro dimensioni è assente, usare
       `None` e annotarlo nel log.
     - costruire `VmafRequest` con `distorted = &output_path`,
       `reference = &input_path`, `log_json = &vmaf_json`,
       `model = &params.vmaf_model`, `pixel_format = &params.pixel_format`,
       `threads = params.cpus`, `progress_interval = params.progress_interval`.
     - eseguire con l'infrastruttura esistente:
       `run_progress_run(state, handle, file_id, &args, ProgressParser::new(info.duration_seconds, info.estimated_frames, passi_totali, passi_totali), &mut cancel).await?`
       così l'avanzamento della misura arriva al frontend come per i passaggi di
       codifica e la cancellazione resta gestita dallo stesso canale (I-6).
     - misurare il tempo con `std::time::Instant` prima e dopo.
     - esito:
       - `RunOutcome::Completed` → leggere `vmaf_json` con
         `tokio::fs::read_to_string`, passarlo a `parse_vmaf_log` e riempire
         `VmafMeasurement { score: mean, min, harmonic_mean, model, duration_seconds, error: None }`;
         emettere una riga di log con il punteggio
         (`format!("VMAF {:.2} (modello {})", ...)`).
       - `RunOutcome::Cancelled` → `VmafMeasurement` con
         `error: Some("misura interrotta".into())`, **senza** chiamare
         `mark_cancelled`: l'output è già pubblicato e il file resta
         `completed` (I-1, I-5).
       - errore di esecuzione o di parsing → `VmafMeasurement` con
         `error: Some(messaggio)`, riga di log che segnala il fallimento, e
         prosecuzione normale (I-5). Non usare `?` su questi rami.
  5. `finish_file` (`src/worker.rs:433`): aggiungere il parametro
     `vmaf: Option<VmafMeasurement>` e assegnarlo al file dentro la mutazione
     dello stato, accanto a `output_path` e `output_size_bytes`. Aggiornare i
     chiamanti: il ramo di successo passa la misura calcolata (o `None`),
     `mark_cancelled` e `fail_file` passano `None`.
  6. Non toccare l'ordine di rename e pubblicazione: la misura legge l'output
     definitivo e non lo riscrive (I-1).
- **Unit tests:** in `src/worker.rs` (modulo `tests` esistente,
  `src/worker.rs:600-660`): `un_job_senza_misura_non_cambia_i_passaggi` —
  con `measure_vmaf: false`, `total_passes_with_measure` restituisce lo stesso
  valore di `total_passes` per entrambe le modalità. La logica di misura vera è
  coperta dagli e2e della fase 5, perché richiede ffmpeg.
- **e2e tests:** nessuno qui; T-VMAF1, T-VMAF6 e T-VMAF7 sono in fase 5.
- **Done:** `cargo test --lib` verde, `cargo clippy --all-targets -- -D warnings`
  senza avvisi, `cargo test --test e2e_conversion` ancora interamente verde (la
  misura è disattiva per default, quindi nessun test esistente deve cambiare
  comportamento) + `STATE.md` e `resume.md` aggiornati.

### 4.5 Update README.md

- **Model:** `agent:gpt5.6-luna`
- **Assignment:** `agent:gpt5.6-luna` — documentazione.
- **Files:** `README.md` (sezione dei parametri di conversione e sezione della
  configurazione/variabili d'ambiente; sezione limiti o note se presente).
- **Change:** documentare solo ciò che questa fase rende usabile, cioè i due
  parametri e il campo restituito dall'API:
  - `measure_vmaf` (default `false`): cosa fa, che costa un passaggio ffmpeg in
    più sul file già convertito, che non modifica l'output.
  - `vmaf_model` (default `vmaf_v1.0.16_5d0h`, il modello pensato per la visione
    su cellulare): a cosa serve ed elenco dei nomi utilizzabili
    (`vmaf_v1.0.16_5d0h` per telefono, `vmaf_v1.0.16_3d0h` per televisore a
    1080p, `vmaf_v1.0.16_3d0h_2160` e `vmaf_v1.0.16_1d5h_2160` per 4K, le quattro
    varianti `_hfr_` per alto frame rate, e i modelli precedenti
    `vmaf_v0.6.1`, `vmaf_v0.6.1neg`, `vmaf_b_v0.6.3`, `vmaf_4k_v0.6.1`,
    `vmaf_4k_v0.6.1neg`). Indicare che il modello telefono restituisce punteggi
    più alti a parità di file, perché su schermo piccolo gli artefatti si vedono
    meno, e che i punteggi di modelli diversi non sono confrontabili fra loro.
  - che il confronto avviene alla risoluzione di uscita, quindi il punteggio
    misura la perdita da compressione e non quella dovuta al ridimensionamento.
  - variabili d'ambiente `AV1WC_MEASURE_VMAF` e `AV1WC_VMAF_MODEL`.
  - esempio realistico di creazione job con la misura attiva e forma del
    risultato (`vmaf.score`, `vmaf.min`, `vmaf.harmonic_mean`, `vmaf.model`,
    `vmaf.duration_seconds`, `vmaf.error`).
  - limiti noti: la misura raddoppia circa il tempo di elaborazione del file;
    un suo fallimento non fa fallire la conversione e compare in `vmaf.error`;
    interrompere il job durante la misura lascia l'output valido senza
    punteggio; il numero è confrontabile solo fra file misurati con lo stesso
    modello.
  Preservare struttura, tono e lingua; modificare, non riscrivere. Nessun nome
  di modulo o funzione, nessun riferimento al piano.
- **Unit tests:** nessuno (documentazione).
- **e2e tests:** nessuno — l'esempio documentato va eseguito almeno una volta.
- **Done:** un utente attiva la misura e interpreta il risultato leggendo solo
  il README; nessun dettaglio implementativo; gates verdi; `STATE.md` e
  `resume.md` aggiornati.

---

## Phase gates

- **Fmt:** `cargo fmt --all -- --check`
- **Lint:** `cargo clippy --all-targets -- -D warnings`
- **Test subset:** `cargo test --lib`, `cargo test --test api`,
  `cargo test --test e2e_conversion`
- **Regression guard:** tutti gli e2e esistenti restano verdi con la misura
  disattiva; I-4 (JSON di job senza `vmaf` leggibili).
- **README:** aggiornato con i due parametri, le variabili d'ambiente, il campo
  `vmaf` e i limiti noti.

## Phase done criterion

Con `measure_vmaf=false` il comportamento è identico a prima e tutti i test
esistenti passano; con `measure_vmaf=true` il worker esegue un passaggio
aggiuntivo che scrive `JobFile.vmaf` con punteggio e modello, non modifica
l'MP4 pubblicato, non fa fallire il file in caso di errore e resta
interrompibile. Il README documenta i parametri e i limiti.
