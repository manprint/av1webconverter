# Altezza massima a menu a tendina — Implementation State

> **LEGGI QUESTO FILE PER PRIMO all'inizio di ogni sessione, prima di qualunque
> altro file del piano. AGGIORNALO dopo ogni sub-fase e prima che una sessione
> finisca.**
> **Last updated:** 2026-08-18 | **By:** `agent:gpt5.6-luna` | **Session:** 3

## 0. Protocol

**Resume (cold start):**
1. Leggi questo file da capo a fondo.
2. Esegui i comandi di gate elencati in §3 e confronta il risultato con quanto
   dichiarano §1 e §7. Il repo è la verità: se il file è divergente, correggilo.
3. Apri solo il file di fase indicato in §1 `Next action:`, alla sub-fase indicata.
   Apri `overview.md` solo se §2 non basta per il lavoro da fare.
4. Se §6 non è vuoto, chiudi o annulla quel lavoro a metà prima di iniziare
   qualcosa di nuovo.

**Update (dopo ogni sub-fase, obbligatorio):** riscrivi §1, aggiungi una riga a
§4, aggiorna §5 §6 §7, aggiungi righe a §8 se hai deviato dal piano, aggiorna §9
§10, aggiorna il timestamp in testa, poi allinea `resume.md`. Una sub-fase non è
`DONE` finché questo non è scritto.

## 1. Current position

- **Phase:** 3 — Integrazione, test end-to-end e documentazione (`phase_03.md`) — status `DONE`
- **Sub-phase:** 3.1 `DONE`; 3.2 `DONE`; 3.3 `DONE`; 3.4 `DONE`
- **Next action:** nessuna — piano completato; eseguire `verify` per audit indipendente.
- **Assigned:** `agent:gpt5.6-luna`
- **Repo state:** branch `main` | working tree `dirty` (modifiche non committate di
  fase 1, `.serena/` non tracciata e cartella di piano) | last commit `3faa063 feat: configure conversion output suffix`

## 2. Feature context (self-contained recap)

Il campo "Altezza massima (px)" del form nuovo job diventa un menu a tendina con
nove scelte fisse: `Dimensione originale`, 144px, 240px, 320px, 480px, 720px,
1080px, 1440px, 2160px. Il default resta 480px, letto dal server via
`GET /api/defaults`. `scale_height` resta un `u32` e il valore `0` significa
"dimensione originale": in quel caso ffmpeg viene invocato **senza** `-vf`, quindi
né ridimensionamento né normalizzazione del SAR. La lista fissa è un vincolo di
sola UI: l'API continua ad accettare qualunque altezza in `{0} ∪ 16..=4320`.

**Reference scenario:** sorgente anamorfico 720x576 con SAR non quadrato,
convertito con `scale_height = 0` → output MP4 AV1 di 720x576 con il SAR del
sorgente intatto e nessun `-vf` nella riga di comando; lo stesso sorgente a 480
produce ancora 854x480 SAR 1:1.

**Hard constraints:** nessuna nuova dipendenza; nessuna migrazione dei job JSON già
su disco; l'API resta retro-compatibile; per ogni altezza `>= 16` gli argomenti
ffmpeg devono restare identici a prima; il repo non ha harness di test JavaScript,
quindi le modifiche frontend si verificano a mano e la copertura automatica
equivalente vive nei test API.

**Key decisions in force:** D1 `0` = originale su `u32` · D2 etichetta
`Dimensione originale` · D3 `-vf` omesso interamente · D4 validate accetta `0` o
16..4320 · D5 valore fuori lista conservato come opzione dinamica · D6 default dal
server · D7 single-agent con self-review · D8 `scale_filter -> Option<String>` ·
D9 `numeric: true` con coercizione in `readParams`.

## 3. Environment and commands

I comandi di gate autorevoli. Identici ai phase gates e alla verification summary
di `overview.md`: non devono divergere.

