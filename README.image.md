# FFmpeg remoto con SVT-AV1

L'immagine locale contiene:

- FFmpeg 8.1.2;
- SVT-AV1 4.2.0;
- libvmaf 3.1.0;
- dav1d 1.5.3 per decodificare e misurare gli output AV1;
- libopus;
- `ffprobe`.

Le versioni e le checksum degli archivi sorgente sono fissate in
`Dockerfile.ffmpeg-svtav1`.

## Build locale

```bash
./build_ffmpeg_image.sh
```

Il build esegue anche un encode minimo con `tune=5`, verifica SVT-AV1 4.2.0
e controlla con due sorgenti identiche che il filtro `libvmaf` produca
realmente un punteggio.

## Immagine pubblicata su GHCR

```bash
docker pull ghcr.io/manprint/av1webconverter-ffmpeg:8.1.2-svt4.2.0-vmaf3.1.0
```

Il workflow attivato dal tag `v1.0.0` pubblica automaticamente questa base
prima dell'immagine applicativa. Per usarla negli script:

```bash
FFMPEG_IMAGE=ghcr.io/manprint/av1webconverter-ffmpeg:8.1.2-svt4.2.0-vmaf3.1.0 \
./execute_docker.sh
```

## Conversione

Lo stesso riferimento immagine deve essere disponibile sull'host locale e su
quello remoto. Il default
`local/ffmpeg-svtav1:8.1.2-svt4.2.0-vmaf3.1.0` è utile per i
test locali; per una conversione reale usare il tag pubblicato su GHCR:

```bash
FFMPEG_IMAGE=ghcr.io/manprint/av1webconverter-ffmpeg:8.1.2-svt4.2.0-vmaf3.1.0 \
./execute_docker.sh
```

Gli output già esistenti vengono saltati. Per sovrascriverli:

```bash
FFMPEG_IMAGE=ghcr.io/manprint/av1webconverter-ffmpeg:8.1.2-svt4.2.0-vmaf3.1.0 \
OVERWRITE=1 \
./execute_docker.sh
```

I parametri principali sono configurabili senza modificare gli script:

```bash
FFMPEG_IMAGE=ghcr.io/manprint/av1webconverter-ffmpeg:8.1.2-svt4.2.0-vmaf3.1.0 \
CRF=45 PRESET=5 SVTAV1_PARAMS=tune=5 \
./execute_docker.sh
```

Il default è `tune=5`, ottimizzato per la qualità percettiva VMAF. Sul
campione reale, interpolato allo stesso bitrate di 119.514 bit/s, ha ottenuto
VMAF 95,076 contro 92,810 di `tune=0`. VMAF-NEG, che neutralizza il vantaggio
dello sharpening, è 90,936 contro 90,691. `tune=0` conserva però un PSNR
superiore (46,314 contro 45,998 dB) e può essere selezionato con
`SVTAV1_PARAMS=tune=0` quando si preferisce la fedeltà matematica.

Le vecchie opzioni `enable-qm=1:qm-min=0` restano escluse perché nei test
hanno ridotto sia SSIM sia PSNR.

## Conversione con dimensione target

Per convertire tutti gli MP4 della directory scegliendo la dimensione finale
di ciascun file:

```bash
FFMPEG_IMAGE=ghcr.io/manprint/av1webconverter-ffmpeg:8.1.2-svt4.2.0-vmaf3.1.0 \
./execute_docker_fixed_dim.sh 300
```

Sono accettati sia `300` sia `300MB`. Per un singolo file:

```bash
FFMPEG_IMAGE=ghcr.io/manprint/av1webconverter-ffmpeg:8.1.2-svt4.2.0-vmaf3.1.0 \
./start_av1_docker_fixed_dim.sh film.mp4 300
```

La codifica usa VBR a due passaggi, preset 3, AV1 10-bit e calcola il bitrate
video sottraendo dal target le tracce audio Opus. Il default conserva l'1%
per l'overhead MP4; è modificabile, ad esempio con
`SIZE_MARGIN_PERCENT=0.5`. SVT-AV1 consente `tune=5` solo in modalità CRF:
la variante a dimensione fissa usa quindi `tune=0`, compatibile con il
two-pass VBR.

La dimensione è un target e può variare leggermente per l'overhead del
contenitore e la precisione del rate control. Se 300 MB è un limite rigido,
usa per esempio `SIZE_MARGIN_PERCENT=2` per mantenere un margine più ampio.

Entrambe le modalità mostrano durante la codifica percentuale, tempo
elaborato, numero di frame, dimensione progressiva del file, bitrate,
velocità ed ETA. L'aggiornamento predefinito avviene ogni 5 secondi; per
cambiarlo, ad esempio:

```bash
PROGRESS_INTERVAL=10 ./execute_docker.sh
PROGRESS_INTERVAL=10 ./execute_docker_fixed_dim.sh 300
```

Il ridimensionamento limita l'altezza a `SCALE_HEIGHT` (480 per default),
mantiene il rapporto d'aspetto visivo del sorgente e normalizza l'output a
pixel quadrati (`SAR 1:1`), evitando DAR errati con sorgenti anamorfiche.

## Codifica locale con Docker

Le varianti locali non usano SSH: `ffprobe`, codifica AV1 e remux MP4
vengono eseguiti nell'immagine Docker configurata con `FFMPEG_IMAGE`.

Qualità costante CRF (bitrate variabile), per tutti gli MP4 della directory:

```bash
./execute_docker_local.sh
```

Per un solo file:

```bash
./start_av1_docker_local.sh film.mp4
```

Dimensione target con VBR a due passaggi, per tutti gli MP4:

```bash
./execute_docker_fixed_dim_local.sh 300
```

Per un solo file:

```bash
./start_av1_docker_fixed_dim_local.sh film.mp4 300
```

Le opzioni `SCALE_HEIGHT`, `PRESET`, `CRF`, `AUDIO_BITRATE`,
`LOCAL_CPUS`, `PROGRESS_INTERVAL`, `OUTPUT_DIR`, `OVERWRITE` e
`FFMPEG_IMAGE` funzionano come nelle varianti remote. `CRF` si applica
soltanto alla modalità a qualità costante; `SIZE_MARGIN_PERCENT` soltanto
alla modalità fixed-size.
