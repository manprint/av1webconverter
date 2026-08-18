# Phase 6 — Modalità Analisi VMAF + Qualità Costante: backend

> **Intent:** aggiungere la terza modalità di codifica, che prima di convertire
> sonda spezzoni casuali del sorgente a CRF crescenti, misura il VMAF di ogni
> sonda e sceglie il CRF più alto che resta sopra la soglia di qualità.
> **Shippable alone?** sì — la modalità va scelta esplicitamente; con `crf` o
> `target_size` non viene eseguita nessuna sonda (I-7).
> **Preconditions:** fase 5 `DONE` (`src/ffmpeg/vmaf.rs` esiste con
> `build_vmaf_args` e `parse_vmaf_log`, e la misura è provata end-to-end).

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

Regole di progetto: italiano per identificatori, commenti, messaggi di log e
testi visibili; i commenti spiegano il *perché*; nessun emoji; nessuna nuova
cartella e nessuna nuova dipendenza in `Cargo.toml` (la casualità viene
dall'UUID del file, D24).

Contesto necessario a chi esegue, senza aprire altri file: la modalità parte dal
`crf` impostato dall'utente (per esempio 40), prova `crf+2`, `crf+4`, … per
`crf_test_steps` valori (con 3 step: 42, 44, 46), su `vmaf_segments` spezzoni
casuali da 30 secondi (con 3: tre spezzoni), quindi `segmenti × step` sonde per
file. Il punteggio di un CRF è la media dei suoi segmenti. Viene scelto il CRF
più alto con media `>= vmaf_target` (default 93); se nessuno la raggiunge si usa
il CRF dell'utente; se la raggiungono tutti si usa il più alto provato. Il
modello di misura di default è `vmaf_v1.0.16_5d0h`, cioè il modello telefono.

---

## Sub-phases

### 6.1 Terza modalità e parametri di configurazione

- **Model:** `agent:gpt5.6-luna`
- **Assignment:** `agent:gpt5.6-luna` — implementazione.
- **Files:** `src/config.rs:13-23` (`EncodingMode`), `src/config.rs:25-51`
  (campi di `EncodingParams`), `src/config.rs:53-75` (`Default`),
  `src/config.rs:77-110` (`validate`), `src/config.rs:137-145`
  (`effective_svtav1_params`), `src/config.rs:431-460` (override da ambiente),
  `src/ffmpeg/encode.rs:218-223` (`total_passes`), `config.example.toml`.
- **Change:**
  1. Aggiungere la variante all'enum `EncodingMode`, con commento che spiega il
     perché:
     ```rust
     /// Codifica a qualità costante preceduta da un'analisi VMAF che sceglie il
     /// CRF: si parte dal CRF impostato e si cerca il valore più alto che resta
     /// sopra la soglia di qualità.
     VmafAnalysis,
     ```
     L'enum ha già `#[serde(rename_all = "snake_case")]`, quindi il valore sul
     filo è `vmaf_analysis`.
  2. Aggiungere a `EncodingParams` tre campi:
     ```rust
     /// Quanti spezzoni del sorgente vengono sondati in modalità di analisi.
     pub vmaf_segments: u32,
     /// Quanti CRF provare oltre a quello impostato, a passi di +2.
     pub crf_test_steps: u32,
     /// Qualità media minima che il CRF scelto deve garantire.
     pub vmaf_target: f64,
     ```
  3. Default: `vmaf_segments: 3`, `crf_test_steps: 3`, `vmaf_target: 93.0`.
  4. In `validate` aggiungere, con messaggi in italiano nello stile degli altri:
     - `vmaf_segments` in `1..=10`, altrimenti errore;
     - `crf_test_steps` in `1..=10`, altrimenti errore;
     - `vmaf_target` in `0.0..=100.0`, altrimenti errore.
     Non validare `crf + 2 * crf_test_steps <= 63`: gli step fuori scala
     vengono scartati a runtime con una riga di log (§ 6.2), perché rifiutare il
     job costringerebbe l'utente a calcolare a mano il limite.
  5. `effective_svtav1_params` (`src/config.rs:139`): il `match` deve trattare
     `VmafAnalysis` come `Crf`, cioè restituire i parametri così come sono senza
     forzare `tune=0`. Solo `TargetSize` continua a forzarlo, perché è l'unica
     modalità a due passaggi VBR (D28). Aggiornare anche il commento sopra la
     funzione.
  6. `total_passes` (`src/ffmpeg/encode.rs:218`): `VmafAnalysis` restituisce `1`,
     come `Crf`: i passaggi delle sonde non sono passaggi di codifica del file e
     vengono contati a parte (§ 6.4).
  7. Override da ambiente accanto agli altri: `AV1WC_VMAF_SEGMENTS` e
     `AV1WC_CRF_TEST_STEPS` con `parse::<u32>()`, `AV1WC_VMAF_TARGET` con
     `parse::<f64>()`, ognuno ignorato se non interpretabile, come già fa
     `AV1WC_PROGRESS_INTERVAL` (`src/config.rs:452`).
  8. Aggiungere le tre chiavi a `config.example.toml` nella sezione
     `[encoding]`, con commento in italiano, e citare la nuova modalità nel
     commento della chiave `mode`.
- **Unit tests:** in `src/config.rs`:
  **T-AN6** `la_modalita_di_analisi_non_forza_tune_zero` —
  `EncodingParams { mode: VmafAnalysis, svtav1_params: "tune=5", .. }` produce
  `effective_svtav1_params() == "tune=5"`, mentre con `TargetSize` resta
  `"tune=0"`;
  `i_parametri_di_analisi_hanno_i_default_attesi` — `vmaf_segments == 3`,
  `crf_test_steps == 3`, `vmaf_target == 93.0`;
  `i_parametri_di_analisi_fuori_intervallo_sono_rifiutati` — `validate()` dà
  errore con `vmaf_segments = 0`, con `crf_test_steps = 11` e con
  `vmaf_target = 150.0`.
  In `src/ffmpeg/encode.rs`: **T-AN6** (seconda parte)
  `la_modalita_di_analisi_ha_un_solo_passaggio_di_codifica` —
  `total_passes(EncodingMode::VmafAnalysis) == 1`.
- **e2e tests:** nessuno in questa sotto-fase.
- **Done:** `cargo test --lib` verde, `cargo run -- --print-config` mostra le tre
  chiavi nuove, nessun `match` su `EncodingMode` resta non esaustivo (il
  compilatore lo garantisce), gates verdi + `STATE.md` e `resume.md` aggiornati.

### 6.2 Segmentazione e griglia dei CRF

- **Model:** `agent:gpt5.6-luna`
- **Assignment:** `agent:gpt5.6-luna` — implementazione; rileggere in autorevisione la
  segmentazione, perché è la parte che determina quanto il risultato
  rappresenta il file.
- **Files:** `src/ffmpeg/vmaf.rs` (funzioni pure aggiunte al modulo creato in
  fase 4; non creare nuovi file né cartelle).
- **Change:**
  1. Aggiungere la costante e il tipo del segmento:
     ```rust
     /// Durata fissa di ogni spezzone sondato. Non configurabile: la leva
     /// offerta all'utente è il numero di segmenti.
     pub const DURATA_SEGMENTO_SECONDI: f64 = 30.0;

     #[derive(Debug, Clone, Copy, PartialEq)]
     pub struct SegmentoAnalisi {
         pub indice: u32,
         pub inizio_secondi: f64,
         pub durata_secondi: f64,
     }
     ```
  2. Aggiungere la scelta dei segmenti:
     ```rust
     /// Spezzoni da sondare: casuali, non sovrapposti, con il primo e l'ultimo
     /// 5% della durata esclusi perché lì stanno sigle, nero e titoli di coda.
     /// L'entropia arriva dai byte dell'UUID del file, così non serve una
     /// dipendenza per il caso e la stessa analisi è ripetibile.
     pub fn segmenti_da_analizzare(
         durata_secondi: f64,
         richiesti: u32,
         entropia: &[u8; 16],
     ) -> Vec<SegmentoAnalisi>
     ```
     Algoritmo, da implementare esattamente così:
     - se `durata_secondi <= DURATA_SEGMENTO_SECONDI`, restituire un solo
       segmento `{ indice: 0, inizio_secondi: 0.0, durata_secondi: durata_secondi }`
       (file corto: si analizza tutto);
     - `inizio_utile = durata * 0.05`, `fine_utile = durata * 0.95`,
       `finestra = fine_utile - inizio_utile`;
     - se `finestra < DURATA_SEGMENTO_SECONDI`, restituire un solo segmento che
       parte da `inizio_utile` e dura `min(DURATA_SEGMENTO_SECONDI, durata - inizio_utile)`;
     - `massimo = (finestra / DURATA_SEGMENTO_SECONDI).floor() as u32`;
       `n = richiesti.min(massimo).max(1)`;
     - dividere `finestra` in `n` blocchi uguali di ampiezza `blocco`; per il
       blocco `i` calcolare `margine = (blocco - DURATA_SEGMENTO_SECONDI).max(0.0)`
       e `offset = margine * (entropia[i % 16] as f64 / 255.0)`; l'inizio è
       `inizio_utile + i as f64 * blocco + offset`. Un blocco per segmento
       garantisce che non si sovrappongano;
     - la durata di ogni segmento è `DURATA_SEGMENTO_SECONDI`, troncata se
       superasse `fine_utile`.
  3. Aggiungere la griglia dei CRF:
     ```rust
     /// CRF candidati: `crf+2`, `crf+4`, … per `step` valori. I valori sopra 63
     /// vengono scartati, perché è il massimo accettato da SVT-AV1.
     pub fn crf_da_testare(crf_di_partenza: u8, step: u32) -> Vec<u8>
     ```
- **Unit tests:** in `src/ffmpeg/vmaf.rs`:
  **T-AN1** `i_crf_da_testare_crescono_di_due` — `crf_da_testare(40, 3) == vec![42, 44, 46]`;
  `i_crf_oltre_il_massimo_vengono_scartati` — `crf_da_testare(60, 5) == vec![62]`
  e `crf_da_testare(63, 2)` è vuoto;
  **T-AN2** `i_segmenti_escludono_testa_e_coda` — su durata 600 s e 3 richiesti:
  tre segmenti, il primo non inizia prima di `30.0` (5% di 600) e l'ultimo
  termina entro `570.0`;
  `i_segmenti_non_si_sovrappongono` — su durata 600 s e 3 richiesti, ordinati
  per inizio, ogni `inizio` è `>=` fine del precedente;
  `il_numero_di_segmenti_si_riduce_se_la_durata_non_basta` — durata 100 s con 5
  richiesti restituisce al massimo `floor(90/30) = 3` segmenti;
  `un_file_corto_diventa_un_unico_segmento` — durata 10 s restituisce un solo
  segmento `{ 0, 0.0, 10.0 }`;
  `gli_stessi_byte_danno_gli_stessi_segmenti` — due chiamate con la stessa
  entropia danno lo stesso risultato, due entropie diverse danno inizi diversi.
- **e2e tests:** nessuno in questa sotto-fase.
- **Done:** `cargo test --lib ffmpeg::vmaf` verde con i sei test nuovi, gates
  verdi + `STATE.md` e `resume.md` aggiornati.

### 6.3 Argomenti delle sonde, aggregazione e criterio di scelta

- **Model:** `agent:gpt5.6-luna`
- **Assignment:** `agent:gpt5.6-luna` — implementazione; rileggere in autorevisione il
  criterio di scelta, perché è la regola che decide la qualità del prodotto.
- **Files:** `src/ffmpeg/vmaf.rs`, `src/ffmpeg/encode.rs:72-91`
  (`video_encoder_args`, da rendere `pub(crate)`), `src/models.rs` (tipi
  persistiti).
- **Change:**
  1. Rendere `video_encoder_args` visibile al modulo `vmaf`
     (`pub(crate) fn video_encoder_args`), così le sonde usano *gli stessi*
     filtro di scala, preset, `svtav1-params` e `pix_fmt` della codifica finale
     (D22): è ciò che rende il CRF trovato trasferibile.
  2. Aggiungere il costruttore degli argomenti della sonda:
     ```rust
     pub struct SondaRequest<'a> {
         pub params: &'a EncodingParams,
         pub input: &'a Path,
         pub output_mkv: &'a Path,
         pub segmento: &'a SegmentoAnalisi,
         pub crf: u8,
     }

     /// Codifica di prova di un solo spezzone: nessun audio, contenitore
     /// Matroska, stessi parametri video della codifica finale.
     pub fn build_probe_args(request: &SondaRequest) -> Vec<OsString>
     ```
     Argomenti prodotti, nell'ordine: `-nostdin`, `-hide_banner`, `-loglevel`,
     `error`, `-nostats`, `-stats_period`, `<progress_interval>`, `-progress`,
     `pipe:1`, `-y`, `-ss`, `<inizio_secondi>`, `-t`, `<durata_secondi>`, `-i`,
     `<input>`, `-map`, `0:v:0`, `-an`, poi `video_encoder_args(params)`, poi
     `-b:v`, `0`, `-crf`, `<crf>`, `-f`, `matroska`, `<output_mkv>`.
     `-ss` prima di `-i` è ricerca sull'input: veloce, e allinea la sonda alla
     misura, che userà lo stesso valore sullo stesso file.
  3. Estendere `VmafRequest` (creata in fase 4) con il taglio del riferimento,
     necessario per misurare una sonda contro il solo spezzone corrispondente:
     ```rust
     /// Quando presenti, vengono emessi come `-ss`/`-t` prima del secondo `-i`,
     /// così il riferimento parte dallo stesso punto della sonda.
     pub riferimento_inizio: Option<f64>,
     pub riferimento_durata: Option<f64>,
     ```
     I chiamanti della fase 4 passano `None` per entrambi.
  4. Aggiungere i tipi persistiti in `src/models.rs`, accanto a
     `VmafMeasurement`:
     ```rust
     #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
     #[serde(rename_all = "snake_case")]
     pub enum MotivoScelta {
         /// Almeno un CRF sopra soglia e almeno uno sotto: scelta nel mezzo.
         SopraSoglia,
         /// Tutti i CRF provati restano sopra soglia: scelto il più alto.
         TuttiSopraSoglia,
         /// Nessun CRF provato raggiunge la soglia: si usa quello impostato.
         NessunoSopraSoglia,
         /// Nessuna misura valida: si usa il CRF impostato.
         AnalisiNonEseguibile,
     }

     #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
     pub struct RisultatoCrf {
         pub crf: u8,
         pub vmaf_medio: Option<f64>,
         pub bitrate_medio_bps: Option<u64>,
         pub vmaf_per_segmento: Vec<Option<f64>>,
     }

     #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
     pub struct SegmentoAnalisiPersistito {
         pub indice: u32,
         pub inizio_secondi: f64,
         pub durata_secondi: f64,
     }

     #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
     pub struct VmafAnalysis {
         pub crf_scelto: u8,
         pub crf_di_partenza: u8,
         pub target: f64,
         pub modello: String,
         pub motivo: MotivoScelta,
         pub segmenti: Vec<SegmentoAnalisiPersistito>,
         pub risultati: Vec<RisultatoCrf>,
         pub duration_seconds: f64,
         pub error: Option<String>,
     }
     ```
     e il campo su `JobFile`, dopo `vmaf`:
     ```rust
     /// Presente solo per i file convertiti in modalità di analisi.
     #[serde(default)]
     pub vmaf_analysis: Option<VmafAnalysis>,
     ```
     Aggiornare i punti di costruzione di `JobFile` con `vmaf_analysis: None`.
  5. Aggiungere in `src/ffmpeg/vmaf.rs` il criterio di scelta:
     ```rust
     /// Applica il criterio: fra i CRF con misura valida si prende il più alto
     /// con VMAF medio >= target. Senza candidati si torna al CRF impostato
     /// dall'utente, che è la scelta prudente.
     pub fn scegli_crf(
         risultati: &[RisultatoCrf],
         target: f64,
         crf_di_partenza: u8,
     ) -> (u8, MotivoScelta)
     ```
     Regole esatte:
     - `validi` = risultati con `vmaf_medio` presente; se vuoto →
       `(crf_di_partenza, AnalisiNonEseguibile)`;
     - `candidati` = validi con `vmaf_medio >= target`; se vuoto →
       `(crf_di_partenza, NessunoSopraSoglia)`;
     - altrimenti `crf` = massimo `crf` fra i candidati; il motivo è
       `TuttiSopraSoglia` se `candidati.len() == validi.len()`, altrimenti
       `SopraSoglia`.
  6. Aggiungere la formattazione del riepilogo per il log, in modo che il
     motivo della scelta sia leggibile da chi apre i log del job:
     ```rust
     /// Righe di riepilogo dell'analisi, una per CRF più la conclusione.
     pub fn righe_riepilogo(analisi: &VmafAnalysis) -> Vec<String>
     ```
     Formato richiesto (una riga per CRF, poi la conclusione):
     ```
     Analisi VMAF - CRF 42: VMAF medio 96.80 su 3 segmenti, 2.10 Mb/s
     Analisi VMAF - CRF 44: VMAF medio 95.10 su 3 segmenti, 1.72 Mb/s
     Analisi VMAF - CRF 46: VMAF medio 93.40 su 3 segmenti, 1.41 Mb/s
     Analisi VMAF - CRF scelto: 46 (VMAF medio 93.40, target 93.00, modello vmaf_v1.0.16_5d0h)
     Analisi VMAF - tutti i CRF provati restano sopra il target: aumentare "CRF Test Steps" per spingere oltre
     ```
     L'ultima riga compare solo con motivo `TuttiSopraSoglia`; con
     `NessunoSopraSoglia` la conclusione dichiara che si usa il CRF impostato
     dall'utente, con `AnalisiNonEseguibile` che l'analisi non ha prodotto
     misure valide.
- **Unit tests:** in `src/ffmpeg/vmaf.rs`:
  **T-AN4** `gli_argomenti_della_sonda_tagliano_lo_spezzone` — l'elenco contiene
  `-ss` con l'inizio del segmento, `-t` con la durata, `-an`, `-crf` con il CRF
  della sonda e termina con il percorso Matroska; contiene anche il filtro di
  scala quando `scale_height != 0`;
  **T-AN5** `gli_argomenti_di_misura_tagliano_il_riferimento` — con
  `riferimento_inizio: Some(120.0)` e `riferimento_durata: Some(30.0)` gli
  argomenti hanno `-ss 120` e `-t 30` **prima** del secondo `-i` e non prima del
  primo;
  **T-AN3** `il_criterio_prende_il_crf_piu_alto_sopra_soglia` — su risultati
  `[(42, 96.8), (44, 95.1), (46, 91.0)]` con target 93 restituisce
  `(44, SopraSoglia)`;
  `il_criterio_segnala_quando_tutti_superano_la_soglia` — `[(42, 96.8), (44, 95.1)]`
  con target 93 restituisce `(44, TuttiSopraSoglia)`;
  `il_criterio_ripiega_sul_crf_impostato` — `[(42, 90.0)]` con target 93 e CRF
  impostato 40 restituisce `(40, NessunoSopraSoglia)`; con risultati senza
  misure valide restituisce `(40, AnalisiNonEseguibile)`;
  `il_riepilogo_dichiara_il_crf_scelto` — `righe_riepilogo` contiene una riga
  per ogni CRF e una riga che inizia con `Analisi VMAF - CRF scelto:`.
  In `src/models.rs`: **T-AN8** `un_job_file_senza_analisi_resta_leggibile` —
  deserializza un `JobFile` senza `vmaf_analysis` e asserisce
  `vmaf_analysis.is_none()` (I-4).
- **e2e tests:** nessuno in questa sotto-fase.
- **Done:** `cargo test --lib` verde con i sette test nuovi, gates verdi +
  `STATE.md` e `resume.md` aggiornati.

### 6.4 Orchestrazione dell'analisi nel worker

> **Cambio di comportamento:** solo con `mode = vmaf_analysis`. Il file esegue
> `segmenti × step` codifiche brevi più altrettante misure prima della codifica
> vera, e `Progress.total_passes` cresce di conseguenza. Con le altre due
> modalità nessuna chiamata a ffmpeg in più (I-7).

- **Model:** `agent:gpt5.6-luna`
- **Assignment:** `agent:gpt5.6-luna` — implementazione; rileggere in autorevisione
  ordine delle operazioni, cancellazione, tolleranza ai fallimenti e conteggio
  dei passaggi.
- **Files:** `src/worker.rs:20-24` (import), `src/worker.rs:236-244`
  (temporanei, guardia e conteggio dei passaggi), `src/worker.rs:245-268`
  (scelta degli argomenti per passaggio), `src/worker.rs:433-463`
  (`finish_file`).
- **Change:**
  1. Import: `build_probe_args`, `crf_da_testare`, `righe_riepilogo`,
     `scegli_crf`, `segmenti_da_analizzare`, `SondaRequest`,
     `DURATA_SEGMENTO_SECONDI` da `crate::ffmpeg::vmaf`; `MotivoScelta`,
     `RisultatoCrf`, `SegmentoAnalisiPersistito`, `VmafAnalysis` da
     `crate::models`.
  2. Temporanei: aggiungere al vettore passato a `TempFiles::new`
     (`src/worker.rs:239`) due percorsi riusati da ogni sonda, così non
     crescono con il numero di sonde e vengono comunque rimossi in caso di
     errore o annullamento:
     `.av1-{file_id}.sonda.mkv` e `.av1-{file_id}.sonda.vmaf.json`.
  3. Conteggio dei passaggi mostrati, prima del ciclo di codifica:
     ```rust
     let sonde = if params.mode == EncodingMode::VmafAnalysis {
         segmenti.len() as u32 * crf_candidati.len() as u32
     } else { 0 };
     // Ogni sonda è una codifica più una misura; il campo è u8, quindi si satura.
     let passi_totali = u8::try_from(
         sonde as u32 * 2 + total_passes(params.mode) as u32
             + u32::from(params.measure_vmaf),
     ).unwrap_or(u8::MAX);
     ```
     `passi_totali` è l'ultimo argomento di `ProgressParser::new`; l'indice del
     passaggio corrente parte da 1 e cresce a ogni esecuzione di ffmpeg.
  4. Inserire la fase di analisi **prima** del ciclo `for pass in 1..=passes`
     (`src/worker.rs:245`), eseguita solo se
     `params.mode == EncodingMode::VmafAnalysis`:
     - `let segmenti = segmenti_da_analizzare(info.duration_seconds, params.vmaf_segments, file_id.as_bytes());`
     - `let crf_candidati = crf_da_testare(params.crf, params.crf_test_steps);`
     - riga di log iniziale: numero di segmenti effettivi con i loro istanti di
       inizio, elenco dei CRF da provare, target e modello. Se il numero di
       segmenti è minore di `params.vmaf_segments`, dirlo esplicitamente
       ("durata insufficiente: ridotti a N segmenti", D23). Se
       `crf_candidati` è vuoto, dirlo e saltare l'analisi con motivo
       `AnalisiNonEseguibile`;
     - doppio ciclo, CRF esterno e segmenti interno; per ogni coppia:
       1. `build_probe_args` verso `.av1-{file_id}.sonda.mkv` (sovrascritto ogni
          volta) ed esecuzione con `run_progress_run`, passando un
          `ProgressParser::new(segmento.durata_secondi, 0, passo_corrente, passi_totali)`;
       2. lettura della dimensione della sonda per il bitrate
          (`dimensione * 8.0 / durata_segmento`);
       3. `build_vmaf_args` con `distorted` = la sonda, `reference` = il
          sorgente, `riferimento_inizio` e `riferimento_durata` = quelli del
          segmento, `scale_to` = la geometria della sonda quando differisce da
          quella del sorgente, `log_json` = `.av1-{file_id}.sonda.vmaf.json`,
          `model` = `params.vmaf_model`, `threads` = `params.cpus`; esecuzione
          con `run_progress_run` e passaggio successivo;
       4. `parse_vmaf_log` sul JSON: il valore usato è `mean`;
       5. riga di log per sonda, per esempio
          `Analisi VMAF - CRF 44, segmento 2/3 (da 00:04:12): VMAF 93.80, 1.72 Mb/s`;
       6. una sonda fallita (errore di esecuzione o di parsing) non interrompe
          l'analisi: si registra `None` per quel segmento e si continua, con una
          riga di log che dice quale sonda è fallita e perché;
       7. `RunOutcome::Cancelled` interrompe subito: `mark_cancelled` e ritorno,
          come per i passaggi di codifica (I-6).
     - per ogni CRF calcolare `vmaf_medio` sui segmenti misurati (media delle
       misure presenti; `None` se nessuna) e `bitrate_medio_bps`;
     - `let (crf_effettivo, motivo) = scegli_crf(&risultati, params.vmaf_target, params.crf);`
     - costruire `VmafAnalysis` con tutti i campi, `duration_seconds` misurato
       con `std::time::Instant`, `error: None` (oppure il motivo del fallimento
       complessivo);
     - emettere `righe_riepilogo(&analisi)` una riga per volta con `log_line`,
       così il CRF scelto resta scritto nel log del file (I-8);
     - la codifica successiva usa una copia dei parametri con il CRF scelto:
       `let mut params_codifica = params.clone(); params_codifica.crf = crf_effettivo;`
       e il resto del percorso resta identico.
  5. Nella scelta degli argomenti per passaggio (`src/worker.rs:246-268`) il
     `match params.mode` deve trattare `VmafAnalysis` come `Crf`, usando
     `build_crf_args(&params_codifica, …)`. `TargetSize` resta invariato.
  6. `finish_file` (`src/worker.rs:433`): aggiungere il parametro
     `vmaf_analysis: Option<VmafAnalysis>` accanto a quello introdotto in fase 4
     e assegnarlo al file nella stessa mutazione. I chiamanti `mark_cancelled` e
     `fail_file` passano `None`.
  7. Con le altre modalità non deve cambiare nulla: nessuna sonda, nessun file
     temporaneo aggiuntivo scritto, `passi_totali` identico a prima (I-7).
- **Unit tests:** in `src/worker.rs`:
  **T-AN7** `il_conteggio_dei_passaggi_satura` — con 10 segmenti e 10 step il
  calcolo di `passi_totali` restituisce `255` invece di andare in overflow;
  `senza_modalita_di_analisi_non_ci_sono_sonde` — con `mode: Crf` il numero di
  sonde calcolato è `0` e `passi_totali == 1` (`2` con `measure_vmaf`).
  Estrarre il calcolo in una funzione privata testabile
  (`fn passi_totali(params: &EncodingParams, sonde: u32) -> u8`) invece di
  lasciarlo inline, così è verificabile senza ffmpeg.
- **e2e tests:** nessuno qui; T-AN10, T-AN11 e T-AN12 sono in fase 7.
- **Done:** `cargo test --lib` verde, `cargo clippy --all-targets -- -D warnings`
  senza avvisi, `cargo test --test e2e_conversion` interamente verde (nessun
  test esistente usa la nuova modalità, quindi nessun comportamento cambia) +
  `STATE.md` e `resume.md` aggiornati.

### 6.5 Verifica manuale della modalità su un file reale

- **Model:** `agent:gpt5.6-luna`
- **Assignment:** `agent:gpt5.6-luna` — esecuzione, prima di esporre la modalità
  nell'interfaccia.
- **Files:** nessuna modifica; esiti in `STATE.md` §7.
- **Change:**
  1. Con i binari della nuova immagine nel `PATH`
     (`target/ffmpeg9/bin`, § 3.1), avviare il server con `cargo run` e creare
     un job via API in modalità di analisi su un clip di circa 90 secondi:
     ```
     curl -fsS -X POST http://127.0.0.1:8080/api/jobs \
       -H 'content-type: application/json' \
       -d '{"name":"analisi","params":{"mode":"vmaf_analysis","crf":40,"preset":10,
            "scale_height":480,"vmaf_segments":2,"crf_test_steps":2,"vmaf_target":93.0}}'
     ```
     `preset 10` per tenere la prova sotto il minuto.
  2. Caricare il clip, avviare, e verificare nel log del file
     (`GET /api/jobs/{id}/files/{file_id}/log`): la riga iniziale con segmenti e
     CRF, una riga per sonda, il riepilogo per CRF e la riga
     `Analisi VMAF - CRF scelto:`.
  3. Verificare in `GET /api/jobs/{id}` che `files[0].vmaf_analysis` contenga
     `crf_scelto`, `motivo`, i due segmenti e i due risultati, e che
     `crf_scelto` sia coerente con il criterio applicato ai numeri riportati.
  4. Registrare in `STATE.md` §7 il CRF scelto, i punteggi ottenuti e il tempo
     totale dell'analisi: sono i numeri che serviranno alla documentazione della
     fase 7 per dare un esempio realistico di costo.
- **Unit tests:** nessuno.
- **e2e tests:** verifica manuale come sopra; la versione automatica è T-AN10 in
  fase 7.
- **Done:** l'esito è registrato in `STATE.md` §7 con CRF scelto, punteggi e
  durata dell'analisi + `STATE.md` e `resume.md` aggiornati.

### 6.6 Update README.md

- **Model:** `agent:gpt5.6-luna`
- **Assignment:** `agent:gpt5.6-luna` — documentazione.
- **Files:** `README.md` (sezione delle modalità di conversione, sezione dei
  parametri, sezione dei limiti noti).
- **Change:** documentare la modalità come comportamento del backend, ossia ciò
  che questa fase rende usabile via API (l'interfaccia web arriva in fase 7):
  - cosa fa la modalità "Analisi VMAF + Qualità Costante": parte dal CRF
    impostato, prova CRF crescenti di 2 in 2 su spezzoni casuali da 30 secondi,
    e usa per la conversione il CRF più alto che mantiene il VMAF medio sopra il
    target;
  - i tre parametri con i default (`vmaf_segments` 3, `crf_test_steps` 3,
    `vmaf_target` 93) e l'intervallo ammesso, più le variabili d'ambiente
    corrispondenti;
  - un esempio realistico di chiamata API con la modalità attiva e le righe di
    log che ci si deve aspettare, con i numeri raccolti in § 6.5;
  - il costo: `segmenti × step` codifiche brevi più altrettante misure prima
    della conversione, con l'esempio dei tempi misurati; il consiglio di
    lasciare valori bassi con preset lenti;
  - limiti noti: il CRF scelto vale per quel file; se tutti i CRF provati
    superano il target conviene aumentare gli step; se nessuno lo raggiunge
    viene usato il CRF impostato; su file più corti di 30 secondi si analizza il
    file intero; il punteggio dipende dal modello scelto e i valori di modelli
    diversi non sono confrontabili.
  Preservare struttura, tono e lingua; modificare, non riscrivere. Nessun nome
  di modulo o funzione, nessun riferimento al piano.
- **Unit tests:** nessuno (documentazione).
- **e2e tests:** nessuno — l'esempio documentato va eseguito almeno una volta.
- **Done:** un utente usa la modalità via API e interpreta i log leggendo solo
  il README; nessun dettaglio implementativo; gates verdi; `STATE.md` e
  `resume.md` aggiornati.

---

## Phase gates

- **Fmt:** `cargo fmt --all -- --check`
- **Lint:** `cargo clippy --all-targets -- -D warnings`
- **Test subset:** `cargo test --lib`, `cargo test --test api`,
  `PATH="$PWD/target/ffmpeg9/bin:$PATH" cargo test --test e2e_conversion`
- **Regression guard:** T-E2E-ALL e T-VMAF1 restano verdi; I-7 (nessuna sonda
  fuori dalla nuova modalità); I-4 (job senza `vmaf_analysis` leggibili).
- **README:** aggiornato con la modalità, i tre parametri, il costo e i limiti.

## Phase done criterion

Con `mode` diverso da `vmaf_analysis` il comportamento è identico a prima. Con
`mode = vmaf_analysis` il worker esegue `segmenti × step` sonde da 30 secondi,
sceglie il CRF più alto con VMAF medio sopra il target (ripiegando sul CRF
impostato quando nessuno lo raggiunge), scrive il riepilogo e la riga del CRF
scelto nel log del file, persiste tutto in `JobFile.vmaf_analysis` e codifica il
file con quel CRF.
