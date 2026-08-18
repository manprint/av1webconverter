# Altezza massima a menu a tendina — Resume

> **Next:** nessuna — piano completato; usare `verify` per audit indipendente
> **Last updated:** 2026-08-18
> Solo quadro di stato. Lo stato di esecuzione completo è in [STATE.md](STATE.md);
> in caso di disaccordo vince STATE.md. Aggiorna entrambi dopo ogni sub-fase.

## Phase status

| Phase | File | Status | Notes |
|-------|------|--------|-------|
| 1 — Backend: sentinella "originale" e filtro opzionale | phase_01.md | `DONE` | 1.1-1.4 completate; gate verdi |
| 2 — Frontend: menu a tendina | phase_02.md | `DONE` | 2.1-2.4 completate; gate verdi e verifica CDP riuscita |
| 3 — Integrazione, e2e e documentazione | phase_03.md | `DONE` | 3.1-3.4 completate; suite completa verde |

Status values: `TODO` · `IN_PROGRESS` · `DONE` · `SKIPPED` · `BLOCKED`

## Tests

| ID | Type | Status | Notes |
|----|------|--------|-------|
| `altezza_zero_significa_dimensione_originale` | unit | `PASS` | `validate()` è `Ok` con `scale_height: 0` |
| `altezza_fra_uno_e_quindici_e_rifiutata` | unit | `PASS` | `scale_height: 8` → `Err`, messaggio con "0 (dimensione originale)" |
| `altezza_oltre_il_massimo_e_rifiutata` | unit | `PASS` | `scale_height: 4321` → `Err` |
| `altezza_zero_da_ambiente_e_accettata` | unit | `PASS` | `Config::validate()` è `Ok` con `encoding.scale_height = 0` |
| `filtro_di_scala_assente_con_altezza_originale` | unit | `PASS` | `scale_filter(0) == None` |
| `filtro_di_scala_calcola_la_larghezza_dal_dar` | unit | `PASS` | esistente aggiornato con `.unwrap()` |
| `filtro_di_scala_resetta_il_sar` | unit | `PASS` | esistente aggiornato con `.unwrap()` |
| `comando_crf_omette_vf_con_altezza_originale` | unit | `PASS` | nessun `-vf` né `scale=-2` negli argomenti CRF |
| `comando_due_passaggi_omette_vf_con_altezza_originale` | unit | `PASS` | nessun `-vf` né `scale=-2` nel primo passaggio VBR |
| T-SH3 `la_dimensione_originale_e_accettata_dall_api` | api | `PASS` | `PATCH` con `scale_height: 0` → 200 e valore persistito |
| T-SH4 (casi in `creazione_job_rifiuta_input_non_valido`) | api | `PASS` | `scale_height` 8 e 5000 → 400 |
| T-SH5 `un_altezza_come_stringa_viene_rifiutata` | api | `PASS` | `"480"` → 400 (deserializzazione Json) |
| T-SH1 `la_dimensione_originale_non_ridimensiona` | e2e | `PASS` | output 720x576, SAR uguale al sorgente, job `completed` |
| T-SH2 `un_sorgente_anamorfico_conserva_l_aspetto_visivo` | e2e | `PASS` | esistente verde: 854x480, SAR 1:1 |

## Docs

| Doc | Phase | Status | Notes |
|-----|-------|--------|-------|
| README.md | 1 | `DONE` | aggiornate `## Configurazione` e `## Parametri di conversione` con `0 = dimensione originale` |
| README.md | 2 | `DONE` | `## Funzionalità`, `## Parametri di conversione`, lista scelte e limiti API aggiornati |
| README.md | 3 | `DONE` | rilettura finale coerente con UI e contratto API |
| docs/api.md | 3 | `DONE` | nota su `params.scale_height` sotto `POST /api/jobs` |
| config.example.toml | 3 | `DONE` | commento della riga `scale_height` |
| docs/architettura.md | 3 | `DONE` | verificato: nessuna menzione del ridimensionamento, non modificato |

## Open blockers

- none

## Decisions changed at runtime

- none
