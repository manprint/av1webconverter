# Phase 1 — Backend: sentinella "dimensione originale" e filtro di scala opzionale

> **Intent:** `scale_height = 0` diventa un valore valido che significa "nessun
> ridimensionamento", e in quel caso ffmpeg viene invocato senza `-vf`.
> **Shippable alone?** yes — il backend accetta e onora `0` da API, TOML ed env;
> la UI non offre ancora la scelta ma nulla si rompe.
> **Additiva:** sì. Il dominio dei valori accettati si allarga (`0` prima era
> rifiutato) e il ramo senza `-vf` si attiva solo per un valore che prima era
> invalido: nessun input già valido cambia comportamento. Provato da I-2.
> **Preconditions:** nessuna.

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

### 1.1 `scale_height = 0` accettato dalla validazione

- **Model:** `agent:gpt5.6-luna`
- **Assignment:** `agent:gpt5.6-luna` — implementazione e self-review.
- **Files:** `src/config.rs:27-28` (doc comment del campo), `src/config.rs:76-78`
  (ramo di `EncodingParams::validate`)
- **Change:**
  1. In `src/config.rs`, sostituisci il doc comment del campo `scale_height`
     (attualmente la sola riga 27, `/// Altezza massima dell'output; l'aspetto viene mantenuto e il SAR forzato a 1:1.`)
     con:
     ```rust
     /// Altezza massima dell'output; l'aspetto viene mantenuto e il SAR forzato
     /// a 1:1. Il valore `0` significa dimensione originale: nessun
     /// ridimensionamento e SAR del sorgente intatto.
     pub scale_height: u32,
     ```
  2. In `EncodingParams::validate` (`src/config.rs:75`), sostituisci il primo
     ramo, oggi:
     ```rust
     if self.scale_height < 16 || self.scale_height > 4320 {
         return Err("scale_height deve essere compreso fra 16 e 4320".into());
     }
     ```
     con:
     ```rust
     // 0 è la sentinella di "dimensione originale": nessun ridimensionamento.
     if self.scale_height != 0 && (self.scale_height < 16 || self.scale_height > 4320) {
         return Err(
             "scale_height deve essere 0 (dimensione originale) oppure compreso fra 16 e 4320"
                 .into(),
         );
     }
     ```
  3. **Non** toccare `Default for EncodingParams` (`src/config.rs:56`): il default
     resta `scale_height: 480` (D6).
  4. **Non** toccare l'override d'ambiente `AV1WC_SCALE_HEIGHT`
     (`src/config.rs:435-439`): `value.parse::<u32>()` accetta già `"0"` e la
     validazione a valle ora lo ammette.
- **Unit tests:** nel modulo `mod tests` di `src/config.rs` (inizia a
  `src/config.rs:491`; metti i nuovi test accanto a
  `suffix_di_output_e_validato`, riga 542):
  - `altezza_zero_significa_dimensione_originale` — costruisce
    `EncodingParams { scale_height: 0, ..Default::default() }` e asserisce
    `params.validate().is_ok()`.
  - `altezza_fra_uno_e_quindici_e_rifiutata` — con `scale_height: 8` asserisce
    che `validate()` è `Err` e che il messaggio contiene
    `"0 (dimensione originale)"`.
  - `altezza_oltre_il_massimo_e_rifiutata` — con `scale_height: 4321` asserisce
    che `validate()` è `Err`.
- **e2e tests:** none — il comportamento osservabile arriva con 1.2, provato in fase 3.
- **Done:** gates verdi (`cargo fmt --all -- --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test --lib`) + il test
  esistente `config_di_default_e_valida` (`src/config.rs:552`) resta verde +
  `STATE.md` e `resume.md` aggiornati.

---

### 1.2 `-vf` omesso quando l'altezza è "originale"

> **Cambio di comportamento e di firma pubblica.** `scale_filter` passa da
> `-> String` a `-> Option<String>`. Con `scale_height == 0` la riga di comando
> ffmpeg non contiene più `-vf`: nessun ridimensionamento **e** nessuna
> normalizzazione del SAR (D3). Per ogni altezza `>= 16` gli argomenti restano
> byte-identici a prima (I-2).

- **Model:** `agent:gpt5.6-luna`
- **Assignment:** `agent:gpt5.6-luna` — implementazione; **self-review obbligatoria**:
  è la catena di codifica, percorso caldo condiviso da CRF e two-pass.
- **Files:** `src/ffmpeg/encode.rs:30-41` (doc comment e firma di `scale_filter`),
  `src/ffmpeg/encode.rs:65-79` (`video_encoder_args`)
