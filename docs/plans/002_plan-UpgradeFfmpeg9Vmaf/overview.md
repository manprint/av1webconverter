# Aggiornamento toolchain FFmpeg 9, misura VMAF e modalità di analisi — Plan Overview

> **Status:** completed | **Authored:** 2026-08-18 da `agent:gpt5.6-luna`
> **Folder:** `docs/plans/002_plan-UpgradeFfmpeg9Vmaf/`
> **Stai eseguendo questo piano? Leggi [STATE.md](STATE.md) PRIMA di ogni altro
> file** — contiene posizione corrente, ambiente, lavoro in sospeso e prossima
> azione. Va aggiornato dopo ogni sotto-fase.

## Goal

Portare l'immagine ffmpeg custom da FFmpeg 8.1.2 a 9.0.1, libvmaf da 3.1.0 a
3.2.0 e dav1d da 1.5.3 a 1.5.4 (SVT-AV1 resta 4.2.0, già ultima release),
compilando libopus 1.6.1 in statico per eliminare l'unica dipendenza dinamica
del binario ffmpeg. Verificare che nessun comportamento di conversione
regredisca. Sfruttare libvmaf 3.2.0 su due livelli: una misura di qualità VMAF
opzionale sui file convertiti, e una nuova modalità di codifica **Analisi VMAF +
Qualità Costante** che sceglie automaticamente, per ogni file, il CRF più alto
che mantiene la qualità sopra una soglia, sondando spezzoni casuali del
sorgente. Senza attivare le nuove funzioni, il comportamento resta identico a
oggi.

```
docker build -f Dockerfile.ffmpeg-svtav1 -t local/ffmpeg-svtav1:9.0.1-svt4.2.0-vmaf3.2.0 .
docker build -t av1webconverter:local .
docker compose up -d

# Scenario A — misura sul file convertito
# POST /api/jobs  { params: { measure_vmaf: true, scale_height: 720 } }
GET /api/jobs/{id} →
  files[0].vmaf.model == "vmaf_v1.0.16_5d0h"
  0 < files[0].vmaf.score <= 100
  MP4 identico byte per byte allo stesso job con measure_vmaf=false

# Scenario B — scelta automatica del CRF
# POST /api/jobs  { params: { mode: "vmaf_analysis", crf: 40, scale_height: 480,
#                             vmaf_segments: 3, crf_test_steps: 3, vmaf_target: 93.0 } }
# upload di un clip di 120 s, POST /api/jobs/{id}/start
GET /api/jobs/{id} →
  files[0].status == "completed"
  files[0].vmaf_analysis.risultati ha 3 righe: CRF 42, 44, 46 con VMAF medio su 3 segmenti
  files[0].vmaf_analysis.crf_scelto è il CRF più alto con VMAF medio >= 93
  il log del file contiene la tabella dei CRF provati e la riga "CRF scelto: <n>"
  l'MP4 finale è codificato con quel CRF
```

## Design decisions

