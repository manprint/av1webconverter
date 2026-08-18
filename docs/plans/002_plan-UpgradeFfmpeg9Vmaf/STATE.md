# Aggiornamento toolchain FFmpeg 9, misura VMAF e modalità di analisi — Implementation State

> **LEGGERE QUESTO FILE PER PRIMO all'inizio di ogni sessione, prima di
> qualunque altro file del piano. AGGIORNARLO dopo ogni sotto-fase e prima di
> chiudere una sessione.**
> **Last updated:** 2026-08-18T14:25:00Z | **By:** `agent:gpt5.6-luna` | **Session:** 1

## 0. Protocol

**Resume (avvio a freddo):**
1. Leggere questo file dall'inizio alla fine.
2. Eseguire i comandi di gate elencati in §3 e confrontare il risultato con
   quanto dichiarato in §1 e §7. La verità è il repository: correggere questo
   file se è divergente.
3. Aprire solo il file di fase indicato in §1 `Next action:`, alla sotto-fase
   nominata. Leggere `overview.md` solo se §2 non basta per il lavoro in corso.
4. Se §6 non è vuoto, completare o annullare quel lavoro in sospeso prima di
   iniziare qualcosa di nuovo.

**Update (dopo ogni sotto-fase, obbligatorio):** riscrivere §1, aggiungere una
riga a §4, aggiornare §5 §6 §7, aggiungere righe a §8 se si è deviato dal piano,
rinfrescare §9 §10, aggiornare il timestamp dell'intestazione, poi sincronizzare
`resume.md`. Una sotto-fase non è `DONE` finché questo non è scritto.

## 1. Current position

- **Phase:** 7 — Modalità di analisi: API, frontend, end-to-end (`phase_07.md`) — status `DONE`
- **Sub-phase:** 7.4 — Update README.md — status `DONE`
- **Next action:** fase 7 completata; nessuna modifica oltre fase 7 senza richiesta esplicita
- **Assigned:** `agent:gpt5.6-luna` — esecutore unico di tutto il piano (D29). Non
  esiste un secondo agente revisore: dove una sotto-fase chiede una revisione,
  va svolta come autorevisione esplicita, cioè una rilettura del lavoro appena
  scritto contro il campo **Change** e le invarianti citate, prima di
  dichiararla conclusa.
- **Repo state:** branch `main` | working tree `dirty` (modifiche precedenti condivise, `.serena/` non tracciata, questa cartella di piano e modifiche 4.1) | last commit `0c2fe8e feat: add original-size scale option`

## 2. Feature context (self-contained recap)

Aggiornare l'immagine ffmpeg custom da FFmpeg 8.1.2 a 9.0.1, libvmaf da 3.1.0 a
3.2.0, dav1d da 1.5.3 a 1.5.4 (SVT-AV1 resta 4.2.0, già ultima release) e
compilare libopus 1.6.1 in statico, così il binario ffmpeg non ha più
dipendenze dinamiche verso libopus. Nessuna opzione della riga di comando usata
dall'applicazione è stata rimossa in FFmpeg 9. Sfruttando libvmaf 3.2.0 si
aggiungono due cose: una misura di qualità VMAF opzionale per file, calcolata
dopo la conversione sull'MP4 definitivo rispetto al sorgente (fasi 4 e 5), e una
terza modalità di codifica **Analisi VMAF + Qualità Costante** (fasi 6 e 7) che,
partendo dal CRF impostato dall'utente, prova CRF crescenti di 2 in 2 su
spezzoni casuali della durata configurata e converte il file con il CRF più alto che
mantiene il VMAF medio sopra la soglia richiesta.

**Reference scenario:** costruite le due immagini con il tag
`9.0.1-svt4.2.0-vmaf3.2.0`; (A) un job con `measure_vmaf=true` e
`scale_height=720` su un clip 1080p di 10 s termina `completed`, `GET
/api/jobs/{id}` restituisce `files[0].vmaf.model == "vmaf_v1.0.16_5d0h"` e
`0 < files[0].vmaf.score <= 100`, e l'MP4 è identico byte per byte a quello
prodotto con `measure_vmaf=false`; (B) un job con `mode = "vmaf_analysis"`,
`crf = 40`, `crf_test_steps = 3`, `vmaf_segments = 3`, `vmaf_target = 93.0` su un
clip di 120 s produce `files[0].vmaf_analysis` con i CRF 42, 44, 46 misurati su
tre segmenti, `crf_scelto` conforme al criterio di soglia, la riga
`Analisi VMAF - CRF scelto:` nel log del file e l'MP4 codificato con quel CRF.

**Hard constraints:** l'output pubblicato non deve dipendere dalla misura
(I-1); nessuna dipendenza dinamica verso libopus nel binario ffmpeg (I-3); i
job già persistiti su disco devono restare leggibili (I-4); un fallimento della
misura non fa fallire il file (I-5); la cancellazione interrompe anche misure e
sonde (I-6); fuori dalla modalità di analisi non viene eseguita nessuna sonda e
il percorso resta identico a oggi (I-7); il CRF usato in analisi è quello
registrato e compare nel log (I-8); italiano per identificatori, commenti e
testi; nessuna nuova cartella dove una esistente serve già allo scopo; nessuna
nuova dipendenza in `Cargo.toml`.

