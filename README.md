# av1webconverter

Applicazione web per la codifica video in AV1 con SVT-AV1. Backend Rust, frontend
embedded nel binario, distribuita come **singolo container**.

Riproduce la catena di conversione degli script bash di riferimento
(`start_av1_docker.sh`, `execute_docker.sh`) usando la build custom di ffmpeg
definita in [`Dockerfile.ffmpeg-svtav1`](Dockerfile.ffmpeg-svtav1) — senza la
parte SSH: ffmpeg viene eseguito nello stesso container del servizio.

- **Documentazione API**: [docs/api.md](docs/api.md)
- **Architettura**: [docs/architettura.md](docs/architettura.md)
- **Immagine ffmpeg**: [README.image.md](README.image.md)

## Funzionalità

- Job di conversione con nome, ognuno contenitore logico di più file.
- Upload di file singoli o di cartelle intere.
- Parametri di conversione per job: default dal file di configurazione,
  sovrascrivibili dal frontend; l'altezza massima si sceglie da un menu con
  valori predefiniti, inclusa `Dimensione originale`.
- Conversione **in sequenza** dentro il job (1 conversione simultanea per
  default, valore configurabile e modificabile per job).
- Progresso per file e progresso totale del job, pesato sulla dimensione dei
  sorgenti.
- Log di conversione per file, con lo stesso formato degli script bash,
  consultabili live dal frontend e persistiti su disco.
- Stop e cancellazione di un job in qualsiasi stato, con pulizia dei file sul
  backend.
- Download dei file convertiti: singolo, selezione multipla o intero job in ZIP.
- Notifiche Slack di terminazione ed errore.
- Log applicativi strutturati JSON.
- Frontend protetto da basic auth con credenziali da configurazione.

## Requisiti

- Per l'uso in container: Docker.
- Per lo sviluppo locale: Rust 1.85+ e `ffmpeg`/`ffprobe` con `libsvtav1` e
  `libopus` nel PATH.

## Avvio con Docker

Il push del tag Git `v1.0.0` esegue i workflow GitHub e pubblica l'immagine
completa, frontend e FFmpeg inclusi, nel GitHub Container Registry:

```bash
docker pull ghcr.io/manprint/av1webconverter:v1.0.0
```

Avvio con Compose (metodo consigliato):

```bash
docker compose pull
docker compose up -d
```

Il riferimento può essere sovrascritto, per esempio per provare una versione
successiva, con `AV1WC_IMAGE=ghcr.io/manprint/av1webconverter:vX.Y.Z`.

Avvio senza Compose:

```bash
docker run -d --name av1webconverter \
  -p 8080:8080 \
  -v "$PWD/av1-data:/data" \
  -e AV1WC_AUTH_USERNAME=admin \
  -e AV1WC_AUTH_PASSWORD='una password robusta' \
  -e AV1WC_SLACK_WEBHOOK_URL='https://hooks.slack.com/services/...' \
  ghcr.io/manprint/av1webconverter:v1.0.0
```

Il frontend è su <http://localhost:8080>. Dentro il volume `/data`:

| Percorso       | Contenuto                                    |
| -------------- | -------------------------------------------- |
| `/data/in`     | file caricati, una sottocartella per job      |
| `/data/out`    | file convertiti, una sottocartella per job    |
| `/data/state`  | stato dei job (JSON) e file temporanei        |
| `/data/logs`   | log applicativi e log di conversione per file |

Per una configurazione completa via file invece che via variabili d'ambiente:

```bash
cp config.example.toml config.toml   # poi modificare
docker run -d -p 8080:8080 \
  -v "$PWD/av1-data:/data" \
  -v "$PWD/config.toml:/etc/av1webconverter/config.toml:ro" \
  ghcr.io/manprint/av1webconverter:v1.0.0
```

Per costruire localmente invece di usare GHCR, creare prima la base FFmpeg e
passarla al `Dockerfile` applicativo:

```bash
docker build -f Dockerfile.ffmpeg-svtav1 \
  -t local/ffmpeg-svtav1:8.1.2-svt4.2.0-vmaf3.1.0 .
docker build --build-arg FFMPEG_IMAGE=local/ffmpeg-svtav1:8.1.2-svt4.2.0-vmaf3.1.0 \
  -t av1webconverter:local .
```

## Sviluppo locale

```bash
cp config.example.toml config.toml
cargo run
```

Il servizio crea da sé `in/`, `out/`, `data/` e `logs/` nella directory corrente.

Comandi utili:

```bash
cargo test                          # tutti i test
cargo test --lib                    # solo test unitari
cargo test --test api               # solo API (nessuna conversione reale)
cargo test --test e2e_conversion    # conversioni reali con ffmpeg
cargo clippy --all-targets          # lint
cargo run -- --print-config         # configurazione effettiva
```