| # | Decision | Consequence |
|---|----------|-------------|
| **D1** (user, Q1) | FFmpeg target 9.0.1, ultima patch della serie 9.0 | `ARG FFMPEG_VERSION=9.0.1` + nuovo sha256 (R1); audit rimozioni 9.0 obbligatorio (R2) |
| **D2** (user, Q2) | libopus 1.6.1 compilato statico nel builder | nuovo step in `Dockerfile.ffmpeg-svtav1`, `libopus-dev` e `libopus0` rimossi da entrambe le immagini (I-3) |
| **D3** (user, Q3) | La misura VMAF diventa una feature dell'applicazione | nuovi parametri, nuovo campo persistito su `JobFile`, nuovo passaggio nel worker, superficie API e frontend |
| **D4** (user, Q4) | Verifica e2e completa contro i binari della nuova immagine | fase 3 estrae ffmpeg/ffprobe in `target/ffmpeg9/bin` e riesegue la suite con quel `PATH` |
| **D5** | Tag immagine `9.0.1-svt4.2.0-vmaf3.2.0`; dav1d solo nella label OCI | schema tag invariato, si aggiornano solo i numeri (fase 2) |
| **D6** | Base `debian:bookworm-slim` (digest pinnato) e `rust:1.96-bookworm` invariate | con opus statico non serve trixie; nessun cambio di glibc fra stage |
| **D7** | Misura VMAF opt-in per job: `measure_vmaf` default `false` | il comportamento esistente non cambia per chi non la chiede |
| **D8** (user) | Modello VMAF di default `vmaf_v1.0.16_5d0h`, cioè il modello **telefono** di VMAF v1 | è la condizione di visione reale del materiale prodotto (480p/720p su cellulare); tara DLM, AIM e croma per la minore visibilità degli artefatti e produce punteggi più alti dello standard, quindi concede CRF più alti a parità di qualità percepita (R16) |
| **D9** | La misura gira dopo il rename finale, su output definitivo vs sorgente | l'MP4 pubblicato non dipende dalla misura (I-1); costa un `ffprobe` sull'output |
| **D10** | Un fallimento della misura non fa fallire il file | stato resta `completed`, `vmaf.error` valorizzato (I-5) |
| **D11** | La misura è un passaggio extra nel modello di avanzamento | `Progress` resta invariato come forma; il frontend mostra già `passaggio X/Y` |
| **D12** | Il confronto avviene **alla risoluzione di uscita**: il riferimento viene ridimensionato alla geometria del distorto quando differisce, con `format=<pixel_format>` su entrambi i rami | il punteggio misura la perdita da compressione, non quella da ridimensionamento: è il segnale corretto per scegliere il CRF, mentre un confronto a 1080p darebbe 75-80 per qualunque CRF a 480p e nessuna soglia sarebbe utilizzabile. Va documentato nel README come caveat di interpretazione (R12, R17) |
| **D13** | I thread di libvmaf vengono dal parametro `cpus` esistente | nessun nuovo parametro di concorrenza |
| **D14** | `JobFile.vmaf` e `JobFile.vmaf_analysis` sono `Option<...>` con `#[serde(default)]` | i JSON dei job già su disco restano deserializzabili (I-4) |
| **D15** | SVT-AV1 resta 4.2.0 | nessuna modifica, nessun rischio su `tune=5` |
| **D16** (user) | Nuova modalità di codifica `vmaf_analysis`, etichetta "Analisi VMAF + Qualità Costante", terza voce di `EncodingMode` | alternativa a `crf` e `target_size`, non combinabile con `target_size`; è una codifica CRF a un passaggio preceduta da una fase di sondaggio |
| **D17** (user) | Parametri della modalità: `vmaf_segments` (default 3), `crf_test_steps` (default 3), `vmaf_target` (default 93.0); il `crf` impostato dall'utente è il punto di partenza | tre campi nuovi, visibili nell'interfaccia solo con la modalità attiva |
| **D18** (user) | Durata dei segmenti fissa a 30 secondi, costante di codice non configurabile | un parametro in meno da validare e documentare; il numero di segmenti resta la leva |
| **D19** (user, matrice) | Ogni CRF viene provato su **tutti** i segmenti e il punteggio del CRF è la media dei segmenti | `vmaf_segments × crf_test_steps` sonde per file (con 3 e 3: 9 codifiche brevi più 9 misure); un segmento atipico non decide da solo |
| **D20** (user) | CRF provati: `crf+2`, `crf+4`, …, `crf+2·crf_test_steps`; il CRF impostato non viene provato | con CRF 40 e 3 step si provano 42, 44, 46; gli step che superano 63 (massimo di SVT-AV1) vengono scartati con una riga di log |
| **D21** (user, criterio) | Si sceglie il **CRF più alto** con VMAF medio `>= vmaf_target`. Se nessuno rispetta la soglia si usa il CRF impostato dall'utente; se la rispettano tutti si usa il più alto provato e il log invita ad aumentare gli step | scelta deterministica e spiegabile, sempre tracciata nel log con il motivo |
| **D22** (user, preset) | Le sonde usano gli stessi `preset`, `svtav1_params`, `pixel_format` e `scale_height` della codifica finale, senza audio | il CRF trovato è direttamente trasferibile; a preset molto bassi l'analisi è costosa, e va documentato |
| **D23** (user, segmenti) | Segmenti casuali, non sovrapposti, esclusi il primo e l'ultimo 5% della durata; se la durata non regge il numero richiesto il numero viene ridotto con riga di log; sotto i 30 s si analizza il file intero come segmento unico | evita sigle, nero e titoli di coda; nessun sondaggio fuori dalla durata |
| **D24** | La casualità dei segmenti deriva dai byte dell'UUID del file, non da una nuova dipendenza | `uuid` v4 è già in uso e i suoi byte sono casuali: nessun crate aggiuntivo, e ripetere l'analisi dello stesso file dà gli stessi segmenti (utile a spiegare un risultato) |
| **D25** | Avanzamento in modalità analisi: `total_passes = sonde·2 + passaggi della codifica finale` (una sonda conta come una codifica più una misura) | il frontend mostra `passaggio X/Y` senza modifiche; le righe di log rendono leggibile a che punto è l'analisi |
| **D26** | L'esito dell'analisi è persistito in `JobFile.vmaf_analysis` con la tabella completa dei CRF provati | l'interfaccia e i log possono mostrare *perché* è stato scelto quel CRF, e l'analisi non va rifatta per capirlo |
| **D27** | L'analisi è per file, non condivisa fra i file di un job | ogni file ha materiale e durata propri; condividere il CRF falserebbe il risultato |
| **D28** | `effective_svtav1_params()` tratta `vmaf_analysis` come `crf` (nessun `tune=0` forzato) | la nuova modalità è una codifica CRF, quindi `tune=5` resta valido; solo `target_size` continua a forzare `tune=0` |
| **D29** (user) | Esecutore unico: `agent:gpt5.6-luna` per ogni fase e sotto-fase | non esiste un secondo agente che revisiona: i punti che prevedevano una revisione esterna diventano autorevisioni esplicite, elencate nella tabella finale e nel campo **Assignment** delle sotto-fase interessate |
| **D30** (runtime, 2026-08-18) | Abilitare `enable_float=true` nella build libvmaf | I modelli VMAF v1 usano feature floating-point; senza questa opzione T-IMG3 fallisce con `Speed_chroma_feature_speed_chroma_uv_score` non inizializzabile |
| **D31** (runtime, 2026-08-18) | T-AN10 usa 3 segmenti e 3 CRF (`42,44,46`) | il test e2e esercita scenario B completo; T-AN11 e T-AN12 coprono separatamente file corto e fallback |
| **D32** (runtime, 2026-08-18) | Acceptance T-AN10/T-AN12 usa clip da 120 s invece di 90 s | con D23 (esclusione testa/coda 5% e segmenti non sovrapposti) 90 s offre solo 81 s utili e non può contenere 3 segmenti da 30 s |