**Key decisions in force:** D1 FFmpeg 9.0.1; D2 libopus 1.6.1 statico e
rimozione di `libopus-dev`/`libopus0`; D5 tag `9.0.1-svt4.2.0-vmaf3.2.0` con
dav1d solo nella label; D6 base bookworm e `rust:1.96-bookworm` invariate;
D7 misura opt-in default `false`; D8 modello di default `vmaf_v1.0.16_5d0h`, il
modello telefono di VMAF v1 (`vmaf_v1.0.16` non è un nome valido); D9 misura
dopo il rename finale; D10 fallimento tollerato; D11 misura come passaggio
extra; D12 confronto alla risoluzione di uscita, riferimento riscalato solo se
la geometria differisce, `format=<pixel_format>` su entrambi i rami; D13 thread
dalla `cpus` esistente; D14 `vmaf` e `vmaf_analysis` con `#[serde(default)]`;
D15 SVT-AV1 resta 4.2.0; D16 terza modalità `vmaf_analysis`; D17 parametri
`vmaf_segments` 3, `crf_test_steps` 3, `vmaf_target` 93.0, durata segmento 30 s;
non configurabili; D19 ogni CRF su tutti i segmenti, media per CRF; D20 CRF
provati `crf+2·k`, scartati quelli sopra 63; D21 si sceglie il CRF più alto con
media sopra il target, altrimenti il CRF impostato dall'utente; D22 sonde con
gli stessi parametri video della codifica finale, senza audio; D23 segmenti
casuali con testa e coda escluse; D24 casualità dai byte dell'UUID del file;
D25 passaggi mostrati `sonde·2 + finali`, saturati a 255; D26 esito persistito
in `JobFile.vmaf_analysis`; D27 analisi per file; D28 `vmaf_analysis` si
comporta come `crf` in `effective_svtav1_params`.

## 3. Environment and commands

Comandi di gate autorevoli. Identici ai gate di fase e alla sezione di verifica
di `overview.md`: non devono divergere.

- **Repo root:** `/mnt/fabio/dati/Git/Github-manprint/av1webconverter`
- **Build:** `cargo build --release` · **Fmt:** `cargo fmt --all -- --check` · **Lint:** `cargo clippy --all-targets -- -D warnings`
- **Unit tests:** `cargo test --lib` · **API:** `cargo test --test api` · **E2E:** `cargo test --test e2e_conversion`
- **E2E con i binari della nuova immagine:** `PATH="$PWD/target/ffmpeg9/bin:$PATH" cargo test --test e2e_conversion`
- **Build immagine ffmpeg:** `docker build -f Dockerfile.ffmpeg-svtav1 -t local/ffmpeg-svtav1:9.0.1-svt4.2.0-vmaf3.2.0 .`
- **Build immagine applicativa:** `docker build --build-arg FFMPEG_IMAGE=local/ffmpeg-svtav1:9.0.1-svt4.2.0-vmaf3.2.0 -t av1webconverter:local .`
- **Setup / caveats:** le build docker richiedono rete e alcuni minuti. La suite
  e2e si auto-salta senza un ffmpeg con `libsvtav1` (`tests/common/mod.rs:94`) e
  le prove VMAF si auto-salteranno senza `libvmaf` (helper introdotto in § 5.3).
  Per usare i binari dell'immagine nei test serve il `PATH`, non
  `AV1WC_FFMPEG_BIN`: gli helper di test invocano `ffmpeg`/`ffprobe` dal `PATH`.
  I binari si estraggono con `docker create` + `docker cp` in
  `target/ffmpeg9/bin` (cartella già ignorata da git).

## 4. Work ledger (append-only, one line per sub-phase, task, or bug fix)

