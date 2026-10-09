# FFmpeg remoto con SVT-AV1

L'immagine locale contiene:

- FFmpeg 9.0.2;
- SVT-AV1 4.2.0;
- libvmaf 3.2.1;
- dav1d 1.5.4 per decodificare e misurare gli output AV1;
- libopus 1.6.1 compilato staticamente;
- `ffprobe`.

Le versioni e le checksum degli archivi sorgente sono fissate in
`Dockerfile.ffmpeg-svtav1`.

## Build locale

```bash
docker build -f Dockerfile.ffmpeg-svtav1 \
  -t local/ffmpeg-svtav1:9.0.2-svt4.2.0-vmaf3.2.1 .
```

La build include FFmpeg 9.0.2, SVT-AV1 4.2.0, libVMAF 3.2.1, dav1d 1.5.4
e libopus 1.6.1 statico. Per verificare rapidamente i componenti:

```bash
docker run --rm --entrypoint sh local/ffmpeg-svtav1:9.0.2-svt4.2.0-vmaf3.2.1 -c \
  'ffmpeg -hide_banner -version | head -1; ffmpeg -hide_banner -filters | grep libvmaf'
```

## Immagine pubblicata su GHCR

```bash
docker pull ghcr.io/manprint/av1webconverter-ffmpeg:9.0.2-svt4.2.0-vmaf3.2.1
```

Il workflow attivato dal tag `v1.0.0` pubblica automaticamente questa base
prima dell'immagine applicativa. Per usarla negli script:

```bash
FFMPEG_IMAGE=ghcr.io/manprint/av1webconverter-ffmpeg:9.0.2-svt4.2.0-vmaf3.2.1 \
./execute_docker.sh
```

## Conversione

Lo stesso riferimento immagine deve essere disponibile sull'host locale e su
quello remoto. Il default
`local/ffmpeg-svtav1:9.0.2-svt4.2.0-vmaf3.2.1` è utile per i
test locali; per una conversione reale usare il tag pubblicato su GHCR:

```bash
FFMPEG_IMAGE=ghcr.io/manprint/av1webconverter-ffmpeg:9.0.2-svt4.2.0-vmaf3.2.1 \
./execute_docker.sh
```

Gli output già esistenti vengono saltati. Per sovrascriverli:

```bash
FFMPEG_IMAGE=ghcr.io/manprint/av1webconverter-ffmpeg:9.0.2-svt4.2.0-vmaf3.2.1 \
OVERWRITE=1 \
./execute_docker.sh
```

I parametri principali sono configurabili senza modificare gli script:

```bash
FFMPEG_IMAGE=ghcr.io/manprint/av1webconverter-ffmpeg:9.0.2-svt4.2.0-vmaf3.2.1 \
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

## Parametri non gestiti dagli script

Gli script presenti in questo repository gestiscono la modalità CRF. La
modalità a dimensione target e le relative impostazioni sono disponibili
nell'applicazione web/API descritta in [README.md](README.md); non esistono
script separati per fixed-size o conversioni locali.

Per cambiare intervallo di progresso nello script CRF:

```bash
PROGRESS_INTERVAL=10 ./execute_docker.sh
```

Il ridimensionamento limita l'altezza a `SCALE_HEIGHT` (480 per default),
mantiene il rapporto d'aspetto visivo del sorgente e normalizza l'output a
pixel quadrati (`SAR 1:1`), evitando DAR errati con sorgenti anamorfiche.

La build include i modelli VMAF v1, fra cui `vmaf_v1.0.16_5d0h` per la visione
su telefono e `vmaf_v1.0.16_3d0h` per la visione su televisore 1080p. Un esempio
di misura usa `model=version=vmaf_v1.0.16_5d0h`.

Le opzioni `SCALE_HEIGHT`, `PRESET`, `CRF`, `AUDIO_BITRATE`, `LOCAL_CPUS`,
`PROGRESS_INTERVAL`, `OUTPUT_DIR`, `OVERWRITE` e `FFMPEG_IMAGE` funzionano come
nelle varianti documentate sopra. `KEYINT_SECONDS` (default 10) fissa la
distanza fra keyframe in secondi: lo script la converte in frame dal frame rate
del sorgente, quindi `-g 600` a 60 fps e `-g 240` a 23.976; con `0` o un frame
rate non leggibile resta `-g 240`. Come l'applicazione, lo script riduce ogni
traccia audio a stereo prima di libopus e marca BT.709 le sorgenti HD senza tag
colore.
