# Aggiornamento toolchain FFmpeg 9, misura VMAF e modalità di analisi — Resume

> **Next:** fase 7 completata; nessuna modifica oltre fase 7 senza richiesta esplicita
> **Last updated:** 2026-08-18T14:20:00Z
> Solo quadro di stato. Lo stato di esecuzione completo vive in
> [STATE.md](STATE.md); in caso di disaccordo vince STATE.md. Aggiornare
> entrambi dopo ogni sotto-fase.

## Phase status

| Phase | File | Status | Notes |
|-------|------|--------|-------|
| 1 — Immagine: FFmpeg 9.0.1, libvmaf 3.2.0, dav1d 1.5.4, opus statico | phase_01.md | `DONE` | 1.1-1.4 done; T-IMG1/2/3 pass |
| 2 — Propagazione tag e riferimenti di versione | phase_02.md | `DONE` | 2.1-2.3 done; YAML e image build pass |
| 3 — Verifica di regressione con la nuova toolchain | phase_03.md | `DONE` | T-E2E-ALL, T-REG1, T-REG2 e T-SMOKE1 pass; README aggiornato |
| 4 — Misura VMAF: backend | phase_04.md | `DONE` | 4.1-4.5 done; gate finale verde |
| 5 — Misura VMAF: API, frontend, end-to-end | phase_05.md | `DONE` | 5.1-5.4 done; gate finale verde |
| 6 — Modalità Analisi VMAF + Qualità Costante: backend | phase_06.md | `DONE` | 6.1-6.6 done; gate finale verde |
| 7 — Modalità di analisi: API, frontend, end-to-end | phase_07.md | `DONE` | 7.1-7.4 done; gate finali verdi |

Status values: `TODO` · `IN_PROGRESS` · `DONE` · `SKIPPED` · `BLOCKED`

## Tests

| ID | Type | Status | Notes |
|----|------|--------|-------|
| T-IMG1 | e2e (immagine) | `DONE` | `ffmpeg version 9.0.1`, encoder libsvtav1 e libopus, decoder libdav1d, filtro libvmaf, `ldd` senza libopus |
| T-IMG2 | e2e (immagine) | `DONE` | `reset_sar`, opzioni libsvtav1 e le sette chiavi di `-progress` presenti |
| T-IMG3 | e2e (immagine) | `DONE` | modello v1 su 640x360 produce log JSON con `pooled_metrics`; input più piccoli non supportano Speed Chroma |
| T-E2E-ALL | e2e | `DONE` | 14 test di `tests/e2e_conversion.rs` passano con i binari 9.0.1 |
| T-REG1 | e2e (manuale) | `DONE` | `tune=5` in CRF funziona con SVT-AV1 4.2 e FFmpeg 9.0.1 |
| T-REG2 | e2e (manuale) | `DONE` | primo passaggio VBR con `tune=0` funziona |
| T-SMOKE1 | e2e (container) | `DONE` | conversione reale nel container: MP4 `av1`/`av01` all'altezza richiesta |
| T-VMAF2 | unit | `DONE` | `build_vmaf_args`: due input, filtro, riscalatura condizionale, modello, thread |
| T-VMAF3 | unit | `DONE` | `parse_vmaf_log`: `pooled_metrics.vmaf.{mean,min,harmonic_mean}` ed errore se assenti |
| T-VMAF4 | unit | `DONE` | `JobFile` senza campo `vmaf` resta deserializzabile |
| T-VMAF5 | api | `DONE` | default esposti, `vmaf` nullo su file appena caricato, modello non valido rifiutato con 400 |
| T-VMAF1 | e2e | `DONE` | punteggio in `(0,100]` con modello atteso e MP4 identico byte per byte senza misura |
| T-VMAF6 | e2e | `DONE` | modello ignoto: file `completed` con `vmaf.error` valorizzato |
| T-VMAF7 | e2e | `DONE` | sorgente anamorfico misurato correttamente |
| T-AN1 | unit | `DONE` | `crf_da_testare`: +2 per step, scarto dei valori sopra 63 |
| T-AN2 | unit | `DONE` | `segmenti_da_analizzare`: testa e coda escluse, nessuna sovrapposizione, riduzione, file corto, ripetibilità |
| T-AN3 | unit | `DONE` | `scegli_crf`: sopra soglia, tutti sopra, nessuno sopra, nessuna misura valida |
| T-AN4 | unit | `DONE` | `build_probe_args`: `-ss`/`-t`, `-an`, CRF della sonda, stessi parametri video |
| T-AN5 | unit | `DONE` | `build_vmaf_args` con taglio del riferimento prima del secondo `-i` |
| T-AN6 | unit | `DONE` | modalità di analisi: `tune` non forzato, `total_passes` pari a 1 |
| T-AN7 | unit | `DONE` | conteggio dei passaggi saturato a 255; nessuna sonda fuori dalla modalità |
| T-AN8 | unit | `DONE` | `JobFile` senza `vmaf_analysis` resta deserializzabile |
| T-AN9 | api | `DONE` | default dei tre parametri, modalità accettata, valori fuori intervallo rifiutati con 400, campo analisi nullo su upload |
| T-AN10 | e2e | `DONE` | 120 s, CRF 42/44/46 su 3 segmenti, criterio ricalcolato, log con il CRF scelto, output AV1 |
| T-AN11 | e2e | `DONE` | clip di 10 s: segmento unico pari al file, conversione completata |
| T-AN12 | e2e | `DONE` | target inarrivabile: `nessuno_sopra_soglia`, CRF impostato e log dichiarati |
| T-A001 | unit/API | `DONE` | default 30 s, durata 15 s accettata via API, valori fuori 1..=300 rifiutati |