## Open questions

Nessuna: tutte le ambiguità sono state risolte dall'utente (Q1 → D1, Q2 → D2,
Q3 → D3, Q4 → D4, criterio → D21, matrice → D19, preset delle sonde → D22,
scelta dei segmenti → D23, modello telefono → D8).

## Architecture summary

`Dockerfile.ffmpeg-svtav1` resta l'unica fonte di verità per versioni e
checksum. Sul lato applicativo, `src/ffmpeg/vmaf.rs` diventa il modulo unico
per VMAF: costruisce gli argomenti del filtro `libvmaf` (due input, log JSON) e
interpreta `pooled_metrics.vmaf`. La misura di fine conversione lo usa
sull'output definitivo; la modalità di analisi lo usa sulle sonde. La modalità
di analisi vive nel worker come fase che precede la codifica: sceglie i
segmenti, per ogni CRF candidato codifica ogni segmento e lo misura, media i
risultati, applica il criterio di soglia, scrive `JobFile.vmaf_analysis` e passa
il CRF scelto al normale percorso di codifica CRF.

## Interface

| Surface | Name | Type / values | Default | Notes |
|---------|------|---------------|---------|-------|
| config `[encoding]` / params job | `mode` | `crf` \| `target_size` \| `vmaf_analysis` | `crf` | nuova terza voce; etichetta nell'interfaccia "Analisi VMAF + Qualità Costante" |
| config `[encoding]` / params job | `measure_vmaf` | `bool` | `false` | misura sul file convertito; indipendente dalla modalità |
| config `[encoding]` / params job | `vmaf_model` | `string` | `vmaf_v1.0.16_5d0h` | modello telefono di VMAF v1; validato con `is_safe_ffmpeg_token`. Altri nomi built-in: `vmaf_v1.0.16_3d0h`, `vmaf_v1.0.16_3d0h_2160`, `vmaf_v1.0.16_1d5h_2160`, le quattro varianti `_hfr_`, `vmaf_v0.6.1`, `vmaf_v0.6.1neg`, `vmaf_b_v0.6.3`, `vmaf_4k_v0.6.1`, `vmaf_4k_v0.6.1neg` (R11) |
| config `[encoding]` / params job | `vmaf_segments` | `u32`, 1–10 | `3` | interfaccia: "VMAF Segments"; solo in modalità `vmaf_analysis` |
| config `[encoding]` / params job | `crf_test_steps` | `u32`, 1–10 | `3` | interfaccia: "CRF Test Steps"; solo in modalità `vmaf_analysis` |
| config `[encoding]` / params job | `vmaf_target` | `f64`, 0–100 | `93.0` | interfaccia: "VMAF Target"; soglia minima di qualità media |
| env | `AV1WC_MEASURE_VMAF`, `AV1WC_VMAF_MODEL`, `AV1WC_VMAF_SEGMENTS`, `AV1WC_CRF_TEST_STEPS`, `AV1WC_VMAF_TARGET` | come i rispettivi campi | — | override dei valori di config |
| API `GET /api/jobs`, `GET /api/jobs/{id}` | `files[].vmaf` | `object|null` | `null` | `{ score, min, harmonic_mean, model, duration_seconds, error }` |
| API `GET /api/jobs`, `GET /api/jobs/{id}` | `files[].vmaf_analysis` | `object|null` | `null` | `{ crf_scelto, crf_di_partenza, target, modello, motivo, segmenti[], risultati[], duration_seconds, error }` |
| immagine ffmpeg | tag | `9.0.1-svt4.2.0-vmaf3.2.0` | — | sostituisce `8.1.2-svt4.2.0-vmaf3.1.0` |