- **Repo root:** `/mnt/fabio/dati/Git/Github-manprint/av1webconverter`
- **Build:** `cargo build` · **Fmt:** `cargo fmt --all -- --check` · **Lint:** `cargo clippy --all-targets -- -D warnings`
- **Unit tests:** `cargo test --lib` · **API:** `cargo test --test api` · **E2E:** `cargo test --test e2e_conversion`
- **Setup / caveats:**
  - `cargo run` crea `in/ out/ data/ logs/` nella CWD; il frontend è embedded via
    `include_str!`, quindi ogni verifica manuale della UI richiede un `cargo run`
    nuovo, non un refresh del browser.
  - `cargo test --test e2e_conversion` esegue conversioni reali con l'ffmpeg **di
    sistema** e si auto-salta se `libsvtav1` manca
    (`common::ffmpeg_with_svtav1()`, `tests/common/mod.rs:94`). Un run verde per
    skip non prova nulla: va registrato come `could-not-run`.
  - La config di test forza `svtav1_params = tune=0` (`tests/common/mod.rs`), perché
    l'SVT-AV1 di sistema è spesso 1.x e rifiuta `tune=5`. I default di produzione
    restano `tune=5`.
  - `AV1WC_SCALE_HEIGHT` sovrascrive il TOML: utile per le verifiche manuali della
    fase 2 (`AV1WC_SCALE_HEIGHT=500 cargo run`).

## 4. Work ledger (append-only, one line per sub-phase, task, or bug fix)

`Phase.Sub` porta la sub-fase per il lavoro pianificato e l'ID di ledger
(`T-A<NNN>` / `B-A<NNN>`) per il lavoro fuori piano. `Commit` è lo sha quando
esiste, `uncommitted` altrimenti.

| # | Phase.Sub | Agent | What changed | Files | Gates | Commit |
|---|-----------|-------|--------------|-------|-------|--------|
| 1 | 1.1 | `agent:gpt5.6-luna` | ammesso `scale_height = 0` come dimensione originale; aggiunti tre test di validazione | `src/config.rs` | fmt, clippy, 79 unit, 15 API verdi | uncommitted |
| 2 | 1.2 | `agent:gpt5.6-luna` | reso filtro opzionale; `-vf` omesso per altezza originale; aggiunti test CRF e two-pass | `src/ffmpeg/encode.rs` | fmt, clippy, encode unit verdi | uncommitted |
| 3 | 1.3 | `agent:gpt5.6-luna` | verificati override env, TOML e deserializzazione schema job esistente; aggiunto test Config | `src/config.rs` | fmt, clippy, 11 config unit verdi; `--print-config` env/TOML verdi | uncommitted |
| 4 | 1.4 | `agent:gpt5.6-luna` | documentato `scale_height = 0` in README per config TOML/env e comportamento utente | `README.md` | fmt, clippy, 83 unit, 15 API, 7 e2e verdi | uncommitted |
| 5 | 2.1 | `agent:gpt5.6-luna` | aggiunto descrittore select con nove opzioni e coercizione numerica in `readParams` | `src/web/app.js` | node syntax, cargo build, fmt, clippy, unit, API verdi | uncommitted |
| 6 | 2.2 | `agent:gpt5.6-luna` | sostituito input numerico con select statica a nove voci nella form nuova | `src/web/index.html` | node syntax, fmt, clippy, unit verdi; DOM embedded verificato | uncommitted |
| 7 | 2.3 | `agent:gpt5.6-luna` | aggiunta opzione dinamica ordinata per altezze fuori lista e riepilogo per dimensione originale | `src/web/app.js` | node syntax, fmt, clippy, 83 unit verdi; verifica CDP editor/API riuscita | uncommitted |
| 8 | 2.4 | `agent:gpt5.6-luna` | documentate scelte UI, limiti API e conservazione altezze fuori elenco | `README.md` | fmt, clippy, 83 unit, 15 API, 7 e2e, node syntax verdi | uncommitted |
| 9 | 3.1 | `agent:gpt5.6-luna` | aggiunti casi API per `scale_height` 0, range invalidi e tipo stringa | `tests/api.rs` | fmt, clippy, 83 unit, 17 API verdi; stringa osservata 400 | uncommitted |
| 10 | 3.2 | `agent:gpt5.6-luna` | aggiunto T-SH1 e2e su sorgente anamorfico con dimensione e SAR originali | `tests/e2e_conversion.rs` | T-SH1 singolo verde; suite e2e 8 test verde | uncommitted |
| 11 | 3.3 | `agent:gpt5.6-luna` | documentati dominio e semantica `scale_height` in API e configurazione di esempio | `docs/api.md`, `config.example.toml` | fmt, clippy, test suite precedente verde | uncommitted |
| 12 | 3.4 | `agent:gpt5.6-luna` | rilettura finale README e documentato limite operativo della dimensione originale | `README.md` | fmt, clippy, 83 unit, 17 API, 8 e2e, node syntax verdi | uncommitted |

## 5. Files touched

