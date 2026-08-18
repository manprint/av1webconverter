# Phase 1 — Immagine: FFmpeg 9.0.1, libvmaf 3.2.0, dav1d 1.5.4, opus statico

> **Intent:** aggiornare `Dockerfile.ffmpeg-svtav1` alle nuove versioni e
> compilare libopus in statico, ottenendo un ffmpeg senza dipendenze dinamiche
> verso libopus.
> **Shippable alone?** sì — l'immagine nuova convive con quella vecchia (tag
> diverso) e nessun altro file del repo la usa ancora dopo questa fase.
> **Preconditions:** nessuna.

## State contract (mandatory)

1. Prima di toccare qualsiasi cosa: leggere [STATE.md](STATE.md) e verificare
   che punti a una sotto-fase di questa fase. Eseguire i comandi di gate
   elencati in STATE.md **§3** e confrontare il risultato con quanto dichiarato
   in §1 e §7; vince il repository, quindi correggere il file se divergono.
2. Dopo **ogni** sotto-fase: aggiornare `STATE.md` (posizione, ledger, file
   toccati, lavoro in sospeso, verifiche, deviazioni, `Next action:`,
   timestamp) e sincronizzare `resume.md`. Una sotto-fase non è conclusa prima
   di questo.
3. Se la sessione termina a metà di una sotto-fase, scrivere in `STATE.md` §6
   esattamente cosa resta a metà prima di fermarsi.

---

## Sub-phases

### 1.1 Aggiornare versioni e checksum delle dipendenze

- **Model:** `agent:gpt5.6-luna`
- **Assignment:** `agent:gpt5.6-luna` — modifica meccanica ma con valori esatti da
  non sbagliare.
- **Files:** `Dockerfile.ffmpeg-svtav1:5-11` (ARG dello stage builder),
  `Dockerfile.ffmpeg-svtav1:116-119` (ARG dello stage finale),
  `Dockerfile.ffmpeg-svtav1:126-129` (LABEL).
- **Change:**
  1. Nello stage builder sostituire i valori:
     - `ARG FFMPEG_VERSION=8.1.2` → `9.0.1`
     - `ARG FFMPEG_SHA256=464beb5e...` → `cf38e0e28c7e5605942c4a77755349b0145804a397af37eb1fb4c77cb237f635` (R1)
     - `ARG VMAF_VERSION=3.1.0` → `3.2.0`
     - `ARG VMAF_SHA256=80090e29...` → `a28f93f3b4fa65601be324587072e32a6a704a304ba7b1aec9b70b3f709bc1dc` (R8)
     - `ARG DAV1D_VERSION=1.5.3` → `1.5.4`
     - `ARG DAV1D_SHA256=732010aa...` → `686616b7c69eb88d44459391ab25cac13b6647a3b288835c5784e71c1514a5c5` (R7)
     - `ARG SVT_AV1_VERSION` e `ARG SVT_AV1_SHA256` restano invariati (D15, R13).
  2. Nello stage finale allineare `ARG FFMPEG_VERSION`, `ARG VMAF_VERSION`,
     `ARG DAV1D_VERSION` agli stessi valori. `SVT_AV1_VERSION` resta 4.2.0.
  3. Non toccare i comandi di build in questa sotto-fase: gli URL sono già
     parametrici sulle `ARG`.
   4. Verificare che nessuna opzione `meson`/`cmake` usata sia stata rimossa:
     libvmaf 3.2.0 conserva `enable_tests`, `enable_docs`, `enable_tools`,
     `built_in_models` e richiede meson >= 0.56.1 (R9); dav1d 1.5.4 conserva
     `enable_tools`, `enable_tests` e richiede meson >= 0.54.0 (R10). Debian
     bookworm fornisce meson 1.0.1, quindi entrambi i vincoli sono soddisfatti:
      nessuna modifica richiesta, annotarlo in `STATE.md` §7 come verifica fatta.
      La build dei modelli VMAF v1 richiede inoltre `-Denable_float=true`,
      aggiunto dopo il primo T-IMG3: con `enable_float=false` il modello si
      carica ma il filtro fallisce su `Speed_chroma_feature_speed_chroma_uv_score`.