## Protocol and data-structure changes

| Change | Shape | Backward-compat strategy |
|--------|-------|--------------------------|
| `JobFile.vmaf` | `Option<VmafMeasurement>`; `{ score: Option<f64>, min: Option<f64>, harmonic_mean: Option<f64>, model: String, duration_seconds: f64, error: Option<String> }` | `#[serde(default)]`: i job già su disco si caricano con `vmaf: None` (T-VMAF4) |
| `JobFile.vmaf_analysis` | `Option<VmafAnalysis>`; `{ crf_scelto: u8, crf_di_partenza: u8, target: f64, modello: String, motivo: MotivoScelta, segmenti: Vec<SegmentoAnalisi>, risultati: Vec<RisultatoCrf>, duration_seconds: f64, error: Option<String> }` con `SegmentoAnalisi { indice: u32, inizio_secondi: f64, durata_secondi: f64 }`, `RisultatoCrf { crf: u8, vmaf_medio: Option<f64>, bitrate_medio_bps: Option<u64>, vmaf_per_segmento: Vec<Option<f64>> }`, `MotivoScelta` serializzato snake_case (`sopra_soglia`, `tutti_sopra_soglia`, `nessuno_sopra_soglia`, `analisi_non_eseguibile`) | `#[serde(default)]`: assente sui job precedenti e su quelli non in modalità analisi (T-AN8) |
| `EncodingMode` acquisisce la variante `VmafAnalysis` (`vmaf_analysis`) | enum serializzato snake_case | variante aggiuntiva: i job salvati con `crf`/`target_size` restano validi. Ogni `match` su `EncodingMode` va aggiornato: `src/config.rs:139` (`effective_svtav1_params`, D28), `src/ffmpeg/encode.rs:218` (`total_passes`), `src/worker.rs:245` (scelta degli argomenti) |
| `EncodingParams` + cinque campi | `measure_vmaf`, `vmaf_model`, `vmaf_segments`, `crf_test_steps`, `vmaf_target` | le struct di config hanno `serde(default)`: TOML e JSON parziali restano validi; `deny_unknown_fields` impone di aggiornare `config.example.toml` |
| `Progress.total_passes` può valere `sonde·2 + passaggi finali` | forma invariata (`u8`) | il frontend mostra già `passaggio X/Y` (`src/web/app.js:353`). Con 10 segmenti e 10 step il conto arriva a 201: il campo resta `u8` ma va saturato a 255 in modo esplicito |
| tag dell'immagine ffmpeg | `9.0.1-svt4.2.0-vmaf3.2.0` | il vecchio tag resta pubblicato; `FFMPEG_IMAGE` è un `ARG` |

## Phases