| # | Phase.Sub | Agent | What changed | Files | Gates | Commit |
|---|-----------|-------|--------------|-------|-------|--------|
| 1 | 1.1 | `agent:gpt5.6-luna` | Aggiornate versioni e checksum FFmpeg, libVMAF e dav1d in entrambi gli stage | `Dockerfile.ffmpeg-svtav1` | grep versioni: verde | uncommitted |
| 2 | 1.2 | `agent:gpt5.6-luna` | Compilazione libopus 1.6.1 statica; rimossi pacchetti libopus dinamici | `Dockerfile.ffmpeg-svtav1` | grep dipendenze: verde | uncommitted |
| 3 | 1.3 (in corso) | `agent:gpt5.6-luna` | Prima build riuscita; T-IMG1/T-IMG2 verdi, T-IMG3 fallito e corretto con `enable_float=true` | `Dockerfile.ffmpeg-svtav1`, `overview.md`, `phase_01.md` | build iniziale verde; T-IMG3 da ripetere | uncommitted |
| 4 | 1.3 | `agent:gpt5.6-luna` | Ricostruita immagine con feature floating-point; T-IMG1/T-IMG2/T-IMG3 verdi; audit R2 senza collisioni | `STATE.md` | T-IMG1/2/3: verde | uncommitted |
| 5 | 1.4 | `agent:gpt5.6-luna` | README aggiornati con tag, componenti e modelli VMAF v1 | `README.md`, `README.image.md` | grep tag: verde | uncommitted |
| 6 | 2.1 | `agent:gpt5.6-luna` | Tag applicativo aggiornato; runtime senza libopus0; immagine costruita | `Dockerfile` | build applicativa e ldd: verde | uncommitted |
| 7 | 2.2 | `agent:gpt5.6-luna` | Workflow, script, architettura e CLAUDE aggiornati a FFmpeg 9.0.1 | `.github/workflows/docker-publish.yml`, `start_av1_docker.sh`, `docs/architettura.md`, `CLAUDE.md`, `src/ffmpeg/encode.rs`, `tests/common/mod.rs` | YAML: verde | uncommitted |
| 8 | 2.3 | `agent:gpt5.6-luna` | README applicativo e immagine allineati al nuovo tag | `README.md`, `README.image.md` | grep vecchio tag: verde | uncommitted |
| 9 | 4.1 | `agent:gpt5.6-luna` | Aggiunti misura VMAF opt-in, modello predefinito, validazione e override ambiente | `src/config.rs`, `config.example.toml` | config unit: 14 pass; print-config, fmt, clippy: verde | uncommitted |
| 10 | 4.2 | `agent:gpt5.6-luna` | Aggiunti `VmafMeasurement`, campo persistito compatibile su `JobFile` e test JSON | `src/models.rs` | models unit: 6 pass; fmt, clippy: verde | uncommitted |
| 11 | 4.3 | `agent:gpt5.6-luna` | Aggiunti argomenti filtro VMAF, parser log JSON e conteggio passaggi con misura | `src/ffmpeg/vmaf.rs`, `src/ffmpeg/mod.rs`, `src/ffmpeg/encode.rs` | ffmpeg unit: 35 pass; fmt, clippy: verde; autorevisione filtro corretta | uncommitted |
| 12 | 4.4 | `agent:gpt5.6-luna` | Innestata misura opt-in dopo rename finale, con progresso, cancellazione e tolleranza errori | `src/worker.rs` | worker unit: 5 pass; lib: 97 pass; API: 17 pass; e2e: 8 pass; fmt, clippy: verde | uncommitted |
| 13 | 4.5 | `agent:gpt5.6-luna` | Documentati parametri, env, campo `vmaf`, confronto alla risoluzione di uscita e limiti | `README.md` | diff check: verde | uncommitted |
| 14 | 3.3 | `agent:gpt5.6-luna` | README documenta test e2e con binari FFmpeg custom tramite PATH | `README.md` | grep procedura: verde | uncommitted |
| 15 | 3.1 | `agent:gpt5.6-luna` | Suite e2e con binari FFmpeg 9 passata; T-REG1/T-REG2 passati | `target/ffmpeg9/bin` | 8 e2e, tune5, two-pass: verde | uncommitted |
| 16 | 3.2 | `agent:gpt5.6-luna` | Smoke container con sorgente 1080p e output AV1 720p verificato | `/tmp/av1wc-smoke` | T-SMOKE1: verde | uncommitted |
| 17 | 5.1 | `agent:gpt5.6-luna` | Aggiornato contratto API VMAF e aggiunti test per default, upload e modello non valido | `docs/api.md`, `tests/api.rs` | T-VMAF5, API 20: verde; fmt: verde | uncommitted |
| 18 | 5.2 | `agent:gpt5.6-luna` | Aggiunti campi VMAF a form/editor e rendering punteggio o errore nella riga file | `src/web/app.js`, `src/web/index.html` | lib 97, API 20, clippy, fmt, node check e marker frontend: verde | uncommitted |
| 19 | 5.3 | `agent:gpt5.6-luna` | Aggiunto helper libvmaf e test T-VMAF1/T-VMAF6/T-VMAF7; suite con FFmpeg 9 verde | `tests/common/mod.rs`, `tests/e2e_conversion.rs` | T-VMAF1/6/7, T-E2E-ALL: 11 verdi; PATH FFmpeg 9 | uncommitted |
| 20 | 5.4 | `agent:gpt5.6-luna` | README completato con uso UI, interpretazione punteggio, troubleshooting e limiti VMAF | `README.md` | README autorevisionato; gate finale verde | uncommitted |
| 21 | 6.1 | `agent:gpt5.6-luna` | Aggiunti modalità `vmaf_analysis`, tre parametri con default, validazione, override ambiente e conteggio passaggi | `src/config.rs`, `src/ffmpeg/encode.rs`, `src/worker.rs`, `config.example.toml` | config 17, encode 15, fmt: verde | uncommitted |
| 22 | 6.2 | `agent:gpt5.6-luna` | Aggiunte segmentazione deterministica da UUID e griglia CRF con scarto oltre 63 | `src/ffmpeg/vmaf.rs` | VMAF unit: 20; fmt: verde | uncommitted |
| 23 | 6.3 | `agent:gpt5.6-luna` | Aggiunti argomenti sonde e taglio riferimento, criterio scelta, riepilogo log e modelli persistiti compatibili | `src/ffmpeg/vmaf.rs`, `src/ffmpeg/encode.rs`, `src/models.rs`, `src/worker.rs` | VMAF unit: 20, models: 7; fmt: verde | uncommitted |
| 24 | 6.4 | `agent:gpt5.6-luna` | Orchestrate sonde e misure con cancellazione, error tolerance, progressione saturata e persistenza del CRF scelto | `src/worker.rs` | clippy, unit 116, API 20, e2e 11, fmt: verde | uncommitted |
| 25 | 6.5 | `agent:gpt5.6-luna` | Verifica API reale con clip 70 s e binari FFmpeg 9/libvmaf | `/tmp/av1wc-manual`, `STATE.md` | completed; 4 sonde; CRF scelto 44; VMAF 99.40/99.22; analisi 5.84 s | uncommitted |
| 26 | 6.6 | `agent:gpt5.6-luna` | README aggiornato con modalità API, parametri/env, esempio reale, costo e limiti | `README.md` | autorevisione README: verde | uncommitted |
| 27 | 7.1 | `agent:gpt5.6-luna` | Contratto API aggiornato per modalità, parametri ed esito `vmaf_analysis`; aggiunti test HTTP T-AN9 | `docs/api.md`, `tests/api.rs` | API 24, fmt, node check: verde | uncommitted |
| 28 | 7.2 | `agent:gpt5.6-luna` | Frontend esteso a tre modalità, campi analisi condizionali e dettaglio VMAF con tooltip escaped | `src/web/app.js`, `src/web/index.html` | node check, fmt, clippy, diff check: verde | uncommitted |
| 29 | 7.3 | `agent:gpt5.6-luna` | Aggiunti helper e test e2e T-AN10/T-AN11/T-AN12 con criterio CRF, log, clip corto e fallback | `tests/e2e_conversion.rs` | FFmpeg 9: 14 test passati; fmt: verde | uncommitted |
| 30 | 7.4 | `agent:gpt5.6-luna` | README completato con uso frontend, esempio numerico, esito, costo, troubleshooting e limiti | `README.md` | autorevisione README: verde; gate finali verdi | uncommitted |
| 31 | 7.final | `agent:gpt5.6-luna` | Gate finali fase 7 eseguiti | repository | fmt, clippy, build release, lib 116, API 24, e2e FFmpeg 9: verde | uncommitted |
| 32 | audit finale | `agent:gpt5.6-luna` | Corrette misura VMAF senza media, documentazione script/tag, override Compose e scenario T-AN10 completo | `src/worker.rs`, `src/ffmpeg/vmaf.rs`, `Dockerfile`, `compose.yaml`, `start_av1_docker.sh`, `README.image.md`, `CLAUDE.md`, `phase_07.md`, `tests/e2e_conversion.rs`, `overview.md`, `resume.md` | gate finale da ripetere | uncommitted |
| 33 | audit finale | `agent:gpt5.6-luna` | Acceptance analisi portata a 120 s per rispettare segmenti non sovrapposti e margini testa/coda | `tests/e2e_conversion.rs`, `phase_07.md`, `overview.md` | e2e da ripetere | uncommitted |
| 34 | audit finale | `agent:gpt5.6-luna` | Gate finali ripetuti dopo correzioni e build immagine applicativa | repository | fmt, clippy, build, unit 117, API 24, e2e 14, frontend, YAML, diff: verde | uncommitted |
| 35 | audit finale | `agent:gpt5.6-luna` | E2E FFmpeg 9 ripetuto dopo acceptance 120 s: scenario 3×3 verde | `tests/e2e_conversion.rs` | `PATH=target/ffmpeg9/bin` e2e: 14 pass | uncommitted |
| 36 | execute verify V001 | `agent:gpt5.6-luna` | Aggiunto log/assert CRF finale, README 3×3, ripristinato default script secondo phase 2.2; `.serena/` lasciata non tracciata su richiesta | `src/worker.rs`, `tests/e2e_conversion.rs`, `README.md`, `start_av1_docker.sh`, `phase_07.md`, `verify/index.md`, `verify_001`, `STATE.md`, `resume.md` | unit 117, API 24, e2e FFmpeg 9 14, fmt/clippy/frontend/YAML/diff: verde | uncommitted |
| 37 | B-A001 | `agent:gpt5.6-luna` | Rimossa fonte 9to5Linux da R2 dopo 403 Lychee; mantenuto changelog ufficiale FFmpeg | `overview.md`, `bugs.md` | URL ufficiale curl: pass; Lychee locale non installato | uncommitted |
| 38 | B-A001 verify | `agent:gpt5.6-luna` | Verificato URL raw ufficiale con container Lychee; nessun errore | `overview.md`, `bugs.md`, `verify_001`, `STATE.md` | Lychee raw: 0 errori | uncommitted |
| 39 | T-A001 | `agent:gpt5.6-luna` | Durata segmenti VMAF configurabile da API/UI/env; Compose default 15 s | `src/config.rs`, `src/ffmpeg/vmaf.rs`, `src/worker.rs`, `src/web/app.js`, `src/web/index.html`, `compose.yaml`, `config.example.toml`, `README.md`, `docs/api.md`, phase docs | unit 118; gate completo da ripetere | uncommitted |
| 40 | T-A001 verify | `agent:gpt5.6-luna` | Test API durata personalizzata e gate completi passati | `tests/api.rs`, `STATE.md`, `resume.md`, `tasks.md` | unit 118, API 25, e2e 14, fmt/clippy/build/frontend/YAML/Compose/diff: verde | uncommitted |
| 41 | T-A001 final | `agent:gpt5.6-luna` | Riferimenti phase/docs riallineati al nuovo campo e override env verificato a 15 s | `overview.md`, `phase_06.md`, `phase_07.md`, `README.md`, `docs/api.md`, `config.example.toml`, `STATE.md`, `resume.md` | full gate finale verde | uncommitted |