- **Unit tests:** nessuno (file di build, non codice Rust).
- **e2e tests:** nessuno in questa sotto-fase; la build reale è la 1.3.
- **Done:** i sei valori sono aggiornati in entrambi gli stage, `grep -n
  "8\.1\.2\|3\.1\.0\|1\.5\.3" Dockerfile.ffmpeg-svtav1` non restituisce nulla,
  + `STATE.md` e `resume.md` aggiornati.

### 1.2 Compilare libopus 1.6.1 in statico e rimuovere la dipendenza dinamica

> **Cambio di comportamento:** dopo questa sotto-fase l'immagine ffmpeg non
> installa più `libopus0` e il binario non lo carica a runtime. La fase 2
> rimuoverà lo stesso pacchetto dall'immagine applicativa; fino a quel momento
> il pacchetto in più non fa danno.

- **Model:** `agent:gpt5.6-luna`
- **Assignment:** `agent:gpt5.6-luna` — implementazione; rileggere in autorevisione la
  rimozione della dipendenza dinamica prima della build.
- **Files:** `Dockerfile.ffmpeg-svtav1:5-11` (nuove ARG),
  `Dockerfile.ffmpeg-svtav1:15-28` (elenco pacchetti apt del builder),
  `Dockerfile.ffmpeg-svtav1:65-84` (blocco di build di libvmaf, dopo il quale
  va inserito quello di opus), `Dockerfile.ffmpeg-svtav1:120-123` (apt dello
  stage finale), `Dockerfile.ffmpeg-svtav1:129` (LABEL description).
- **Change:**
  1. Aggiungere nello stage builder, accanto alle altre ARG di versione:
     ```
     ARG OPUS_VERSION=1.6.1
     ARG OPUS_SHA256=6ffcb593207be92584df15b32466ed64bbec99109f007c82205f0194572411a1
     ```
     (R15)
  2. Rimuovere `libopus-dev` dall'elenco `apt-get install` del builder: opus
     viene ora compilato dai sorgenti. Gli altri pacchetti restano.
  3. Inserire il nuovo blocco di build **dopo** quello di libvmaf e **prima**
     di `ENV PKG_CONFIG_PATH=/opt/ffmpeg/lib/pkgconfig`, nello stesso stile dei
     blocchi esistenti (scarica, verifica sha256, estrai, compila, installa in
     `/opt/ffmpeg`):
     ```
     RUN curl -fL --retry 3 \
             -o opus.tar.gz \
             "https://downloads.xiph.org/releases/opus/opus-${OPUS_VERSION}.tar.gz" \
         && printf '%s  %s\n' "${OPUS_SHA256}" opus.tar.gz | sha256sum -c - \
         && tar -xf opus.tar.gz \
         && cd "opus-${OPUS_VERSION}" \
         && ./configure \
             --prefix=/opt/ffmpeg \
             --libdir=/opt/ffmpeg/lib \
             --disable-shared \
             --enable-static \
             --disable-doc \
             --disable-extra-programs \
         && make -j"$(nproc)" \
         && make install
     ```
     Il tarball 1.6.1 contiene `configure`, `Makefile.in` e supporta le quattro
     opzioni usate (R15): non serve autoreconf.
  4. Nello stage finale rimuovere l'intero `RUN apt-get ... libopus0 ...`: a
     quel punto l'immagine ffmpeg non ha più pacchetti aggiuntivi da
     installare.
  5. Aggiornare la `LABEL org.opencontainers.image.description` includendo
     `libopus ${OPUS_VERSION}` al posto del generico `libopus`, e aggiungere
     `ARG OPUS_VERSION=1.6.1` anche nello stage finale perché la label la usi.
  6. `--extra-libs="-lpthread -lm -lstdc++"` di ffmpeg resta invariato: opus
     statico non richiede altre librerie.
- **Unit tests:** nessuno (file di build).
- **e2e tests:** nessuno qui; T-IMG1 in 1.3 verifica il risultato.
- **Done:** `grep -n libopus Dockerfile.ffmpeg-svtav1` mostra soltanto la ARG,
  il blocco di build, la label e `--enable-libopus`; nessun riferimento a
  `libopus-dev` o `libopus0` + `STATE.md` e `resume.md` aggiornati.

### 1.3 Costruire l'immagine e verificarla (audit delle rimozioni 9.0)