- **Change:**
  1. Aggiungi in coda al doc comment di `scale_filter`
     (`src/ffmpeg/encode.rs:30-37`, l'ultima riga esistente è
     `/// La virgola dentro min() va protetta, perché la virgola separa i filtri.`)
     un paragrafo nuovo, lasciando intatte le righe già presenti:
     ```rust
     ///
     /// `scale_height == 0` significa dimensione originale: non serve alcun
     /// filtro, quindi `-vf` non viene passato e il SAR del sorgente resta
     /// intatto.
     ```
  2. Sostituisci il corpo di `scale_filter` (`src/ffmpeg/encode.rs:38-41`) con:
     ```rust
     pub fn scale_filter(scale_height: u32) -> Option<String> {
         if scale_height == 0 {
             return None;
         }
         let height = format!("trunc(min({scale_height}\\,ih)/2)*2");
         Some(format!("scale=-2:{height}:reset_sar=1"))
     }
     ```
  3. Sostituisci `video_encoder_args` (`src/ffmpeg/encode.rs:65-79`), oggi un
     singolo `vec![...]` che contiene `"-vf"` e
     `scale_filter(params.scale_height).into()` alle righe 68-70, con:
     ```rust
     fn video_encoder_args(params: &EncodingParams) -> Vec<OsString> {
         let mut args: Vec<OsString> = vec!["-c:v".into(), "libsvtav1".into()];
         // Nessun filtro in modalità dimensione originale: -vf va omesso, non svuotato.
         if let Some(filter) = scale_filter(params.scale_height) {
             args.push("-vf".into());
             args.push(filter.into());
         }
         args.extend([
             "-preset".into(),
             params.preset.to_string().into(),
             "-svtav1-params".into(),
             params.effective_svtav1_params().into(),
             "-g".into(),
             params.keyint.to_string().into(),
             "-pix_fmt".into(),
             params.pixel_format.clone().into(),
         ]);
         args
     }
     ```
     L'ordine degli altri argomenti non cambia: `-c:v libsvtav1` resta primo e
     `-preset` segue immediatamente il filtro quando presente.
  4. Verifica che non esistano altri punti di chiamata di `scale_filter` oltre a
     `video_encoder_args`: `grep -rn "scale_filter" src/`. Attesi solo la
     definizione, la chiamata in `video_encoder_args` e i test del modulo.
     `video_encoder_args` è già usato sia da `build_crf_args`
     (`src/ffmpeg/encode.rs:85`) sia da `build_two_pass_args`
     (`src/ffmpeg/encode.rs:118`), quindi entrambe le modalità sono coperte da
     questa singola modifica (I-1).