## 5. Files touched

| Path | What was done | Phase.Sub |
|------|---------------|-----------|
| `Dockerfile.ffmpeg-svtav1` | ARG aggiornate in builder e runtime | 1.1 |
| `Dockerfile.ffmpeg-svtav1` | blocco build opus statico e runtime senza pacchetti opus | 1.2 |
| `Dockerfile.ffmpeg-svtav1` | abilitazione feature floating-point libvmaf dopo fallimento T-IMG3 | 1.3 |
| `docs/plans/002_plan-UpgradeFfmpeg9Vmaf/overview.md` | decisione D30 registrata | 1.3 |
| `docs/plans/002_plan-UpgradeFfmpeg9Vmaf/phase_01.md` | nota operativa su enable_float | 1.3 |
| `README.md` | tag immagine locale aggiornato | 1.4 |
| `README.image.md` | componenti, tag e modelli VMAF aggiornati | 1.4 |
| `Dockerfile` | base FFmpeg 9.0.1 e runtime senza libopus0 | 2.1 |
| `.github/workflows/docker-publish.yml` | tag workflow aggiornato | 2.2 |
| `start_av1_docker.sh` | commento tag aggiornato | 2.2 |
| `docs/architettura.md` | nota reset_sar aggiornata | 2.2 |
| `CLAUDE.md` | vincolo scale corretto | 2.2 |
| `src/ffmpeg/encode.rs`, `tests/common/mod.rs` | riferimenti FFmpeg aggiornati | 2.2 |
| `src/config.rs` | campi, default, validazione, override ambiente e test VMAF | 4.1 |
| `config.example.toml` | chiavi VMAF e valori predefiniti documentati | 4.1 |
| `src/models.rs` | struttura esito VMAF, campo opzionale su file e test di compatibilità/serializzazione | 4.2 |
| `src/ffmpeg/vmaf.rs` | richiesta, filtro libvmaf, parser JSON e test | 4.3 |
| `src/ffmpeg/mod.rs` | dichiarazione modulo VMAF | 4.3 |
| `src/ffmpeg/encode.rs` | conteggio passaggi comprensivo della misura e test | 4.3 |
| `src/worker.rs` | esecuzione/persistenza VMAF dopo pubblicazione output e test regressione passaggi | 4.4 |
| `README.md` | documentazione uso e limiti della misura VMAF | 4.5 |
| `README.md` | procedura regressione con binari custom | 3.3 |
| `docs/api.md` | parametri `measure_vmaf`/`vmaf_model` e campo `files[].vmaf` | 5.1 |
| `tests/api.rs` | test API T-VMAF5 per default, upload e modello non valido | 5.1 |
| `src/web/app.js` | campi parametro VMAF, testo punteggio e errore | 5.2 |
| `src/web/index.html` | checkbox misura e input modello nel form nuovo | 5.2 |
| `tests/common/mod.rs` | helper filtro libvmaf e sorgente VMAF 640x360 | 5.3 |
| `tests/e2e_conversion.rs` | test reali VMAF riuscito, fallito e anamorfico | 5.3 |
| `README.md` | guida utente VMAF per UI, API, troubleshooting e limiti | 5.4 |
| `compose.yaml` | override ambiente VMAF esposti | audit finale |
| `start_av1_docker.sh` | immagine FFmpeg default fissata al tag 9.0.1 | audit finale |
| `src/worker.rs` | log VMAF senza media trattato come errore; cancellazione durante misura lascia output completato | audit finale |
| `docs/plans/002_plan-UpgradeFfmpeg9Vmaf/overview.md` | R2 usa solo URL ufficiale FFmpeg dopo B-A001 | B-A001 |
| `docs/plans/002_plan-UpgradeFfmpeg9Vmaf/bugs.md` | ledger bug B-A001 | B-A001 |
| `src/config.rs` | nuovo campo/default/env/validazione durata segmento | T-A001 |
| `src/ffmpeg/vmaf.rs`, `src/worker.rs` | segmentazione usa durata per job | T-A001 |
| `src/web/app.js`, `src/web/index.html` | campo durata UI | T-A001 |
| `compose.yaml`, `config.example.toml` | override Compose 15 e default file 30 | T-A001 |
| `README.md`, `docs/api.md`, phase docs | documentazione configurazione | T-A001 |
| `src/ffmpeg/vmaf.rs` | test log senza media | audit finale |
| `tests/e2e_conversion.rs` | T-AN10/T-AN12 a 120 s e 3×3 | audit finale |
| `docs/plans/002_plan-UpgradeFfmpeg9Vmaf/phase_07.md` | acceptance T-AN10 a 120 s e 3×3 | audit finale |
| `src/config.rs` | modalità analisi, parametri VMAF/CRF, default, validazione, env e test | 6.1 |
| `src/ffmpeg/encode.rs` | encoder args visibili a VMAF e un passaggio per analisi | 6.1 |
| `src/worker.rs` | match modalità analisi trattato come CRF | 6.1 |
| `config.example.toml` | modalità e parametri analisi documentati | 6.1 |
| `src/ffmpeg/vmaf.rs` | segmentazione, griglia CRF, argomenti sonde, taglio riferimento, scelta e riepilogo | 6.2, 6.3 |
| `src/models.rs` | esiti persistiti analisi VMAF e compatibilità JSON | 6.3 |
| `src/worker.rs` | orchestrazione sonde/misure, progressione, cancellazione, persistenza analisi | 6.4 |
| `/tmp/av1wc-manual` | verifica manuale API reale su clip 70 s | 6.5 |
| `README.md` | documentazione backend della modalità analisi, parametri, costo, esempio e limiti | 6.6 |
| `docs/api.md` | contratto modalità `vmaf_analysis`, parametri, progressione e campo `vmaf_analysis` | 7.1 |
| `tests/api.rs` | test T-AN9 per default, modalità accettata, intervalli e campo nullo | 7.1 |
| `tests/api.rs` | test API durata segmento personalizzata | T-A001 |
| `src/web/app.js` | opzione analisi, visibilità multi-modalità e testo/tooltip esito | 7.2 |
| `src/web/index.html` | opzione analisi e tre campi condizionali nel form nuovo | 7.2 |
| `tests/e2e_conversion.rs` | helper job analisi e T-AN10/T-AN11/T-AN12 | 7.3 |
| `README.md` | uso frontend della modalità analisi, esempio, esito, costo, troubleshooting e limiti | 7.4 |

