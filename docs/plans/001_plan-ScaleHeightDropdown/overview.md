# Altezza massima a menu a tendina — Plan Overview

> **Status:** planning | **Authored:** 2026-08-18 by `agent:gpt5.6-luna`
> **Folder:** `docs/plans/001_plan-ScaleHeightDropdown/`
> **Devi eseguire questo piano? Leggi prima [STATE.md](STATE.md)** — contiene la
> posizione corrente, l'ambiente, il lavoro a metà e l'azione successiva. Esegui i
> comandi di gate elencati in `STATE.md` §3 e confrontali con quanto dichiarano §1
> e §7 prima di modificare qualunque cosa: il repo ha ragione, non il file.
> Aggiorna `STATE.md` e `resume.md` dopo ogni sub-fase.

## Goal

Il campo "Altezza massima (px)" della pagina principale, oggi un input numerico
libero, diventa un menu a tendina con nove scelte fisse: `Dimensione originale`,
144px, 240px, 320px, 480px, 720px, 1080px, 1440px, 2160px. Il default resta
480px, ereditato dal server. Quando è selezionata `Dimensione originale` la
catena di codifica non applica alcun ridimensionamento: `-vf` non viene passato
a ffmpeg e il SAR del sorgente resta intatto. La lista fissa vive solo nella UI:
l'API continua ad accettare qualunque altezza valida, per non rompere il
contratto pubblico documentato in `docs/api.md`.

```
Scenario di riferimento
1. Sorgente anamorfico 720x576, SAR != 1:1 (tests/common/mod.rs:140 make_anamorphic_video)
2. Job creato con scale_height = 0 ("Dimensione originale")
3. Conversione completata
   => output MP4 AV1 con width=720, height=576, SAR uguale a quello del sorgente
   => nessun "-vf" nella riga di comando ffmpeg
4. Lo stesso sorgente con scale_height = 480 produce ancora 854x480 SAR 1:1
```

## Design decisions

| # | Decision | Consequence |
|---|----------|-------------|
| **D1** (user, Q1) | `scale_height` resta `u32`; il valore `0` significa "dimensione originale" | Nessun cambio di tipo in serde/API/JSON persistiti; "originale" è esprimibile anche in `config.toml` (`scale_height = 0`) e in `AV1WC_SCALE_HEIGHT=0` |
| **D2** (user, Q2) | Etichetta della voce: `Dimensione originale` | Testo usato in `index.html`, in `PARAM_FIELDS` e nel riepilogo del job |
| **D3** (user, Q3) | Con `scale_height == 0` l'argomento `-vf` viene **omesso interamente** | Nessun `setsar`, nessuna normalizzazione: un anamorfico conserva dimensione **e** SAR sorgente. `scale_filter` diventa `Option<String>` |
| **D4** (user, Q4) | `validate()` accetta `0` **oppure** `16..=4320`; nessuna restrizione ai soli nove valori | L'API resta permissiva e retro-compatibile; chi invia 500 continua a funzionare |
| **D5** (user, Q5) | Un valore fuori lista (da config, env o API) genera un'opzione dinamica `<N>px` selezionata | Aprire l'editor di un job non altera silenziosamente i suoi parametri |
| **D6** (user, Q6) | Il default della form resta quello del server (`GET /api/defaults`) | `AV1WC_SCALE_HEIGHT` continua a valere; il default di fabbrica resta 480 |
| **D7** | Esecuzione in single-agent mode: `agent:gpt5.6-luna` esegue ogni sub-fase e fa self-review esplicita ai gate | Nessuna delega; ogni gate va dichiarato eseguito in `STATE.md` §7 |
| **D8** | `pub fn scale_filter(u32) -> Option<String>` (era `-> String`) e `-vf` spinto condizionalmente in `video_encoder_args` | Un solo punto di chiamata (`src/ffmpeg/encode.rs:70`) copre sia CRF sia two-pass; i due unit test esistenti sul filtro vanno aggiornati con `.unwrap()` |
| **D9** | Nel frontend il descrittore del campo porta `numeric: true`; `readParams` coerce a `Number` anche i `select` così marcati | Senza questo `scale_height` arriverebbe al backend come stringa `"480"` e serde rifiuterebbe il body con 400 |

## Open questions

nessuna — tutte le clarification sono state risolte al gate D2 (Q1..Q6 → opzione
consigliata).

## Architecture summary

Il valore attraversa tre strati senza cambiare tipo: `EncodingParams.scale_height:
u32` (config/API/JSON del job) → `scale_filter()` che ora ritorna `Option<String>`
→ `video_encoder_args()` che aggiunge `-vf` solo se il filtro esiste. Il frontend
non introduce stato nuovo: la sola aggiunta è un descrittore `select` con
coercizione numerica e la conservazione dei valori fuori lista. Nessuna
migrazione dati, nessun nuovo endpoint, nessuna nuova dipendenza.