| Phase | File | Primary assignment | Shippable alone? |
|-------|------|--------------------|------------------|
| 1 — Immagine: FFmpeg 9.0.1, libvmaf 3.2.0, dav1d 1.5.4, opus statico | [phase_01.md](phase_01.md) | `agent:gpt5.6-luna` | sì |
| 2 — Propagazione tag e riferimenti di versione | [phase_02.md](phase_02.md) | `agent:gpt5.6-luna` | sì |
| 3 — Verifica di regressione con la nuova toolchain | [phase_03.md](phase_03.md) | `agent:gpt5.6-luna` | sì |
| 4 — Misura VMAF: backend | [phase_04.md](phase_04.md) | `agent:gpt5.6-luna` | sì |
| 5 — Misura VMAF: API, frontend, end-to-end | [phase_05.md](phase_05.md) | `agent:gpt5.6-luna` | sì |
| 6 — Modalità Analisi VMAF + Qualità Costante: backend | [phase_06.md](phase_06.md) | `agent:gpt5.6-luna` | sì |
| 7 — Modalità di analisi: API, frontend, end-to-end | [phase_07.md](phase_07.md) | `agent:gpt5.6-luna` | sì |

## Reuse map (top candidates)

| Need | Reuse | Location |
|------|-------|----------|
| Argomenti comuni ffmpeg (`-progress`, `-nostdin`, `-stats_period`) | `common_input_args` | `src/ffmpeg/encode.rs:50` |
| Argomenti della codifica video (filtro scala, preset, svtav1-params, pix_fmt) | `video_encoder_args` | `src/ffmpeg/encode.rs:72` |
| Codifica CRF completa | `build_crf_args` | `src/ffmpeg/encode.rs:93` |
| Esecuzione ffmpeg con progresso e cancellazione | `run_ffmpeg` | `src/ffmpeg/encode.rs:237` |
| Esecuzione con pump di eventi verso frontend e log | `run_progress_run` | `src/worker.rs:353` |
| Numero di passaggi per modalità | `total_passes` | `src/ffmpeg/encode.rs:218` |
| Parser di `-progress` | `ProgressParser::new(total_seconds, total_frames, pass, total_passes)` | `src/ffmpeg/progress.rs:25` |
| Geometria, durata e frame stimati di un file | `probe` | `src/ffmpeg/probe.rs:27` |
| Validazione dei valori che finiscono sulla riga di comando | `is_safe_ffmpeg_token` | `src/config.rs:167` |
| Punto di innesto della misura finale | `convert_file` dopo il rename | `src/worker.rs:317-348` |
| Punto di innesto dell'analisi (prima del ciclo dei passaggi) | `convert_file` | `src/worker.rs:236-245` |
| Riga di log verso frontend e file di log | `log_line` | `src/worker.rs:535` |
| Cartella temporanea del job e pulizia su errore | `state.temp_dir()`, `TempFiles` | `src/worker.rs:236`, `src/worker.rs:565` |
| Byte casuali già disponibili senza nuove dipendenze | `Uuid::as_bytes` del `file_id` | `src/models.rs:72` |
| Gate e2e sulla presenza del codec | `ffmpeg_with_svtav1` | `tests/common/mod.rs:94` |
| Attesa dello stato di un job nei test | `wait_for_job_status` | `tests/common/mod.rs:228` |
| Campi del form parametri e visibilità per modalità | `PARAM_FIELDS`, `applyModeVisibility` | `src/web/app.js:13-52`, `src/web/app.js:193` |

## References (external documentation consulted)