## Docs

| Doc | Phase | Status | Notes |
|-----|-------|--------|-------|
| README.md + README.image.md | 1 | `DONE` | componenti e tag della nuova immagine, modelli VMAF v1 disponibili |
| README.md + README.image.md | 2 | `DONE` | immagine applicativa, rimozione di `libopus0` dai requisiti |
| README.md | 3 | `DONE` | procedura PATH per verifica end-to-end con binari dell'immagine |
| README.md | 4 | `DONE` | `measure_vmaf`, `vmaf_model`, variabili d'ambiente, campo `vmaf`, caveat del confronto, limiti |
| README.md | 5 | `DONE` | uso UI, lettura del punteggio, passaggio extra, troubleshooting e limiti |
| README.md | 6 | `DONE` | modalità di analisi via API, tre parametri, costo con numeri reali, limiti |
| README.md | 7 | `DONE` | modalità dall'interfaccia, esempio numerico, lettura dell'esito, risoluzione dei problemi |
| docs/api.md | 5 | `DONE` | parametri della misura e campo `vmaf` |
| docs/api.md | 7 | `DONE` | modalità `vmaf_analysis`, tre parametri, campo `vmaf_analysis` |
| docs/architettura.md | 2 | `DONE` | nota su `reset_sar` allineata a FFmpeg 9.0.1 |
| CLAUDE.md | 2 | `DONE` | correzione della voce falsa sul filtro di scala |

## Open blockers

- none

## Decisions changed at runtime

- D30: libvmaf build usa `enable_float=true` per modelli VMAF v1.
- D31: T-AN10 usa 3 segmenti e 3 CRF.
- D32: T-AN10/T-AN12 usano clip da 120 s per rispettare segmenti non sovrapposti.

## V001 correction status

- V001-F01: `FIXED` — T-AN10 asserisce riga CRF codifica finale.
- V001-F02: `FIXED` — esempio README coerente 3×3.
- V001-F03: `FIXED` — default script ripristinato come phase 2.2.
- V001-F04: `ACCEPTED` — `.serena/` resta non tracciata su richiesta utente.
- V001-F05: `FIXED` — conteggio unit 117.
- B-A001: `FIXED` — fonte Lychee 403 rimossa da R2; changelog ufficiale mantenuto.
- T-A001: `DONE` — durata segmenti configurabile; default applicativo 30 s,
  Compose 15 s; unit 118, API 25, e2e 14.