Nessuna sub-fase crea file o cartelle nuove: tutte le modifiche sono su file già
esistenti, nelle posizioni che il repo usa già (`src/`, `src/web/`, `tests/`,
`docs/`). Se durante l'esecuzione servisse un file nuovo, è una deviazione dal
piano e va registrata in `STATE.md` §8.

## Interface

| Surface | Name | Type / values | Default | Notes |
|---------|------|---------------|---------|-------|
| UI (form nuovo job + editor job) | `Altezza massima` | `select` con 9 voci: `0` (Dimensione originale), 144, 240, 320, 480, 720, 1080, 1440, 2160 | dal server (480) | L'etichetta perde il suffisso `(px)`: le unità sono nelle singole voci. Un valore fuori lista viene aggiunto come opzione dinamica (D5) |
| API `POST/PATCH /api/jobs` | `params.scale_height` | numero JSON: `0` oppure `16..=4320` | 480 | `0` = nessun ridimensionamento. Valori 1..15 e >4320 → `400`. Una stringa `"480"` → `400` (serde) |
| Config TOML | `encoding.scale_height` | `u32` | `480` | `0` = nessun ridimensionamento |
| Env | `AV1WC_SCALE_HEIGHT` | `u32` | — | Sovrascrive il TOML; `0` ammesso |

## Protocol and data-structure changes

| Change | Shape | Backward-compat strategy |
|--------|-------|--------------------------|
| Dominio di `EncodingParams.scale_height` esteso | `u32`, aggiunto il valore ammesso `0` | Tipo invariato: i job JSON già su disco (`data_dir/jobs/<id>.json`) deserializzano senza modifiche. Nessun valore prima valido diventa invalido |
| `pub fn scale_filter` | `u32 -> Option<String>` (era `-> String`) | API interna del crate, nessun consumatore esterno. I due unit test esistenti vanno aggiornati (fase 1 § 1.3) |

## Phases

| Phase | File | Primary assignment | Shippable alone? |
|-------|------|--------------------|------------------|
| 1 — Backend: sentinella "originale" e filtro opzionale | [phase_01.md](phase_01.md) | `agent:gpt5.6-luna` | yes — il backend accetta `0` e non ridimensiona; la UI non lo offre ancora |
| 2 — Frontend: menu a tendina | [phase_02.md](phase_02.md) | `agent:gpt5.6-luna` | yes — richiede la fase 1 per avere `0` accettato dall'API |
| 3 — Integrazione, test end-to-end e documentazione | [phase_03.md](phase_03.md) | `agent:gpt5.6-luna` | yes |

## Reuse map (top candidates)

| Need | Reuse | Location |
|------|-------|----------|
| Punto unico dove nasce `-vf` per CRF e two-pass | `video_encoder_args` | `src/ffmpeg/encode.rs:65-79` |
| Costruzione del filtro di scala | `scale_filter` | `src/ffmpeg/encode.rs:38-41` |
| Validazione dei parametri lato API e config | `EncodingParams::validate` | `src/config.rs:75-78` |
| Override da ambiente | `AV1WC_SCALE_HEIGHT` | `src/config.rs:435-439` |
| Descrittori dei campi UI, condivisi da form e editor | `PARAM_FIELDS` | `src/web/app.js:13-34` (riga 27 il campo interessato) |
| Serializzazione/deserializzazione della form | `readParams` / `writeParams` | `src/web/app.js:117-141` / `:145-157` |
| Rendering `select` nell'editor per-job | `buildParamsEditor` | `src/web/app.js:207-211` |
| Riepilogo testuale del job | `renderJob` | `src/web/app.js:277` |
| Sorgente anamorfico 720x576 SAR non quadrato | `make_anamorphic_video` | `tests/common/mod.rs:140` |
| Lettura di width/height/SAR/DAR di un output | `video_geometry` | `tests/common/mod.rs:160` |
| Server di test con config mutata | `start_with` | `tests/common/mod.rs:32` |
| Helper unit test sugli argomenti ffmpeg | `as_strings` | `src/ffmpeg/encode.rs:326` |

## References (external documentation consulted)

nessuna — nessun contratto esterno nuovo. Il cambiamento consiste nell'**omettere**
`-vf`, non nell'usare opzioni ffmpeg o SVT-AV1 non già in uso nel repo. Il filtro
esistente `scale=-2:...:reset_sar=1` resta invariato per tutte le altezze `>= 16`.