| # | What it settled | Source | Version / date |
|---|-----------------|--------|----------------|
| R1 | FFmpeg 9.0.1 è l'ultima release; tarball `ffmpeg-9.0.1.tar.xz`, sha256 `cf38e0e28c7e5605942c4a77755349b0145804a397af37eb1fb4c77cb237f635` | https://ffmpeg.org/releases/ | 9.0.1 |
| R2 | Rimozioni della 9.0: `-psnr`, `-map_channel`, filtri NPP, preset NVENC legacy e SDK < 11.1, decoder CELT, vecchio handler HLS, codec v308/v408/v410, bump major delle `libav*`; verifica TLS attiva per default | https://github.com/FFmpeg/FFmpeg/blob/release/9.0/Changelog, https://9to5linux.com/ffmpeg-9-0-lei-open-source-multimedia-framework-officially-released | 9.0 "Lei" |
| R3 | Requisiti minimi in FFmpeg 9.0.1: `dav1d >= 1.0.0` (configure:7334), `SvtAv1Enc >= 0.9.0` (configure:7443), `libvmaf >= 2.0.0` (configure:7458) | tarball 9.0.1, `configure` | 9.0.1 |
| R4 | `-progress` (fftools/ffmpeg_opt.c:1650) e `-stats_period` (:1779) esistono ancora; chiavi emesse invariate: `frame`, `total_size`, `bitrate`, `out_time_us`, `out_time_ms`, `speed`, `progress` (fftools/ffmpeg.c:671-679) | tarball 9.0.1 | 9.0.1 |
| R5 | `reset_sar` del filtro `scale` è ancora presente | tarball 9.0.1, `libavfilter/vf_scale.c` | 9.0.1 |
| R6 | Il wrapper libsvtav1 espone ancora `preset`, `crf`, `qp`, `svtav1-params`; default `b=0`, `flags=+cgop`, `g=-1`, `qmin=1`, `qmax=63` | tarball 9.0.1, `libavcodec/libsvtav1.c:793-850` | 9.0.1 |
| R7 | dav1d 1.5.4 "Sonic": ottimizzazioni, tile task schedulati per tutti i passaggi, tabelle di quantizzazione precalcolate; nessuna rottura di API o opzioni meson. sha256 `686616b7c69eb88d44459391ab25cac13b6647a3b288835c5784e71c1514a5c5` | https://code.videolan.org/videolan/dav1d/-/raw/master/NEWS, https://download.videolan.org/pub/videolan/dav1d/1.5.4/ | 1.5.4 |
| R8 | libvmaf 3.2.0 (2026-06-20): prima release pubblica dei modelli VMAF v1, feature `motion_v2`, aggiornamenti cambi/adm/vif. sha256 `a28f93f3b4fa65601be324587072e32a6a704a304ba7b1aec9b70b3f709bc1dc` | https://github.com/Netflix/vmaf/releases/tag/v3.2.0 | 3.2.0 |
| R9 | libvmaf 3.2.0 richiede meson >= 0.56.1 e mantiene `enable_tests`, `enable_docs`, `enable_tools`, `built_in_models` | tarball 3.2.0, `libvmaf/meson.build:9`, `libvmaf/meson_options.txt` | 3.2.0 |
| R10 | dav1d 1.5.4 richiede meson >= 0.54.0 e mantiene `enable_tools`, `enable_tests` | tarball 1.5.4, `meson.build:31` | 1.5.4 |
| R11 | Nomi `version` dei modelli built-in in libvmaf 3.2.0, fra cui `vmaf_v1.0.16_3d0h`, `vmaf_v1.0.16_5d0h`, `vmaf_v1.0.16_1d5h_2160`, `vmaf_v1.0.16_3d0h_2160` e le varianti `_hfr_`; il nome nudo `vmaf_v1.0.16` non esiste | tarball 3.2.0, `libvmaf/src/model.c:78-150` | 3.2.0 |
| R12 | Il filtro `libvmaf` accetta `yuv420p/422p/444p` a 8/10/12/16 bit (`vf_libvmaf.c:88-101`), sincronizza due input via framesync; opzioni `model` (default `version=vmaf_v0.6.1`), `log_fmt` (default `xml`), `n_threads` (`vf_libvmaf.c:74-78`) | tarball 9.0.1 | 9.0.1 |
| R13 | SVT-AV1 4.2.0 è l'ultimo tag pubblicato | API tag di https://gitlab.com/AOMediaCodec/SVT-AV1 | 2026-08-18 |
| R14 | Schema del log JSON di libvmaf: `version`, `fps`, `frames[].metrics`, `pooled_metrics.<metrica>.{min,max,mean,harmonic_mean}`, `aggregate_metrics` | tarball 3.2.0, `libvmaf/src/output.c:150-300` | 3.2.0 |
| R15 | libopus 1.6.1 è l'ultima release upstream; sha256 `6ffcb593207be92584df15b32466ed64bbec99109f007c82205f0194572411a1`; il tarball contiene `configure` e supporta `--disable-shared --enable-static --disable-doc --disable-extra-programs`; FFmpeg richiede solo il pkg-config `opus` (configure:7419) | https://downloads.xiph.org/releases/opus/ | 1.6.1 |
| R16 | `vmaf_v1.0.16_5d0h` è il **modello telefono** di VMAF v1: distanza di visione normalizzata a 5H, con DLM, AIM e croma tarati sulla minore visibilità degli artefatti su schermo piccolo, intervallo [0, 100]. Il modello telefono produce punteggi più alti di quello standard sullo stesso file, perché a quella condizione di visione gli artefatti si vedono meno | tarball 3.2.0, `resource/doc/models_v1.md:12,23-26`, `resource/doc/models_v0.md:13-30` | 3.2.0 |
| R17 | I modelli VMAF sono tarati su sorgenti riportati a 1080p e visti a distanza fissa: un confronto fatto alla risoluzione nativa dell'uscita misura la sola perdita da compressione, ed è per questo che le soglie assolute di questo piano si applicano a quel confronto | tarball 3.2.0, `resource/doc/models_v0.md:7`, `resource/doc/faq.md:9` | 3.2.0 |

