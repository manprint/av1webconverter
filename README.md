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
- Misura VMAF opzionale sull'output definitivo, senza modificarlo.
- Modalità **Analisi VMAF + Qualità Costante**, selezionabile via API o frontend.
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
  -t local/ffmpeg-svtav1:9.0.1-svt4.2.0-vmaf3.2.0 .
docker build --build-arg FFMPEG_IMAGE=local/ffmpeg-svtav1:9.0.1-svt4.2.0-vmaf3.2.0 \
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

Per verificare la conversione con i binari FFmpeg dell'immagine custom, estrarre
`ffmpeg` e `ffprobe` in `target/ffmpeg9/bin` e anteporre quella directory al
`PATH`:

```bash
PATH="$PWD/target/ffmpeg9/bin:$PATH" cargo test --test e2e_conversion
```

Gli stessi test si saltano automaticamente quando il binario nel `PATH` non
include SVT-AV1; il binario dell'immagine include SVT-AV1, libdav1d, libopus e
libvmaf.

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
| `AV1WC_MEASURE_VMAF`       | abilita la misura VMAF (`true`/`false`)            |
| `AV1WC_VMAF_MODEL`         | modello VMAF di default                           |
| `AV1WC_VMAF_SEGMENTS`      | segmenti analizzati in modalità VMAF              |
| `AV1WC_CRF_TEST_STEPS`     | CRF aggiuntivi provati in modalità VMAF           |
| `AV1WC_VMAF_TARGET`        | VMAF medio minimo richiesto                        |
| `AV1WC_VMAF_SEGMENT_DURATION_SECONDS` | durata spezzone VMAF in secondi; default 30, Compose 15 |
| `AV1WC_LOG_LEVEL`          | filtro dei log applicativi                         |

## Parametri di conversione

Tre modalità:

**`crf` — qualità costante** (default). Bitrate variabile, un solo passaggio.
Usa `crf` e `svtav1_params`.

**`target_size` — dimensione finale desiderata.** VBR a due passaggi. Il bitrate
video è calcolato sottraendo dal target il margine per l'overhead MP4
(`size_margin_percent`) e lo spazio della traccia audio Opus. La dimensione è un
obiettivo, non un limite rigido: per un vincolo stretto alzare il margine (es.
`size_margin_percent = 2`).

**`vmaf_analysis` — Analisi VMAF + Qualità Costante.** Parte dal `crf` impostato,
prova CRF crescenti di 2 in 2 su spezzoni casuali della durata configurata e usa per la
conversione il CRF più alto che mantiene il VMAF medio almeno pari a
`vmaf_target`. Ogni CRF viene misurato su tutti i segmenti; se nessuno supera la
soglia viene usato il CRF impostato. Fuori da questa modalità non vengono eseguite
sonde aggiuntive.

Default rilevanti, ereditati dalle misure documentate in `README.image.md`:

- `svtav1_params = "tune=5"`, ottimizzato per la qualità percettiva VMAF (95,076
  contro 92,810 di `tune=0` a pari bitrate). `tune=0` conserva un PSNR superiore
  (46,314 contro 45,998 dB) ed è la scelta quando serve fedeltà matematica.
- SVT-AV1 accetta `tune=5` nelle modalità CRF e analisi VMAF: in modalità
  `target_size` il valore viene forzato a `tune=0`, l'unico compatibile con il
  VBR a due passaggi.
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

### Misura VMAF

`measure_vmaf` (default `false`) misura, dopo la conversione, la qualità VMAF
dell'MP4 definitivo rispetto al sorgente. È un passaggio ffmpeg aggiuntivo e non
modifica l'output pubblicato. Si può impostare anche con `AV1WC_MEASURE_VMAF`.