## 6. In-flight work

 none — fase 7 conclusa; nessun lavoro sospeso.

## 7. Verification state

| Gate / test | Command | Last result | When |
|-------------|---------|-------------|------|
| fmt | `cargo fmt --all -- --check` | `pass` | 2026-08-18, fase 5 finale |
| lint | `cargo clippy --all-targets -- -D warnings` | `pass` | 2026-08-18, fase 5 finale |
| unit | `cargo test --lib` | `pass: 97` | 2026-08-18, fase 5 finale |
| config subset | `cargo test --lib config::` | `pass: 14` | 2026-08-18, fase 4.1 |
| config subset | `cargo test --lib config::` | `pass: 17` | 2026-08-18, fase 6.1 |
| print config | `cargo run -- --print-config` | `pass: chiavi VMAF presenti` | 2026-08-18, fase 4.1 |
| models subset | `cargo test --lib models::` | `pass: 6` | 2026-08-18, fase 4.2 |
| ffmpeg subset | `cargo test --lib ffmpeg::` | `pass: 35` | 2026-08-18, fase 4.3 |
| encode subset | `cargo test --lib ffmpeg::encode::` | `pass: 15` | 2026-08-18, fase 6.1 |
| vmaf subset | `cargo test --lib ffmpeg::vmaf::` | `pass: 20` | 2026-08-18, fase 6.3 |
| models subset | `cargo test --lib models::` | `pass: 7` | 2026-08-18, fase 6.3 |
| worker subset | `cargo test --lib worker::` | `pass: 7` | 2026-08-18, fase 6.4 |
| full lib | `cargo test --lib` | `pass: 116` | 2026-08-18, fase 6.4 |
| api | `cargo test --test api` | `pass: 20` | 2026-08-18, fase 6.4 |
| e2e FFmpeg 9 | `PATH="$PWD/target/ffmpeg9/bin:$PATH" cargo test --test e2e_conversion` | `pass: 11` | 2026-08-18, fase 6.4 |
| clippy | `cargo clippy --all-targets -- -D warnings` | `pass` | 2026-08-18, fase 6.4 |
| manuale T-AN real | server API + clip 70 s | `pass: completed; CRF 44; VMAF medi 99.40/99.22; 5.84 s` | 2026-08-18, fase 6.5 |
| final fmt | `cargo fmt --all -- --check` | `pass` | 2026-08-18, fase 6 finale |
| final clippy | `cargo clippy --all-targets -- -D warnings` | `pass` | 2026-08-18, fase 6 finale |
| final unit | `cargo test --lib` | `pass: 116` | 2026-08-18, fase 6 finale |
| final api | `cargo test --test api` | `pass: 20` | 2026-08-18, fase 6 finale |
| final e2e | `PATH="$PWD/target/ffmpeg9/bin:$PATH" cargo test --test e2e_conversion` | `pass: 11` | 2026-08-18, fase 6 finale |
| 7.1 api | `cargo test --test api` | `pass: 24` | 2026-08-18, fase 7.1 |
| 7.1 fmt | `cargo fmt --all -- --check` | `pass` | 2026-08-18, fase 7.1 |
| 7.1 frontend syntax | `node --check src/web/app.js` | `pass` | 2026-08-18, fase 7.1 |
| 7.2 frontend syntax | `node --check src/web/app.js` | `pass` | 2026-08-18, fase 7.2 |
| 7.2 clippy | `cargo clippy --all-targets -- -D warnings` | `pass` | 2026-08-18, fase 7.2 |
| 7.2 diff check | `git diff --check` | `pass` | 2026-08-18, fase 7.2 |
| print config env | `AV1WC_VMAF_SEGMENTS=4 AV1WC_CRF_TEST_STEPS=5 AV1WC_VMAF_TARGET=91.5 cargo run -- --print-config` | `pass: chiavi e override presenti` | 2026-08-18, fase 6 finale |
| worker subset | `cargo test --lib worker::` | `pass: 5` | 2026-08-18, fase 4.4 |
| full lib | `cargo test --lib` | `pass: 97` | 2026-08-18, fase 4 finale |
| api | `cargo test --test api` | `pass: 20` | 2026-08-18, fase 5.1 |
| fmt | `cargo fmt --all -- --check` | `pass` | 2026-08-18, fase 5.2 |
| lint | `cargo clippy --all-targets -- -D warnings` | `pass` | 2026-08-18, fase 5.2 |
| full lib | `cargo test --lib` | `pass: 97` | 2026-08-18, fase 5.2 |
| api | `cargo test --test api` | `pass: 20` | 2026-08-18, fase 5.2 |
| frontend syntax | `node --check src/web/app.js` | `pass` | 2026-08-18, fase 5.2 |
| e2e | `cargo test --test e2e_conversion` | `pass: 11` | 2026-08-18, fase 5.3 con FFmpeg 9 |
| e2e system | `cargo test --test e2e_conversion` | `pass: 11` | 2026-08-18, fase 5 finale |
| clippy/fmt finali | comandi sopra | `pass` | 2026-08-18, fase 5 finale |
| api | `cargo test --test api` | `pass: 17` | 2026-08-18, baseline |
| e2e | `cargo test --test e2e_conversion` | `pass: 8` | 2026-08-18, baseline |
| T-IMG1 | `docker run ... ffmpeg -version/ldd/encoders/decoders/filters` | pass | 2026-08-18 |
| T-IMG2 | `docker run ... reset_sar/encoder/progress` | pass | 2026-08-18 |
| T-IMG3 | `docker run ... libvmaf model vmaf_v1.0.16_5d0h` su 640x360 | pass | 2026-08-18; 320x240 escluso perché Speed Chroma richiede input maggiore |
| T-APP1 | `docker build ... av1webconverter:local` + `docker run ...` | pass: FFmpeg 9.0.1, 0 riferimenti libopus | 2026-08-18 |
| T-YAML1 | `python3 -c "import yaml..."` | pass | 2026-08-18 |
| T-E2E-ALL | `PATH="$PWD/target/ffmpeg9/bin:$PATH" cargo test --test e2e_conversion` | pass: 8 | 2026-08-18 |
| T-REG1 | comando FFmpeg 9 con tune=5 CRF | pass | 2026-08-18 |
| T-REG2 | comando FFmpeg 9 primo pass two-pass tune=0 | pass | 2026-08-18 |
| T-SMOKE1 | container applicativo + ffprobe output | pass: `av1,av01,720` | 2026-08-18 |
| T-VMAF1 | e2e | `DONE` | score in `(0,100]`, modello default, errore nullo, durata positiva, output identico byte per byte |
| T-VMAF6 | e2e | `DONE` | modello ignoto lascia file completato con score nullo ed errore valorizzato |
| T-VMAF7 | e2e | `DONE` | sorgente anamorfico misurato con score presente |
| T-E2E-ALL | e2e | `DONE` | 11 test passati con `PATH=target/ffmpeg9/bin` |
| T-VMAF5 | api | `DONE` | default VMAF, `vmaf` nullo su upload, modello non valido rifiutato con 400 |
| T-VMAF2 | unit | `DONE` | `build_vmaf_args`: due input, filtro, riscalatura condizionale, modello, thread |
| T-VMAF3 | unit | `DONE` | `parse_vmaf_log`: metriche aggregate ed errore se assenti |
| T-VMAF4 | unit | `DONE` | `JobFile` senza campo `vmaf` resta deserializzabile |
| T-AN9 | api | `DONE` | default dei tre parametri, modalità accettata, intervalli fuori limite rifiutati con 400, `vmaf_analysis` nullo su upload |
| T-AN10 | e2e | `DONE` | 120 s, CRF 42/44/46 su 3 segmenti, criterio ricalcolato dai risultati, log con CRF scelto, output AV1 |
| T-AN11 | e2e | `DONE` | 10 s, un solo segmento da inizio 0 con durata dell'intero clip, completato |
| T-AN12 | e2e | `DONE` | target 99.9, `nessuno_sopra_soglia`, CRF 40 e fallback dichiarati nel log |
| T-E2E-ALL | e2e | `DONE` | 14 test passati con `PATH=target/ffmpeg9/bin` |
| 7.4 README review | lettura completa sezione VMAF/analisi | `pass` | 2026-08-18, fase 7.4 |
| final fmt | `cargo fmt --all -- --check` | `pass` | 2026-08-18, fase 7 finale |
| final clippy | `cargo clippy --all-targets -- -D warnings` | `pass` | 2026-08-18, fase 7 finale |
| final build | `cargo build --release` | `pass` | 2026-08-18, fase 7 finale |
| final unit | `cargo test --lib` | `pass: 117` | 2026-08-18, fase 7 finale; corretto in V001 |
| final api | `cargo test --test api` | `pass: 24` | 2026-08-18, fase 7 finale |
| final e2e | `PATH="$PWD/target/ffmpeg9/bin:$PATH" cargo test --test e2e_conversion` | `pass: 14` | 2026-08-18, fase 7 finale |
| final frontend syntax | `node --check src/web/app.js` | `pass` | 2026-08-18, fase 7 finale |
| final diff check | `git diff --check` | `pass` | 2026-08-18, fase 7 finale |
| audit unit | `cargo test --lib` | `pass: 117` | 2026-08-18, audit finale |
| audit api | `cargo test --test api` | `pass: 24` | 2026-08-18, audit finale |
| T-A001 API | `cargo test --test api` | `pass: 25` | 2026-08-18; test personalizzato incluso |
| audit e2e system | `cargo test --test e2e_conversion` | `pass: 14` | 2026-08-18, audit finale |
| audit e2e FFmpeg 9 | `PATH="$PWD/target/ffmpeg9/bin:$PATH" cargo test --test e2e_conversion` | `pass: 14` | 2026-08-18, audit finale |
| V001 correction fmt | `cargo fmt --all -- --check` | `pass` | 2026-08-18 |
| V001 correction lint | `cargo clippy --all-targets -- -D warnings` | `pass` | 2026-08-18 |
| V001 correction unit | `cargo test --lib` | `pass: 117` | 2026-08-18 |
| V001 correction API | `cargo test --test api` | `pass: 24` | 2026-08-18 |
| V001 correction e2e FFmpeg 9 | `PATH="$PWD/target/ffmpeg9/bin:$PATH" cargo test --test e2e_conversion` | `pass: 14` | 2026-08-18; T-AN10 log CRF finale presente |
| V001 correction docs | README, frontend, YAML, diff | `pass` | 2026-08-18 |
| V001 correction e2e sistema | `cargo test --test e2e_conversion` | `pass: 14` | 2026-08-18 |
| B-A001 link source | `docker run --rm lycheeverse/lychee:latest --no-progress --verbose https://raw.githubusercontent.com/FFmpeg/FFmpeg/release/9.0/Changelog` | `pass: 0 errori` | 2026-08-18 |
| T-A001 unit | `cargo test --lib` | `pass: 118` | 2026-08-18 |
| T-A001 API | `cargo test --test api` | `pass: 25` | 2026-08-18 |
| T-A001 e2e | `PATH="$PWD/target/ffmpeg9/bin:$PATH" cargo test --test e2e_conversion` | `pass: 14` | 2026-08-18 |
| T-A001 env | `AV1WC_VMAF_SEGMENT_DURATION_SECONDS=15 cargo run -- --print-config` | `pass: 15.0` | 2026-08-18 |
| T-A001 Compose | `docker compose config` | `pass: override 15` | 2026-08-18 |
| audit Docker app | `docker build --build-arg FFMPEG_IMAGE=local/ffmpeg-svtav1:9.0.1-svt4.2.0-vmaf3.2.0 -t av1webconverter:local .` | `pass` | 2026-08-18, audit finale |
| audit frontend/YAML/diff | `node --check ...; python3 ...; git diff --check` | `pass` | 2026-08-18, audit finale |
| audit VMAF parser | `cargo test --lib ffmpeg::vmaf::` | `pass: 21` | 2026-08-18, audit finale |
| audit Docker app runtime | `docker run ... ffmpeg -version; ldd ...` | `pass: FFmpeg 9.0.1, libopus assente` | 2026-08-18, audit finale |
| audit Compose | `docker compose config` | `pass` | 2026-08-18, audit finale |
| V001 fmt | `cargo fmt --all -- --check` | `pass` | 2026-08-18, verify |
| V001 lint | `cargo clippy --all-targets -- -D warnings` | `pass` | 2026-08-18, verify |
| V001 unit | `cargo test --lib` | `pass: 117` | 2026-08-18, verify |
| V001 api | `cargo test --test api` | `pass: 24` | 2026-08-18, verify |
| V001 e2e sistema | `cargo test --test e2e_conversion` | `pass: 14` | 2026-08-18, verify; feature VMAF usa helper di skip se toolchain sistema incompleta |
| V001 e2e FFmpeg 9 | `PATH="$PWD/target/ffmpeg9/bin:$PATH" cargo test --test e2e_conversion` | `pass: 14` | 2026-08-18, verify |
| V001 build release | `cargo build --release` | `pass` | 2026-08-18, verify |
| V001 frontend/YAML/diff | `node --check ...; python3 ...; git diff --check` | `pass` | 2026-08-18, verify |
| V001 Docker | build immagini FFmpeg/applicativa | `pass` | 2026-08-18, verify |
| T-AN1 | unit | `DONE` | griglia CRF +2 e scarto oltre 63 |
| T-AN2 | unit | `DONE` | segmenti ripetibili, non sovrapposti, ridotti e testa/coda escluse |
| T-AN3 | unit | `DONE` | scelta massimo CRF sopra target e motivi di fallback |
| T-AN4 | unit | `DONE` | sonda con taglio, senza audio e stessi parametri video |
| T-AN5 | unit | `DONE` | riferimento tagliato prima del secondo input |
| T-AN6 | unit | `DONE` | modalità analisi conserva tune e ha un solo passaggio di codifica |
| T-AN8 | unit | `DONE` | JobFile senza `vmaf_analysis` deserializzabile |
| T-AN7 | unit | `DONE` | conteggio sonde saturato a 255 e nessuna sonda fuori analisi |

