# UpgradeFfmpeg9Vmaf — Bug ledger

> Difetti corretti con `/plan-execute-verify bug`. Append-only.
> Gli ID non vengono riutilizzati.

## B-A001 — Link Lychee bloccato da 403

- **Date:** 2026-08-18 | **Agent:** `agent:gpt5.6-luna` | **Severity:** minor
- **Symptom:** il workflow Lychee fallisce con HTTP 403 su una fonte secondaria
  non necessaria referenziata in `overview.md` R2.
- **Reproduction:** `lycheeverse/lychee-action@v2` → Lychee → `[403]` sulla
  fonte secondaria; il comando locale `curl -L` rispondeva 200 ma Lychee CI la
  rifiuta.
- **Root cause:** R2 includeva fonte secondaria con risposta 403 al client Lychee
  — `docs/plans/002_plan-UpgradeFfmpeg9Vmaf/overview.md:166`.
- **Fix:** rimossa fonte 9to5Linux da R2; changelog ufficiale FFmpeg resta unica
  fonte per le rimozioni di FFmpeg 9.
- **Files:** `docs/plans/002_plan-UpgradeFfmpeg9Vmaf/overview.md:166`,
  `docs/plans/002_plan-UpgradeFfmpeg9Vmaf/bugs.md`.
- **Regression test:** link check Lychee del workflow — URL ufficiale FFmpeg
  resta raggiungibile; fonte 403 non è più analizzata.
- **Gates:** `curl -L` su URL ufficiale, gate Rust e diff check → pass;
  Lychee locale non installato, workflow CI resta gate definitivo.
- **README:** not user-visible.
- **Plan impact:** aggiornata fonte R2; nessun cambio di decisioni, codice o
  comportamento applicativo.
- **Related:** phase 1 § 1.3.