`vmaf_model` (default `vmaf_v1.0.16_5d0h`) seleziona il modello built-in di
libvmaf. Il default è il modello per telefono, tarato su distanza di visione 5H.
Sono disponibili `vmaf_v1.0.16_5d0h` per telefono,
`vmaf_v1.0.16_3d0h` per televisore a 1080p,
`vmaf_v1.0.16_3d0h_2160` e `vmaf_v1.0.16_1d5h_2160` per 4K, le quattro
varianti `_hfr_` per alto frame rate e i modelli precedenti
`vmaf_v0.6.1`, `vmaf_v0.6.1neg`, `vmaf_b_v0.6.3`, `vmaf_4k_v0.6.1`,
`vmaf_4k_v0.6.1neg`. Il modello telefono tende a restituire punteggi più alti a
parità di file, perché su schermo piccolo gli artefatti si vedono meno; punteggi
ottenuti con modelli diversi non sono confrontabili. Il modello si può impostare
con `AV1WC_VMAF_MODEL`.

Il confronto avviene alla risoluzione di uscita: quando l'output è ridimensionato,
il sorgente viene riscalato alla stessa geometria. Il punteggio misura quindi la
perdita da compressione e non quella dovuta al ridimensionamento.

Esempio di job con misura attiva:

```json
{
  "name": "episodio misurato",
  "params": {
    "crf": 40,
    "measure_vmaf": true,
    "vmaf_model": "vmaf_v1.0.16_5d0h"
  }
}
```

Al termine, ogni file contiene `vmaf` con questa forma:

```json
{
  "score": 95.5,
  "min": 80.1,
  "harmonic_mean": 95.0,
  "model": "vmaf_v1.0.16_5d0h",
  "duration_seconds": 12.4,
  "error": null
}
```

La misura raddoppia circa il tempo di elaborazione del file. Un suo fallimento
non fa fallire la conversione e viene riportato in `vmaf.error`; se il job viene
interrotto durante la misura, l'output resta valido ma senza punteggio. Il numero
è confrontabile solo fra file misurati con lo stesso modello.

#### Uso dall'interfaccia

Nel pannello **Nuovo job**, attivare **Misura VMAF dopo la conversione** e, se
necessario, modificare **Modello VMAF**. Gli stessi campi sono disponibili in
**Parametri** per un job già creato, finché non è in esecuzione. Dopo la
conversione il risultato appare nella riga del file, accanto alle dimensioni:

```
VMAF 95.50 (vmaf_v1.0.16_5d0h)
```

`VMAF <punteggio> (<modello>)` indica la media VMAF aggregata dal modello
selezionato. Se la misura non produce un punteggio viene mostrato `VMAF non
disponibile`; il motivo dettagliato resta nel log del file e nel campo `error`
della misura restituito dall'API. Durante la misura il progresso mostra un
passaggio aggiuntivo rispetto alla sola codifica.

#### Risoluzione dei problemi e limiti

Se il punteggio manca sempre, verificare che ffmpeg includa il filtro `libvmaf`:

```bash
ffmpeg -hide_banner -filters | grep libvmaf
```

Il filtro è incluso nell'immagine fornita, ma può mancare nelle build ffmpeg di
sistema. Se il nome del modello non è riconosciuto da libvmaf, il file resta
convertito e il motivo compare nel campo di errore della misura.

La misura non modifica l'MP4 pubblicato. I risultati sono confrontabili solo
fra file misurati con lo stesso modello. Con più conversioni in parallelo la
misura usa la CPU insieme alle codifiche e può aumentare il tempo complessivo.

### Analisi VMAF + Qualità Costante

La modalità si attiva via API impostando `mode` a `vmaf_analysis`, oppure dal
pannello **Nuovo job** selezionando **Analisi VMAF + Qualità Costante**. In questa
modalità restano visibili **CRF** e **svtav1-params**; compaiono anche i tre campi
specifici:

| Parametro | Default | Intervallo | Effetto |
|-----------|---------|------------|---------|
| `vmaf_segments` | `3` | `1..=10` | numero di spezzoni casuali |
| `vmaf_segment_duration_seconds` | `30.0` | `1..=300` | durata di ogni spezzone in secondi |
| `crf_test_steps` | `3` | `1..=10` | numero di CRF aggiuntivi, a passi di 2 |
| `vmaf_target` | `93.0` | `0..=100` | VMAF medio minimo per accettare un CRF |

