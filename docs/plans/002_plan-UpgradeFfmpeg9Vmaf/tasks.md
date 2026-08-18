# UpgradeFfmpeg9Vmaf — Task ledger

> Modifiche richieste fuori dalle sottofasi originali. Append-only.

## T-A001 — Durata segmenti VMAF configurabile

- **Date:** 2026-08-18 | **Agent:** `agent:gpt5.6-luna`
- **Request:** rendere durata segmenti VMAF configurabile da UI e variabile
  ambiente; Compose default 15 s.
- **Change:** aggiunto `vmaf_segment_duration_seconds` con default 30 s, env,
  validazione e uso nella segmentazione; aggiunto campo UI/API; Compose imposta 15.
- **Files:** `src/config.rs`, `src/ffmpeg/vmaf.rs`, `src/worker.rs`,
  `src/web/app.js`, `src/web/index.html`, `compose.yaml`, `config.example.toml`,
  `README.md`, `docs/api.md`, `phase_06.md`, `phase_07.md`, `overview.md`.
- **Tests:** default/API 30, rifiuto 0.5, segmentazione 15 s; suite e2e invariata.
- **Gates:** `cargo fmt --all -- --check`, clippy, build release, unit 118, API
  25, e2e 14 sistema/FFmpeg 9, frontend, YAML, Compose, diff → pass.
- **README:** updated configurazione, UI e limiti durata.
- **Plan impact:** superseded D18; updated D23/interface/phase docs.
- **Related:** phase 6 § 6.1–6.2, phase 7 § 7.1–7.2.