- **Unit tests:** aggiornati e nuovi, nel `mod tests` di `src/ffmpeg/encode.rs`
  (inizia a riga 322):
  - **Aggiorna** `filtro_di_scala_calcola_la_larghezza_dal_dar`
    (`src/ffmpeg/encode.rs:344-349`): `scale_filter(480)` →
    `scale_filter(480).unwrap()`, assert invariata su
    `"scale=-2:trunc(min(480\\,ih)/2)*2:reset_sar=1"`.
  - **Aggiorna** `filtro_di_scala_resetta_il_sar` (`src/ffmpeg/encode.rs:352-358`):
    `let filter = scale_filter(720).unwrap();`, le tre assert restano identiche.
  - `filtro_di_scala_assente_con_altezza_originale` — asserisce
    `assert_eq!(scale_filter(0), None)`.
  - `comando_crf_omette_vf_con_altezza_originale` — costruisce
    `EncodingParams { scale_height: 0, ..EncodingParams::default() }`, chiama
    `build_crf_args(&params, Path::new("/in/a.mp4"), Path::new("/tmp/a.mkv"))`,
    passa il risultato in `as_strings` (`src/ffmpeg/encode.rs:326`) e asserisce:
    `!args.iter().any(|arg| arg == "-vf")`, nessun elemento che contenga
    `"scale=-2"`, e `args.join(" ").contains("-c:v libsvtav1 -preset 1")` (prova
    che l'ordine resta corretto senza il filtro).
  - `comando_due_passaggi_omette_vf_con_altezza_originale` — stessi `params` più
    `mode: EncodingMode::TargetSize` e `target_size_mb: Some(300.0)`; chiama
    `build_two_pass_args(&params, Path::new("/in/a.mp4"), Path::new("/tmp/a.mkv"), Path::new("/tmp/a.log"), 1_000_000, 1)`
    (stessa forma di `primo_passaggio_scarta_output_e_audio`,
    `src/ffmpeg/encode.rs:403-412`) e asserisce l'assenza di `-vf`.
- **e2e tests:** none in questa fase — la prova su conversione reale è **T-SH1** in
  `phase_03.md § 3.2`.
- **Done:** gates verdi (`cargo fmt --all -- --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test --lib`,
  `cargo test --test api`) + il test esistente
  `comando_crf_riproduce_lo_script_di_riferimento` (`src/ffmpeg/encode.rs:360`,
  che asserisce `-vf scale=-2:trunc(min(480\,ih)/2)*2:reset_sar=1` con i default)
  resta verde senza modifiche — è la prova di I-2 + `STATE.md` e `resume.md`
  aggiornati.

---

### 1.3 Verifica incrociata: config, env e job persistiti

- **Model:** `agent:gpt5.6-luna`
- **Assignment:** `agent:gpt5.6-luna` — verifica e, se serve, correzione.
- **Files:** `src/config.rs:435-439` (solo lettura), `src/state.rs` e
  `src/store.rs` (solo lettura), `src/config.rs` `mod tests`
- **Change:** non è previsto codice di produzione nuovo. Verifica tre percorsi e
  scrivi il risultato in `STATE.md` §7:
  1. `AV1WC_SCALE_HEIGHT=0 cargo run -- --print-config` stampa
     `scale_height = 0` senza errore di validazione.
  2. Un `config.toml` con `scale_height = 0` viene caricato senza errore.
  3. Un job JSON già presente in `data_dir/jobs/` con `"scale_height": 480`
     deserializza come prima (nessun campo aggiunto, `deny_unknown_fields`
     invariato).
  Se uno dei tre falla, la causa è nel piano: fermati, registra la deviazione in
  `STATE.md` §8 e correggi 1.1 prima di proseguire.
- **Unit tests:**
  - `altezza_zero_da_ambiente_e_accettata` — in `mod tests` di `src/config.rs`:
    costruisce una `Config` di default, imposta `config.encoding.scale_height = 0`
    e asserisce che `config.validate()` (`src/config.rs:453`) è `Ok`. Non usa
    variabili d'ambiente reali, per non rendere il test dipendente dall'ordine di
    esecuzione.
- **e2e tests:** none (nessun comportamento nuovo, solo verifica).
- **Done:** i tre percorsi verificati e annotati in `STATE.md` §7 con l'output
  osservato + gates verdi (`cargo fmt --all -- --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test --lib`) + `STATE.md` e
  `resume.md` aggiornati.

---

### 1.4 Update README.md

Sub-fase di chiusura obbligatoria. Solo guida utente, nessun dettaglio
implementativo.

- **Model:** `agent:gpt5.6-luna`
- **Assignment:** `agent:gpt5.6-luna` — documentazione e self-review.
- **Files:** `README.md` (radice del repo)
- **Change:** aggiorna queste sezioni per ciò che **questa fase rende
  effettivamente usabile** (il valore `0` accettato da API, TOML ed env):
  - **`## Configurazione`**, tabella delle variabili d'ambiente, riga
    `AV1WC_SCALE_HEIGHT` (`README.md:137`): la descrizione diventa
    `altezza massima dell'output; 0 = dimensione originale`.
  - **`## Parametri di conversione`**, elenco dei default rilevanti, ultimo punto
    (`README.md:163-165`, quello che inizia con
    `- Il ridimensionamento limita l'altezza a scale_height ...`): conserva il
    testo esistente e aggiungi in coda al punto una frase:
    `Con scale_height = 0 non viene applicato alcun ridimensionamento: il video
    conserva dimensione e rapporto dei pixel del sorgente.`
  Includi: cosa cambia per l'utente, il valore di default (480), come esprimere
  "dimensione originale" nel file di configurazione e via variabile d'ambiente.
  Escludi: nomi di funzioni o moduli, `scale_filter`, `-vf`, riferimenti a fasi o
  a questo piano. Preserva struttura, tono e lingua (italiano) esistenti: modifica,
  non riscrivere. Nessuna emoji.
- **Unit tests:** none (documentazione).
- **e2e tests:** none — il comando `AV1WC_SCALE_HEIGHT=0 cargo run -- --print-config`
  citato in 1.3 è stato eseguito e ha prodotto l'output documentato.
- **Done:** un utente nuovo capisce dal solo README come ottenere una conversione
  senza ridimensionamento, senza leggere il codice; nessun dettaglio
  implementativo presente; gates verdi; `STATE.md` e `resume.md` aggiornati.

---

## Phase gates

- **Fmt:** `cargo fmt --all -- --check`
- **Lint:** `cargo clippy --all-targets -- -D warnings`
- **Test subset:** `cargo test --lib` e `cargo test --test api`
- **Regression guard:** `comando_crf_riproduce_lo_script_di_riferimento`,
  `comando_crf_include_i_thread_solo_se_richiesti`,
  `primo_passaggio_scarta_output_e_audio`, `config_di_default_e_valida`
- **README:** aggiornato per il comportamento utente introdotto da questa fase,
  privo di dettagli implementativi

## Phase done criterion

`EncodingParams { scale_height: 0, .. }.validate()` è `Ok`, e
`build_crf_args` / `build_two_pass_args` con `scale_height = 0` producono una
riga di comando che non contiene `-vf` né `scale=-2`, mentre con
`scale_height = 480` la producono identica a prima del piano. Provato da
`filtro_di_scala_assente_con_altezza_originale`,
`comando_crf_omette_vf_con_altezza_originale`,
`comando_due_passaggi_omette_vf_con_altezza_originale` e
`comando_crf_riproduce_lo_script_di_riferimento`. `README.md` riflette il
comportamento rilasciato da questa fase.