**Failing output (verbatim, trimmed to the error):**
```
none
```

## 8. Runtime deviations from the plan

| # | Plan said | What was done | Why | Impact on later phases |
|---|-----------|---------------|-----|------------------------|
| 1 | T-IMG3 iniziale | il modello VMAF v1 non inizializzava `Speed_chroma_feature_speed_chroma_uv_score` | libvmaf era compilato con `enable_float=false` | D30 e `enable_float=true` applicati; T-IMG3 ripetuto con successo |
| 2 | precondition fase 4 | fase 3 non risultava conclusa in STATE e i gate FFmpeg 9 della fase 3 non erano registrati come eseguiti | richiesta esplicita di implementare fase 4 nella sessione corrente | fase 3 completata retroattivamente prima di fase 5 |
| 3 | T-VMAF1 con `make_test_video` esistente | il clip 320x240 non inizializza il modello VMAF v1 con Speed Chroma | libvmaf richiede dimensioni minime per il modello; T-IMG3 usa già 640x360 | helper `make_test_video` ora genera 640x360; nessun impatto sul contratto applicativo |
| 4 | verifica manuale su clip di circa 90 s | usato clip reale di 70 s già disponibile, con 2 segmenti e 2 step | sufficiente a esercitare 4 sonde, persistenza e log; durata riproducibile | numeri di costo documentati sono riferiti a 70 s |
| 5 | T-AN10/T-AN12 iniziali | T-AN10 ora usa 3 segmenti e 3 step; T-AN12 usa gli stessi parametri | allineamento allo scenario B e D31 | e2e finale più costoso |
| 6 | clip da 90 s in scenario B | clip portato a 120 s | 90 s non contiene 3 segmenti non sovrapposti dopo esclusione del 10% totale | acceptance aggiornata con D32 |
| 7 | misura senza punteggio medio | ora produce `vmaf.error` invece di successo nullo | un risultato senza media non è una misura valida | comportamento documentato e test unitario |
| 8 | phase_02 §2.2 diceva di modificare solo il commento dello script | default ripristinato a `fabiop85/ffmpeg-netflix:latest` | correction plan V001 richiedeva coerenza col piano | finding V001-F03 chiuso |
| 9 | R2 includeva fonte secondaria 9to5Linux | rimossa dopo 403 Lychee; resta URL ufficiale FFmpeg | CI Lychee rifiutava fonte secondaria | B-A001 chiuso |
| 10 | D18 diceva durata non configurabile | durata resa configurabile su richiesta utente | necessario propagare campo attraverso config/API/UI/worker | T-A001 chiuso |