- **Model:** `agent:gpt5.6-luna`
- **Assignment:** `agent:gpt5.6-luna` — esecuzione e raccolta delle prove; rileggere in autorevisione l'audit contro R2.
- **Files:** nessuna modifica di file; i risultati vanno in `STATE.md` §7.
- **Change:**
  1. Costruire l'immagine:
     ```
     docker build -f Dockerfile.ffmpeg-svtav1 \
       -t local/ffmpeg-svtav1:9.0.1-svt4.2.0-vmaf3.2.0 .
     ```
     Se la build fallisce, non aggirare il problema: registrare l'errore
     verbatim in `STATE.md` §7 e correggere la causa nella sotto-fase che l'ha
     introdotta.
  2. **T-IMG1** — contenuto e dipendenze del binario:
     ```
     docker run --rm --entrypoint sh local/ffmpeg-svtav1:9.0.1-svt4.2.0-vmaf3.2.0 -c \
       'ffmpeg -hide_banner -version | head -1; \
        ldd /usr/local/bin/ffmpeg 2>&1 | head; \
        ffmpeg -hide_banner -encoders 2>/dev/null | grep -E "libsvtav1|libopus"; \
        ffmpeg -hide_banner -decoders 2>/dev/null | grep libdav1d; \
        ffmpeg -hide_banner -filters 2>/dev/null | grep libvmaf'
     ```
     Asserzioni: la prima riga contiene `ffmpeg version 9.0.1`; l'output di
     `ldd` non contiene `libopus` (I-3); sono presenti l'encoder `libsvtav1`,
     l'encoder `libopus`, il decoder `libdav1d` e il filtro `libvmaf`.
  3. **T-IMG2** — le opzioni su cui si appoggia l'applicazione esistono ancora
     (I-2):
     ```
     docker run --rm --entrypoint sh local/ffmpeg-svtav1:9.0.1-svt4.2.0-vmaf3.2.0 -c \
       'ffmpeg -hide_banner -h filter=scale | grep reset_sar; \
        ffmpeg -hide_banner -h encoder=libsvtav1 | grep -E "svtav1-params|preset|crf"; \
        ffmpeg -nostdin -hide_banner -loglevel error -nostats -stats_period 1 \
          -progress pipe:1 -y -f lavfi -i testsrc=size=320x240:rate=25:duration=2 \
          -c:v libsvtav1 -preset 12 -crf 40 -f matroska /tmp/t.mkv | \
          grep -E "^(frame|total_size|out_time_us|out_time_ms|bitrate|speed|progress)=" | \
          sort -u'
     ```
     Asserzioni: `reset_sar` presente (R5); `svtav1-params`, `preset`, `crf`
     presenti (R6); l'output di `-progress` contiene tutte e sette le chiavi
     lette da `src/ffmpeg/progress.rs:42-85` (R4).
  4. **T-IMG3** — il modello VMAF v1 è caricabile:
     ```
     docker run --rm --entrypoint sh local/ffmpeg-svtav1:9.0.1-svt4.2.0-vmaf3.2.0 -c \
       'ffmpeg -nostdin -hide_banner -loglevel error -y \
           -f lavfi -i testsrc=size=640x360:rate=25:duration=2 \
           -f lavfi -i testsrc=size=640x360:rate=25:duration=2 \
          -lavfi "[0:v]format=yuv420p[d];[1:v]format=yuv420p[r];[d][r]libvmaf=model=version=vmaf_v1.0.16_5d0h:log_fmt=json:log_path=/tmp/v.json" \
          -f null - && head -c 400 /tmp/v.json'
     ```
     Asserzioni: il comando esce con stato 0 e il JSON contiene `pooled_metrics`
     (R14). Questo prova il nome di modello scelto in D8 prima che la fase 4 lo
     usi come default.
  5. **Audit delle rimozioni 9.0 (R2)**: confrontare l'elenco delle rimozioni
     con gli argomenti effettivamente usati dall'applicazione
     (`src/ffmpeg/encode.rs:50-176`, `src/ffmpeg/probe.rs:27-38`). Le rimozioni
     riguardano `-psnr`, `-map_channel`, filtri NPP, preset NVENC legacy,
     decoder CELT, vecchio handler HLS, codec v308/v408/v410: nessuno di questi
     compare nel codice. La verifica dei certificati TLS attiva per default non
     ci riguarda perché ffmpeg è compilato con `--disable-network`. Registrare
     l'esito in `STATE.md` §7 come riga di verifica, non come deviazione.