| Path | What was done | Phase.Sub |
|------|---------------|-----------|
| `docs/plans/001_plan-ScaleHeightDropdown/*` | creati: overview, tre file di fase, resume, questo file | pianificazione |
| `src/config.rs` | documentato valore sentinella `0`, validazione aggiornata, tre test aggiunti | 1.1 |
| `src/ffmpeg/encode.rs` | filtro opzionale, argomenti video condizionali, test altezza originale | 1.2 |
| `src/config.rs` | test di validazione Config con `scale_height = 0` | 1.3 |
| `README.md` | documentate configurazione e comportamento utente della dimensione originale | 1.4 |
| `src/web/app.js` | descrittore `scale_height` e coercizione numerica per select | 2.1 |
| `src/web/index.html` | select statica con nove scelte di altezza | 2.2 |
| `src/web/app.js` | conservazione opzioni dinamiche e testo riepilogo altezza originale | 2.3 |
| `README.md` | documentate lista UI, default e limiti delle altezze | 2.4 |
| `tests/api.rs` | test API T-SH3, T-SH4 e T-SH5 | 3.1 |
| `tests/e2e_conversion.rs` | test T-SH1 su conversione senza ridimensionamento | 3.2 |
| `docs/api.md` | documentato contratto `params.scale_height` | 3.3 |
| `config.example.toml` | documentato `0 = dimensione originale` | 3.3 |
| `README.md` | rilettura finale e limite operativo della dimensione originale | 3.4 |

## 6. In-flight work

none — piano completato; codice, test e documentazione coerenti.

## 7. Verification state

| Gate / test | Command | Last result | When |
|-------------|---------|-------------|------|
| Fmt | `cargo fmt --all -- --check` | `passed` | 2026-08-18, pre e post modifica |
| Lint | `cargo clippy --all-targets -- -D warnings` | `passed` | 2026-08-18 |
| Unit | `cargo test --lib` | `passed` — 83 test | 2026-08-18 |
| API | `cargo test --test api` | `passed` — 17 test; T-SH3/T-SH4/T-SH5 verdi | 2026-08-18 |
| E2E (T-SH1, T-SH2) | `cargo test --test e2e_conversion` | `passed` — 8 test, T-SH1 eseguito e test anamorfico esistente verde | 2026-08-18 |
| Verifica manuale UI (fase 2.1) | `cargo run` + `node --check` + bundle `/app.js` | `passed` | descrittore e ramo numerico presenti nel bundle embedded; persistenza rinviata a fase 2 |
| Verifica manuale UI (fase 2.2) | `cargo run` + `curl /` | `passed` | DOM embedded mostra label e nove opzioni nell'ordine |
| Verifica manuale UI (fase 2.3) | Chrome CDP + API + JSON job | `passed` | `500px` dinamico ordinato/selezionato, salvataggio conserva 500, riepilogo mostra dimensione originale |
| README fase 2 | rilettura `README.md` | `passed` | lista UI, default, limiti API e valori fuori elenco documentati senza dettagli implementativi |
| Documentazione tecnica fase 3.3 | rilettura `docs/api.md`, `config.example.toml`, `docs/architettura.md` | `passed` | API/config aggiornati; architettura non richiede modifiche |
| README fase 3.4 | rilettura sezioni utente e confronto con `src/web/index.html` | `passed` | lista, default, semantica, limiti e caveat coerenti; nessun dettaglio implementativo |

**Failing output (verbatim, trimmed to the error):**
```
<none>
```

## 8. Runtime deviations from the plan

| # | Plan said | What was done | Why | Impact on later phases |
|---|-----------|---------------|-----|------------------------|
| — | — | — | — | — |

## 9. Blockers and open questions

- none — le sei domande del clarification gate (Q1..Q6) sono state risolte
  dall'utente scegliendo l'opzione consigliata; sono registrate come D1..D6 in
  `overview.md`.
- Da annotare, non bloccante: `CLAUDE.md` afferma che il filtro di scala emette
  `setsar=1`, mentre il codice usa `reset_sar=1` (`src/ffmpeg/encode.rs:38-41`).
  È una imprecisione preesistente della documentazione interna, fuori dallo scopo
  di questo piano: non correggerla durante l'esecuzione senza chiedere all'utente.

## 10. Do-not-repeat

- Non trasformare `scale_height` in `Option<u32>`: valutato e scartato al
  clarification gate (Q1) perché TOML non ha `null` e "dimensione originale" non
  sarebbe esprimibile nel file di configurazione.
- Non usare `-vf setsar=1` quando l'altezza è originale: valutato e scartato (Q3)
  perché su sorgenti anamorfici cambierebbe l'aspetto visivo.
- Non restringere `validate()` ai soli nove valori del menu: valutato e scartato
  (Q4) perché romperebbe il contratto pubblico di `docs/api.md`.