## 9. Blockers and open questions

- nessun BLOCKER aperto; `.serena/` resta non tracciata e finding V001-F04 è
  ACCEPTED su richiesta utente; V001-F01/F02/F03/F05 e B-A001 sono chiusi.

## 10. Do-not-repeat

- Non usare `vmaf_v1.0.16` come nome di modello: in libvmaf 3.2.0 i modelli v1
  built-in esistono solo con il suffisso di distanza di visione
  (`vmaf_v1.0.16_5d0h` per il telefono, `vmaf_v1.0.16_3d0h` per il televisore a
  1080p, e le varianti 4K e `_hfr_`). Verificato in `libvmaf/src/model.c:78-150`.
- Non misurare riportando l'uscita a 1080p per applicare le soglie di questo
  piano: un 480p confrontato a 1080p resta intorno a 75-80 qualunque sia il CRF,
  e il criterio di soglia diventerebbe inutilizzabile. Il confronto va fatto alla
  risoluzione di uscita (D12).
- Non nascondere i campi per modalità con il meccanismo attuale a modalità
  singola: `crf` e `svtav1_params` servono anche in modalità di analisi, quindi
  `applyModeVisibility` va esteso a più modalità per campo (§ 7.2), altrimenti
  attivando l'analisi spariscono campi necessari.
- Non aggiungere il crate `rand` per scegliere i segmenti: l'entropia arriva dai
  byte dell'UUID del file (D24).
- Non provare a ottenere libopus 1.5+ passando la base a `debian:trixie-slim`:
  cambierebbe la glibc fra lo stage builder dell'immagine ffmpeg e lo stage
  runtime dell'immagine applicativa, obbligando a spostare entrambe le basi e
  l'immagine Rust. Scelto invece opus statico (D2).
- Non usare `AV1WC_FFMPEG_BIN` per far girare gli e2e con i binari nuovi: gli
  helper in `tests/common/mod.rs` invocano `ffmpeg` dal `PATH`, quindi la
  variabile non ha effetto sulla preparazione dei clip di prova.