- **Unit tests:** nessuno.
- **e2e tests:** T-IMG1, T-IMG2, T-IMG3 come sopra — criterio osservabile: i
  tre comandi escono con stato 0 e le asserzioni elencate sono soddisfatte.
- **Done:** l'immagine `local/ffmpeg-svtav1:9.0.1-svt4.2.0-vmaf3.2.0` esiste,
  T-IMG1/T-IMG2/T-IMG3 passano con l'output registrato in `STATE.md` §7,
  l'audit R2 è registrato + `STATE.md` e `resume.md` aggiornati.

### 1.4 Update README.md

- **Model:** `agent:gpt5.6-luna`
- **Assignment:** `agent:gpt5.6-luna` — documentazione; rileggerla in autorevisione
  nella fase finale del piano.
- **Files:** `README.md:90-100` (blocco di build locale dell'immagine),
  `README.image.md:1-95` (documento dedicato all'immagine ffmpeg).
- **Change:** aggiornare la documentazione utente per ciò che questa fase rende
  effettivamente disponibile, cioè la nuova immagine ffmpeg:
  - `README.image.md`, sezione iniziale con l'elenco dei componenti: FFmpeg
    9.0.1, libvmaf 3.2.0, dav1d 1.5.4, libopus 1.6.1 compilato staticamente
    (nota che l'immagine non richiede più il pacchetto `libopus0`).
  - `README.image.md`, tutti i comandi `docker pull` / `docker build` /
    `FFMPEG_IMAGE=...`: sostituire il tag `8.1.2-svt4.2.0-vmaf3.1.0` con
    `9.0.1-svt4.2.0-vmaf3.2.0`.
  - `README.image.md`, aggiungere alle note che i modelli VMAF v1 sono
    disponibili nella build, incluso il modello telefono
    `vmaf_v1.0.16_5d0h` (distanza di visione 5H, adatto ai video guardati su
    cellulare) e quello standard a 1080p `vmaf_v1.0.16_3d0h`, indicando il
    comando di misura con `model=version=vmaf_v1.0.16_5d0h`.
  - `README.md`, blocco di build locale: stesso aggiornamento di tag.
  - Non toccare le misure di qualità già registrate in `README.image.md`: sono
    lo storico delle misure con FFmpeg 8.1.2 e vanno lasciate come tali,
    indicando accanto la versione con cui furono prese.
  Mantenere struttura, tono e lingua italiana dei file; modificare, non
  riscrivere. Nessun dettaglio implementativo, nessun riferimento al piano o
  alle fasi.
- **Unit tests:** nessuno (documentazione).
- **e2e tests:** nessuno — i comandi documentati vanno eseguiti almeno una
  volta e devono produrre l'output descritto.
- **Done:** un utente nuovo costruisce o scarica l'immagine aggiornata usando
  solo il README, senza leggere il Dockerfile; `grep -rn "8\.1\.2" README.md
  README.image.md` restituisce solo i riferimenti storici alle misure; gates
  verdi; `STATE.md` e `resume.md` aggiornati.

---

## Phase gates

- **Fmt:** non applicabile (nessun codice Rust modificato in questa fase).
- **Lint:** `docker build -f Dockerfile.ffmpeg-svtav1 -t local/ffmpeg-svtav1:9.0.1-svt4.2.0-vmaf3.2.0 .` completa senza errori.
- **Test subset:** T-IMG1, T-IMG2, T-IMG3.
- **Regression guard:** I-2 (opzioni della riga di comando ancora valide), I-3
  (nessuna dipendenza dinamica verso libopus).
- **README:** `README.image.md` e `README.md` aggiornati al nuovo tag e ai nuovi
  componenti, senza dettagli implementativi.

## Phase done criterion

L'immagine `local/ffmpeg-svtav1:9.0.1-svt4.2.0-vmaf3.2.0` è costruita e
contiene FFmpeg 9.0.1 con libsvtav1 4.2.0, libopus 1.6.1 statico, libdav1d
1.5.4 e libvmaf 3.2.0; `ldd` non mostra dipendenze verso libopus; le sette
chiavi di `-progress`, `reset_sar` e le opzioni di libsvtav1 usate dal codice
sono presenti; il modello `vmaf_v1.0.16_5d0h` produce un log JSON valido.
`README.md` e `README.image.md` riflettono ciò che questa fase rende usabile.
