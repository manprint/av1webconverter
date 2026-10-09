# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`av1webconverter` — a Rust web application for AV1 video encoding with SVT-AV1. Single self-contained binary with the frontend embedded via `include_str!`, shipped as one Docker container that also carries the custom ffmpeg build.

`specifications.md` (Italian) is the original requirements document. **The codebase, comments, log messages, UI text, and docs are all in Italian** — match that when adding code.

## Commands

```bash
cargo run                          # dev server (creates in/ out/ data/ logs/ in CWD)
cargo run -- --print-config        # effective config after env overrides
cargo test                         # everything
cargo test --lib                   # unit tests only (fast, no ffmpeg)
cargo test --test api              # HTTP/API integration, no real encoding
cargo test --test e2e_conversion   # real ffmpeg conversions
cargo test --lib config::          # single module
cargo clippy --all-targets         # lint (kept warning-free)
```

E2E tests spawn a real server on an ephemeral port and encode real video via the **system** ffmpeg. They self-skip when `libsvtav1` is absent (`common::ffmpeg_with_svtav1()`), so a machine without the codec still passes the suite. Test config in `tests/common/mod.rs` overrides `svtav1_params` to `tune=0` because system SVT-AV1 is often 1.x, which rejects `tune=5`; production defaults stay `tune=5`.

Production image published by the `v1.0.0` Git tag workflow:

```bash
docker pull ghcr.io/manprint/av1webconverter:v1.0.0
docker compose up -d
```

## Architecture

Full detail in `docs/architettura.md`; API contract in `docs/api.md`; the measurements behind the encoding defaults in `docs/qualita-codifica.md`. The parts that need reading several files to grasp:

**State ownership.** `AppState` (`src/state.rs`) holds `HashMap<Uuid, Arc<JobHandle>>`. A `JobHandle` is the serializable `RwLock<Job>` plus a `watch<bool>` cancel channel and a `worker_running` atomic. API-level mutations go through `AppState::mutate`, which persists the job JSON and publishes a `job_updated` snapshot. The worker instead writes progress straight through the `RwLock` and emits the lighter `file_progress` event, so a 5-second progress tick doesn't broadcast a full snapshot.

**Persistence is one JSON per job** (`src/store.rs`, `data_dir/jobs/<id>.json`), written to a temp file and renamed. No database. On boot, jobs found in `running` state have no surviving ffmpeg process, so their `running` files are reset to `pending` and the job returns to `pending` for the user to restart.

**Encoding chain** (`src/worker.rs` + `src/ffmpeg/`) reproduces `start_av1_docker.sh` minus SSH: `ffprobe` → encode to temp Matroska → remux to MP4 with `-tag:v av01 -movflags +faststart` → atomic rename into `out/<job_id>/`. The final path only ever appears complete. `TargetSize` mode adds a second pass. Temp files are cleaned by a `Drop` guard, so errors and cancellation don't leak them.

**Cancellation** is the `watch<bool>`: `run_ffmpeg` selects on it while reading child output and kills the process on `true`. Callers distinguish `RunOutcome::Cancelled` from failure.

**Sequencing** uses `futures::stream::for_each_concurrent(concurrency, …)` over files in upload order — with `concurrency = 1` this is exactly the "next file starts when the previous finishes" the spec requires.

**Progress parsing** (`src/ffmpeg/progress.rs`) is a port of the awk `show_progress` from the bash scripts, reading `-progress pipe:1`. Percent comes from `out_time_us` (never regresses), refined upward by `frame / total_frames`, capped at 99.9% until the process actually ends. Job-level progress is weighted by source file size.

## Non-obvious constraints

- **`tune=5` is CRF-only.** `EncodingParams::effective_svtav1_params()` forces `tune=0` in `TargetSize` mode because SVT-AV1 rejects `tune=5` under two-pass VBR. It replaces the whole string, so per-job extras such as `film-grain=8` are dropped in that mode. It also needs SVT-AV1 4.x.
- **The scale filter matches the bash scripts.** It uses `scale=...:reset_sar=1`, an option added in ffmpeg 7.1 and present in the FFmpeg 9.0.2 image. The comma inside `min()` must stay escaped; with `scale_height == 0` no scale filter is emitted.
- **`enable-qm=1:qm-min=0` stays excluded** — it lowered both SSIM and PSNR in the measurements recorded in `README.image.md`.
- **Encoder defaults are measured, not guessed.** `docs/qualita-codifica.md` records why `tune=5` stays, why variance boost and the psy options are excluded, and why film grain is per-job only. Re-run comparable measurements before changing any of them.
- **libopus rejects 5.1(side)** (AC3, E-AC3, DTS) in FFmpeg 9, failing the whole file. `AUDIO_DOWNMIX_FILTER` (`aformat=channel_layouts=mono|stereo`) must precede every `-c:a libopus`, in `start_av1_docker.sh` too. `-map 0:a?` re-encodes every track, so `compute_video_bitrate` reserves `audio_bitrate` once per `MediaInfo::audio_streams`.
- **The GOP is in seconds.** `-g` comes from `EncodingParams::effective_keyint(frame_rate)` = `round(fps × keyint_seconds)`; no frame rate, fps outside 1..=1000 or `keyint_seconds = 0` fall back to the fixed `keyint`. Every command that must match the final encode (the VMAF analysis probes included) takes the source `MediaInfo` for this. A `keyint=` inside `svtav1_params` overrides `-g`.
- **Colour tags go through `setparams` only.** `color_tag_filter` adds the missing BT.709 tags to HD sources (width ≥ 1280 or height ≥ 720) that are untagged or tagged only BT.709; a source carrying any other tag is left alone. The output options `-colorspace`/`-color_primaries`/`-color_trc` would make `scale` convert pixels and tag only the matrix. Every VMAF graph must apply the same tags to the reference branch (`VmafRequest::tag_colore`): with mismatched tags FFmpeg 9 converts one branch before libvmaf and the score collapses (67.6 → 34.0 in the test).
- **The VMAF model switches to HFR on its own.** `modello_vmaf_effettivo` maps `vmaf_v1.0.16_X` to `vmaf_v1.0.16_hfr_X` at ≥ 45 fps, because the standard model scores 60 fps sources about 0.75 lower and `vmaf_analysis` would pick needlessly low CRFs. Results record the effective model, not the configured one.
- **ffmpeg is spawned without a shell**, args as a vector. Values reaching the command line are still validated against a restricted alphabet (`is_safe_ffmpeg_token`), so `tune=5; rm -rf /` is rejected at the API boundary. Keep that check when adding parameters.
- **Config env overrides win over the TOML file** (`AV1WC_*`), so the container can be configured without mounting a file. `deny_unknown_fields` + `serde(default)` on config structs means partial JSON from the frontend is fine but typos are errors.
- **`README.image.md` documents the scripts that exist here** — `start_av1_docker.sh` and `execute_docker.sh`. Fixed-size conversion and local web conversion are available through the Rust application and its API, not through additional shell scripts; the SSH half of the reference script remains out of scope.

## Conventions

Italian for identifiers in tests, comments, and all user-facing strings. Comments explain *why*, not *what*. Errors surface through `ApiError` (`src/error.rs`), which maps to status codes and hides internal detail from clients while logging it. New API routes use axum 0.8 `{param}` path syntax.