I test end-to-end usano l'`ffmpeg` di sistema; se manca `libsvtav1` vengono
saltati con un messaggio invece di far fallire la build.

## Configurazione

Tutte le voci sono documentate in [`config.example.toml`](config.example.toml).
Le variabili d'ambiente (prefisso `AV1WC_`) hanno la precedenza sul file, così da
poter configurare il container senza montare un TOML. Le più usate:

| Variabile                  | Effetto                                            |
| -------------------------- | -------------------------------------------------- |
| `AV1WC_AUTH_USERNAME`      | utente della basic auth                            |
| `AV1WC_AUTH_PASSWORD`      | password della basic auth                          |
| `AV1WC_SLACK_WEBHOOK_URL`  | webhook Slack (impostarlo abilita le notifiche)    |
| `AV1WC_DEFAULT_CONCURRENCY`| conversioni simultanee per job                     |
| `AV1WC_CRF`                | CRF di default                                     |
| `AV1WC_PRESET`             | preset SVT-AV1 di default                          |
| `AV1WC_SCALE_HEIGHT`       | altezza massima dell'output; `0` = dimensione originale |
| `AV1WC_SVTAV1_PARAMS`      | `-svtav1-params` di default                        |
| `AV1WC_LOG_LEVEL`          | filtro dei log applicativi                         |

## Parametri di conversione

Due modalità:

**`crf` — qualità costante** (default). Bitrate variabile, un solo passaggio.
Usa `crf` e `svtav1_params`.

**`target_size` — dimensione finale desiderata.** VBR a due passaggi. Il bitrate
video è calcolato sottraendo dal target il margine per l'overhead MP4
(`size_margin_percent`) e lo spazio della traccia audio Opus. La dimensione è un
obiettivo, non un limite rigido: per un vincolo stretto alzare il margine (es.
`size_margin_percent = 2`).

Default rilevanti, ereditati dalle misure documentate in `README.image.md`:

- `svtav1_params = "tune=5"`, ottimizzato per la qualità percettiva VMAF (95,076
  contro 92,810 di `tune=0` a pari bitrate). `tune=0` conserva un PSNR superiore
  (46,314 contro 45,998 dB) ed è la scelta quando serve fedeltà matematica.
- SVT-AV1 accetta `tune=5` **solo in CRF**: in modalità `target_size` il valore
  viene forzato a `tune=0`, l'unico compatibile con il VBR a due passaggi.
  `tune=5` richiede inoltre SVT-AV1 4.x, quello dell'immagine custom.
- `enable-qm=1:qm-min=0` resta escluso: nei test riduceva sia SSIM sia PSNR.
- Il ridimensionamento limita l'altezza a `scale_height` mantenendo l'aspetto
  visivo del sorgente e normalizzando a pixel quadrati (SAR 1:1), per evitare DAR
  errati con sorgenti anamorfiche. Con `scale_height = 0` non viene applicato
  alcun ridimensionamento: il video conserva dimensione e rapporto dei pixel del
  sorgente. Il default è `480`; per usare la dimensione originale, impostare
  `scale_height = 0` nel file TOML oppure `AV1WC_SCALE_HEIGHT=0`.
- Nell'interfaccia sono disponibili `Dimensione originale`, 144, 240, 320, 480,
  720, 1080, 1440 e 2160 px; il default è 480px. L'API accetta anche altezze
  diverse da quelle del menu, fra 16 e 4320. Se un job usa una di queste altezze,
  l'interfaccia la mostra come voce aggiuntiva e la conserva quando i parametri
  vengono salvati.
- La scelta `Dimensione originale` può produrre file più grandi e richiedere più
  tempo, perché l'altezza del sorgente non viene ridotta.

## Log

- **Applicativi**: leggibili su stdout e strutturati JSON in
  `logs/av1webconverter.jsonl` (rotazione giornaliera).
- **Conversione**: una riga per aggiornamento di progresso, per singolo file, in
  `logs/jobs/<job_id>/<file_id>.log`, nello stesso formato degli script bash:

  ```
  [Codifica]  42.3% | 00:00:51 / 00:02:00 | frame 1275 | file 12.40 MB | 1234kbits/s | 2.1x | ETA 00:00:33
  ```

  In modalità `target_size` l'etichetta indica anche il passaggio corrente
  (`[Codifica (passaggio 2/2)]`).

## Formati di input

Oggi il formato previsto è MP4. La lista `jobs.allowed_extensions` include già
altri contenitori (`mov`, `mkv`, `avi`, `webm`, `m4v`, `ts`, `mpg`, `mpeg`,
`wmv`): l'input viene passato a ffmpeg senza assunzioni sul contenitore, quindi
per abilitarne altri basta aggiungerli in configurazione. L'output è sempre MP4
con video AV1 e audio Opus.