## Invariants

- **I-1:** l'MP4 pubblicato è indipendente dalla misura VMAF: con gli stessi
  parametri e lo stesso sorgente, `measure_vmaf=true` e `false` producono lo
  stesso file byte per byte.
- **I-2:** la riga di comando di conversione non cambia con l'aggiornamento:
  `-progress pipe:1`, `-stats_period`, `-nostdin`, `-nostats`,
  `scale=...:reset_sar=1`, `-svtav1-params`, `-tag:v av01`,
  `-movflags +faststart`, `-pass`/`-passlogfile` restano validi (R4, R5, R6).
- **I-3:** il binario ffmpeg dell'immagine non ha dipendenze dinamiche verso
  libopus.
- **I-4:** i job già persistiti (JSON senza `vmaf` né `vmaf_analysis`) restano
  caricabili.
- **I-5:** un fallimento della misura VMAF non cambia lo stato del file: resta
  `completed`.
- **I-6:** la cancellazione del job interrompe anche i passaggi di misura e le
  sonde di analisi.
- **I-7:** con `mode` diverso da `vmaf_analysis` non viene eseguita nessuna
  sonda e il percorso di conversione è identico a quello odierno, sonda per
  sonda assente: nessuna chiamata a ffmpeg in più.
- **I-8:** in modalità `vmaf_analysis` la codifica finale usa esattamente il CRF
  registrato in `vmaf_analysis.crf_scelto`, e quel valore compare nel log del
  file.
- **I-9:** l'analisi non tocca il sorgente: i segmenti sono letti con `-ss`/`-t`
  sull'input e tutte le sonde vivono nella cartella temporanea, rimossa dal
  `Drop` guard.

## Risk register

| Risk | Mitigation |
|------|-----------|
| Una rimozione della 9.0 tocca una nostra opzione | audit esplicito contro R2 in 1.3; I-2 coperto da T-IMG2 e dalla suite e2e |
| opus statico non trovato da `configure` | `PKG_CONFIG_PATH` già impostato; T-IMG1 verifica l'encoder `libopus` |
| Rimuovere `libopus0` rompe il runtime | T-IMG1 verifica con `ldd` prima che la fase 2 tocchi il Dockerfile applicativo |
| Il modello telefono non è caricabile nella build | T-IMG3 lo prova nella fase 1, prima che diventi il default in fase 4 |
| L'analisi costa troppo tempo su preset bassi | sonde limitate a 30 s, numero di sonde deciso dall'utente, tempo totale registrato in `vmaf_analysis.duration_seconds`, costo documentato nel README con un esempio numerico |
| Il file è più corto della finestra di analisi | D23: numero di segmenti ridotto, o segmento unico pari al file; T-AN11 lo prova su un clip di 10 s |
| Tutti i CRF provati superano la soglia, quindi la scelta è al limite della griglia | D21: si prende il più alto e il log invita ad aumentare gli step; il motivo è persistito in `vmaf_analysis.motivo` |
| Il CRF scelto su spezzoni non rappresenta il file intero | D19 media su più segmenti, D23 esclude sigle e coda; la tabella per segmento resta visibile per capire la varianza |
| `Progress.total_passes` è `u8` e con molte sonde può saturare | conteggio saturato in modo esplicito a 255 e verificato da un test unitario (T-AN7) |
| Mismatch di geometria o pixel format fra sonda e riferimento | D12 e `format=<pixel_format>` su entrambi i rami; T-VMAF7 su sorgente anamorfico, T-AN10 su sorgente ridimensionato |

## Verification summary