**VMAF Segments** indica quanti spezzoni casuali vengono sondati per ogni CRF;
la durata è impostata da **Durata segmento VMAF (s)**, default `30` secondi
(15 secondi nel Compose di produzione).
**CRF Test Steps** indica quanti CRF aggiuntivi
vengono provati a passi di 2; default `3`. **VMAF Target** è il punteggio VMAF
medio minimo richiesto; default `93`. I tre valori rispettano gli intervalli
indicati nella tabella.

Le variabili d'ambiente corrispondenti sono `AV1WC_VMAF_SEGMENTS`,
`AV1WC_CRF_TEST_STEPS` e `AV1WC_VMAF_TARGET`. I CRF oltre 63 vengono ignorati;
se tutti quelli provati restano sopra il target conviene aumentare
`crf_test_steps`.

Esempio di richiesta API:

```json
{
  "name": "analisi qualità",
  "params": {
    "mode": "vmaf_analysis",
    "crf": 40,
    "preset": 10,
    "scale_height": 480,
    "vmaf_segments": 3,
    "crf_test_steps": 3,
    "vmaf_target": 93.0
  }
}
```

Lo stesso esempio dall'interfaccia si ottiene impostando **CRF** a `40`, **CRF
Test Steps** a `3`, **VMAF Segments** a `3` e **VMAF Target** a `93`. Il backend
prova quindi CRF `42`, `44` e `46` su tre spezzoni della durata configurata e usa il CRF più
alto che resta sopra `93`.

Il log contiene la selezione dei segmenti, una riga per ogni sonda, il riepilogo
per CRF e la conclusione `Analisi VMAF - CRF scelto:`. In una prova su un clip di
70 secondi con due segmenti e due step, le sonde CRF 42 e 44 hanno prodotto VMAF
medi 99.40 e 99.22 in 5.84 secondi; il CRF scelto è stato 44 perché entrambi
superavano il target.

Il costo è `segmenti × step` codifiche brevi più altrettante misure VMAF prima
della conversione finale. Per esempio, 2 segmenti e 2 step significano 4 sonde,
4 misure e poi la codifica definitiva. Lasciare valori bassi con preset lenti.
Il risultato viene persistito in `files[].vmaf_analysis` con segmenti, punteggi,
motivo della scelta e durata dell'analisi.

Durante l'analisi la barra di avanzamento conta anche le sonde: ogni sonda usa
due passaggi, oltre alla codifica finale, quindi il numero di passaggi mostrato è
più alto del normale. La riga del file mostra `analisi VMAF: CRF <n>, target
<t>`; il tooltip della riga contiene una riga per ogni CRF con il relativo VMAF.
Il riepilogo completo, compreso il motivo della scelta, resta disponibile nel
log di conversione.

Per il caso d'uso tipico su cellulare, con output 480p o 720p, il modello VMAF
predefinito è tarato per lo schermo del telefono. In questa condizione il target
`93` corrisponde già a una qualità molto alta. Se tutti gli step restano sopra il
target, aumentare **CRF Test Steps** per cercare un CRF più alto e risparmiare
spazio.

Limiti noti: il CRF scelto vale per quel file e per il modello usato; punteggi di
modelli diversi non sono confrontabili. Su file più corti della durata configurata si analizza
il file intero. Se nessun CRF raggiunge il target viene usato il CRF impostato,
mentre se tutti lo superano bisogna aumentare gli step per cercare valori ancora
più alti.

#### Risoluzione dei problemi

Se non compare alcun risultato di analisi, verificare che ffmpeg includa il
filtro `libvmaf`:

```bash
ffmpeg -hide_banner -filters | grep libvmaf
```

L'immagine fornita lo include; alcune build ffmpeg di sistema no. Se l'analisi è
molto lenta, ridurre il preset oppure il numero di segmenti per ogni CRF.

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