## Invariants

- **I-1:** CRF e two-pass condividono lo stesso filtro video — il `-vf` nasce in
  un solo punto (`video_encoder_args`). Guardato da
  `comando_crf_omette_vf_con_altezza_originale` e
  `comando_due_passaggi_omette_vf_con_altezza_originale`.
- **I-2:** con `scale_height >= 16` gli argomenti ffmpeg sono identici a prima del
  piano. Guardato da `comando_crf_riproduce_lo_script_di_riferimento`
  (`src/ffmpeg/encode.rs:360`, asserisce `-vf scale=-2:trunc(min(480\,ih)/2)*2:reset_sar=1`)
  e da T-SH2.
- **I-3:** l'API accetta qualunque `scale_height` in `{0} ∪ 16..=4320`; la lista
  di nove valori è solo un vincolo di UI. Guardato da T-SH3 e T-SH4.
- **I-4:** aprire l'editor dei parametri di un job non modifica i suoi parametri,
  nemmeno quando `scale_height` è fuori dalla lista (D5).
- **I-5:** i job JSON già persistiti restano deserializzabili senza migrazione.

## Risk register

| Risk | Mitigation |
|------|-----------|
| `0` è un valore magico e può essere letto come "altezza zero" | Doc comment su `scale_height`, messaggio di `validate()` esplicito, riga di README; provato da `altezza_zero_significa_dimensione_originale` (fase 1) |
| Il `select` restituisce una stringa: `scale_height: "480"` verrebbe rifiutato con 400 | Flag `numeric: true` + ramo dedicato in `readParams` (fase 2 § 2.1); T-SH5 documenta che il backend rifiuta la stringa, così la regressione è visibile lato server |
| Un job creato prima del piano con altezza fuori lista (es. 500) potrebbe essere alterato aprendo l'editor | Opzione dinamica `ensureOption` (fase 2 § 2.3), invariante I-4 |
| Il repo non ha harness di test JavaScript: le modifiche frontend non sono coperte da test automatici | Dichiarato apertamente: la fase 2 chiude con una verifica manuale scritta nel done-criterion e registrata in `STATE.md` §7; la parte verificabile in automatico è spinta lato backend (T-SH3, T-SH4, T-SH5) |
| `-vf` assente potrebbe cambiare il calcolo del progresso | Il progresso deriva da `out_time_us` e `total_frames` (`src/ffmpeg/progress.rs`), indipendenti dalla risoluzione; T-SH1 verifica che il job arrivi a `completed` |

## Verification summary

| Gate | Command | Where it runs |
|------|---------|---------------|
| Fmt | `cargo fmt --all -- --check` | ogni fase |
| Lint | `cargo clippy --all-targets -- -D warnings` | ogni fase |
| Unit | `cargo test --lib` | ogni fase |
| API | `cargo test --test api` | fasi 1 e 3 |
| E2E | `cargo test --test e2e_conversion` | fase 3 |

**Acceptance:** lo scenario di riferimento è provato da **T-SH1** — conversione
reale di un sorgente anamorfico 720x576 con `scale_height = 0`, con assert su
`video_geometry`: `height == 576`, `width == 720` e SAR uguale a quello del
sorgente — e da **T-SH2** (il test anamorfico esistente a 480, che deve restare
verde) per l'assenza di regressioni.

**Run caveats:** `cargo test --test e2e_conversion` esegue conversioni reali con
l'ffmpeg **di sistema** e si auto-salta se `libsvtav1` manca
(`common::ffmpeg_with_svtav1()`); un run "verde" per skip non prova nulla, va
dichiarato come `could-not-run` in `STATE.md` §7. La config di test forza
`svtav1_params = tune=0` (`tests/common/mod.rs`), i default di produzione restano
`tune=5`.

Questi comandi sono gli stessi scritti in `STATE.md` §3: non devono divergere.

## Model-assignment summary

| Phase | Sub-phases by assignment | Primary | Review gates |
|-------|--------------------------|---------|--------------|
| 1 | 1.1, 1.2, 1.3, 1.4 → `agent:gpt5.6-luna` | `agent:gpt5.6-luna` | self-review — 1.2 (catena di codifica, hot path) |
| 2 | 2.1, 2.2, 2.3, 2.4 → `agent:gpt5.6-luna` | `agent:gpt5.6-luna` | self-review — 2.1 (coercizione numerica), 2.3 (nessuna alterazione dei job esistenti) |
| 3 | 3.1, 3.2, 3.3, 3.4 → `agent:gpt5.6-luna` | `agent:gpt5.6-luna` | self-review — 3.2 (assert di accettazione), 3.4 (lettura finale dei docs) |