| Gate | Command | Where it runs |
|------|---------|---------------|
| fmt | `cargo fmt --all -- --check` | ogni fase che tocca codice Rust (3–7) |
| lint | `cargo clippy --all-targets -- -D warnings` | ogni fase che tocca codice Rust |
| unit | `cargo test --lib` | fasi 4–7 |
| API | `cargo test --test api` | fasi 4–7 |
| e2e (ffmpeg di sistema) | `cargo test --test e2e_conversion` | fasi 3–7 |
| e2e (binari della nuova immagine) | `PATH="$PWD/target/ffmpeg9/bin:$PATH" cargo test --test e2e_conversion` | fase 3, ripetuto in fasi 5 e 7 |
| build immagine ffmpeg | `docker build -f Dockerfile.ffmpeg-svtav1 -t local/ffmpeg-svtav1:9.0.1-svt4.2.0-vmaf3.2.0 .` | fase 1 |
| build immagine applicativa | `docker build --build-arg FFMPEG_IMAGE=local/ffmpeg-svtav1:9.0.1-svt4.2.0-vmaf3.2.0 -t av1webconverter:local .` | fasi 2 e 3 |

**Acceptance:** lo scenario A è provato da T-IMG1 (immagine con FFmpeg 9.0.1,
libsvtav1, libopus, libdav1d, libvmaf e nessuna dipendenza dinamica verso
libopus), T-E2E-ALL (suite e2e verde con i binari nuovi) e T-VMAF1 (punteggio in
`(0,100]` con modello `vmaf_v1.0.16_5d0h` e MP4 identico byte per byte senza
misura). Lo scenario B è provato da T-AN10 (job in modalità analisi: tre CRF
provati su tre segmenti, `crf_scelto` conforme al criterio di soglia, log con la
tabella e la riga del CRF scelto, output codificato con quel CRF) e T-AN3
(criterio di scelta nei tre casi: misto, tutti sopra, nessuno sopra).

**Run caveats:** le build docker richiedono rete e alcuni minuti; la suite e2e
richiede un ffmpeg con `libsvtav1` (si auto-salta altrimenti,
`tests/common/mod.rs:94`) e le prove VMAF un ffmpeg con `libvmaf` (gate
`ffmpeg_with_libvmaf`). I test della modalità di analisi usano preset alti e
clip brevi per restare sotto il minuto. Gli e2e con i binari nuovi vanno
lanciati con `PATH` prefissato, non con `AV1WC_FFMPEG_BIN`, perché gli helper di
test invocano `ffmpeg` dal `PATH`.

Questi comandi sono gli stessi scritti in `STATE.md` §3: non devono divergere.

## Model-assignment summary

Esecutore unico: **`agent:gpt5.6-luna`**. Lo stesso agente esegue ogni fase e
ogni sotto-fase; dove il piano prevedeva una revisione di un altro agente, quel
punto diventa una **autorevisione esplicita**, da svolgere come passaggio
separato prima di dichiarare la sotto-fase conclusa (rileggere il lavoro appena
scritto contro il campo **Change** e contro le invarianti citate, non solo
constatare che i gate sono verdi).

| Phase | Sub-phases | Assegnate a | Autorevisioni esplicite |
|-------|------------|-------------|-------------------------|
| 1 | 1.1 – 1.4 | `agent:gpt5.6-luna` | 1.2 (rimozione della dipendenza dinamica), 1.3 (audit delle rimozioni 9.0), 1.4 (documentazione) |
| 2 | 2.1 – 2.3 | `agent:gpt5.6-luna` | 2.2 (nota falsa in `CLAUDE.md` e in `docs/architettura.md`) |
| 3 | 3.1 – 3.3 | `agent:gpt5.6-luna` | 3.1 (asserzioni di accettazione della regressione) |
| 4 | 4.1 – 4.5 | `agent:gpt5.6-luna` | 4.3 (stringa di filtro e parsing del log), 4.4 (worker, cancellazione, tolleranza ai fallimenti) |
| 5 | 5.1 – 5.4 | `agent:gpt5.6-luna` | 5.3 (asserzioni e2e, identità byte per byte), 5.4 (lettura finale della documentazione) |
| 6 | 6.1 – 6.6 | `agent:gpt5.6-luna` | 6.2 (segmentazione), 6.3 (criterio di scelta del CRF), 6.4 (orchestrazione nel worker e conteggio dei passaggi) |
| 7 | 7.1 – 7.4 | `agent:gpt5.6-luna` | 7.3 (asserzioni e2e), 7.4 (lettura finale della documentazione) |
