# Phase 3 — Verifica di regressione con la nuova toolchain

> **Intent:** dimostrare che la conversione non regredisce con FFmpeg 9.0.1,
> eseguendo l'intera suite end-to-end contro i binari della nuova immagine e una
> conversione reale dentro il container applicativo.
> **Shippable alone?** sì — non modifica codice di produzione, aggiunge solo
> prove e un helper di test.
> **Preconditions:** fase 2 `DONE` (immagine applicativa `av1webconverter:local`
> costruita sulla base `9.0.1-svt4.2.0-vmaf3.2.0`).

## State contract (mandatory)

1. Prima di toccare qualsiasi cosa: leggere [STATE.md](STATE.md) e verificare
   che punti a una sotto-fase di questa fase. Eseguire i comandi di gate
   elencati in STATE.md **§3** e confrontare il risultato con quanto dichiarato
   in §1 e §7; vince il repository, quindi correggere il file se divergono.
2. Dopo **ogni** sotto-fase: aggiornare `STATE.md` (posizione, ledger, file
   toccati, lavoro in sospeso, verifiche, deviazioni, `Next action:`,
   timestamp) e sincronizzare `resume.md`.
3. Se la sessione termina a metà di una sotto-fase, scrivere in `STATE.md` §6
   esattamente cosa resta a metà prima di fermarsi.

---

## Sub-phases

### 3.1 Suite end-to-end contro i binari di FFmpeg 9.0.1

- **Model:** `agent:gpt5.6-luna`
- **Assignment:** `agent:gpt5.6-luna` — esecuzione e diagnosi degli eventuali
  fallimenti; approvare in autorevisione le asserzioni di accettazione della
  regressione.
- **Files:** nessuna modifica di codice richiesta se la suite passa; gli esiti
  vanno in `STATE.md` §7. Se un test va adattato, il file è
  `tests/e2e_conversion.rs` e la modifica va segnalata come deviazione in
  `STATE.md` §8.
- **Change:**
  1. Estrarre i binari dall'immagine ffmpeg in una cartella già ignorata da git
     (`target/`), senza creare nuove cartelle di primo livello:
     ```
     mkdir -p target/ffmpeg9/bin
     cid=$(docker create local/ffmpeg-svtav1:9.0.1-svt4.2.0-vmaf3.2.0)
     docker cp "$cid:/usr/local/bin/ffmpeg"  target/ffmpeg9/bin/ffmpeg
     docker cp "$cid:/usr/local/bin/ffprobe" target/ffmpeg9/bin/ffprobe
     docker rm "$cid"
     target/ffmpeg9/bin/ffmpeg -hide_banner -version | head -1
     ```
     L'ultima riga deve stampare `ffmpeg version 9.0.1`. I binari sono statici,
     quindi girano sull'host senza le librerie dell'immagine.
  2. **T-E2E-ALL** — eseguire tutta la suite end-to-end con quei binari in
     testa al `PATH` (gli helper di test invocano `ffmpeg` e `ffprobe` dal
     `PATH`, `tests/common/mod.rs:94-115`, quindi il `PATH` è la leva corretta,
     non `AV1WC_FFMPEG_BIN`):
     ```
     PATH="$PWD/target/ffmpeg9/bin:$PATH" cargo test --test e2e_conversion
     ```
     Asserzione: tutti e otto i test passano, nessuno salta per assenza di
     `libsvtav1`. I test coprono già CRF dall'upload al download, sequenzialità,
     stop di un job in corso, output già presente, dimensione target a due
     passaggi, sorgente anamorfico, dimensione originale e sorgente non valido:
     è la copertura che prova I-2 sul campo.
  3. **T-REG1** — parametri di produzione con SVT-AV1 4.2: la configurazione di
     test forza `svtav1_params = "tune=0"` (`tests/common/mod.rs:53`) perché
     l'SVT di sistema è spesso 1.x. Verificare a mano che il default di
     produzione `tune=5` funzioni con la nuova toolchain:
     ```
     target/ffmpeg9/bin/ffmpeg -nostdin -hide_banner -loglevel error -y \
       -f lavfi -i testsrc=size=640x360:rate=25:duration=3 \
       -c:v libsvtav1 -preset 6 -svtav1-params tune=5 -b:v 0 -crf 32 \
       -pix_fmt yuv420p10le -f matroska /tmp/tune5.mkv
     ```
     Asserzione: stato di uscita 0 e file non vuoto (D15 conferma SVT-AV1 4.2.0,
     che accetta `tune=5` in CRF).
  4. **T-REG2** — due passaggi con `tune` forzato a 0: verificare che il vincolo
     documentato in `src/config.rs:137-141` regga anche con FFmpeg 9:
     ```
     target/ffmpeg9/bin/ffmpeg -nostdin -hide_banner -loglevel error -y \
       -f lavfi -i testsrc=size=640x360:rate=25:duration=3 \
       -c:v libsvtav1 -preset 8 -svtav1-params tune=0 -b:v 800k -pass 1 \
       -passlogfile /tmp/p1 -an -f null /dev/null
     ```
     Asserzione: stato di uscita 0. Se invece fallisce, è una regressione reale
     da riportare come blocco in `STATE.md` §9, non da aggirare.
  5. Registrare in `STATE.md` §7 una riga per T-E2E-ALL, T-REG1, T-REG2 con
     comando, esito e data. In caso di fallimento incollare l'output verbatim
     ridotto all'errore.
- **Unit tests:** `cargo test --lib` deve restare verde (nessuna modifica
  attesa, serve come controllo che l'ambiente sia sano).
- **e2e tests:** T-E2E-ALL, T-REG1, T-REG2 come sopra.
- **Done:** i tre esiti sono registrati in `STATE.md` §7, la suite passa senza
  test saltati, nessuna modifica al codice di produzione + `STATE.md` e
  `resume.md` aggiornati.

### 3.2 Conversione reale dentro il container applicativo

- **Model:** `agent:gpt5.6-luna`
- **Assignment:** `agent:gpt5.6-luna` — esecuzione.
- **Files:** nessuna modifica; esiti in `STATE.md` §7.
- **Change:**
  1. Preparare un clip di prova e una cartella dati temporanea:
     ```
     mkdir -p /tmp/av1wc-smoke/{in,out,state,logs}
     target/ffmpeg9/bin/ffmpeg -y -f lavfi -i testsrc=size=1920x1080:rate=25:duration=10 \
       -f lavfi -i sine=frequency=440:duration=10 \
       -c:v libx264 -preset veryfast -c:a aac -shortest /tmp/av1wc-smoke/clip.mp4
     ```
  2. Avviare il container applicativo direttamente (senza compose, per non
     dipendere dai default di `compose.yaml`):
     ```
     docker run -d --name av1wc-smoke -p 18080:8080 \
       -e AV1WC_AUTH_ENABLED=false \
       -v /tmp/av1wc-smoke:/data av1webconverter:local
     curl -fsS http://127.0.0.1:18080/healthz
     ```
     Asserzione: `/healthz` risponde 200.
  3. Creare un job, caricare il clip, avviarlo e attendere il termine:
     ```
     job=$(curl -fsS -X POST http://127.0.0.1:18080/api/jobs \
       -H 'content-type: application/json' \
       -d '{"name":"smoke 9.0.1","params":{"scale_height":720}}' | python3 -c 'import sys,json;print(json.load(sys.stdin)["id"])')
     curl -fsS -X POST "http://127.0.0.1:18080/api/jobs/$job/files" \
       -F "file=@/tmp/av1wc-smoke/clip.mp4"
     curl -fsS -X POST "http://127.0.0.1:18080/api/jobs/$job/start"
     # attendere lo stato terminale
     until curl -fsS "http://127.0.0.1:18080/api/jobs/$job" | grep -q '"status":"completed"'; do sleep 5; done
     ```
  4. Verificare l'output prodotto:
     ```
     out=$(ls /tmp/av1wc-smoke/out/*/*.mp4 | head -1)
     target/ffmpeg9/bin/ffprobe -v error -select_streams v:0 \
       -show_entries stream=codec_name,codec_tag_string,height -of csv=p=0 "$out"
     ```
     Asserzioni: `codec_name` è `av1`, `codec_tag_string` è `av01`, l'altezza è
     `720`.
  5. Pulire: `docker rm -f av1wc-smoke` e rimozione di `/tmp/av1wc-smoke` se
     non serve più.
- **Unit tests:** nessuno.
- **e2e tests:** T-SMOKE1 — il job passa a `completed` e l'MP4 prodotto è AV1
  con tag `av01` all'altezza richiesta, prodotto dal container reale.
- **Done:** T-SMOKE1 registrato in `STATE.md` §7 con l'output di `ffprobe`, il
  container di prova rimosso + `STATE.md` e `resume.md` aggiornati.

### 3.3 Update README.md

- **Model:** `agent:gpt5.6-luna`
- **Assignment:** `agent:gpt5.6-luna` — documentazione.
- **Files:** `README.md` (sezione dei test, se presente; altrimenti la sezione
  sullo sviluppo).
- **Change:** questa fase non introduce comportamento visibile all'utente
  finale: verificare che il README resti accurato e limitarsi a due
  integrazioni utili a chi esegue i test:
  - come eseguire la suite end-to-end con i binari dell'immagine
    (`PATH="$PWD/target/ffmpeg9/bin:$PATH" cargo test --test e2e_conversion`) e
    perché serve il `PATH` e non una variabile d'ambiente dell'applicazione;
  - la nota che i test end-to-end si saltano da soli se l'ffmpeg di sistema non
    ha `libsvtav1`.
  Se il README non ha una sezione sui test e il progetto documenta i comandi
  altrove, aggiornare quel documento invece di crearne uno nuovo, e registrarlo
  in `STATE.md`. Nessun dettaglio implementativo, nessun riferimento al piano.
- **Unit tests:** nessuno (documentazione).
- **e2e tests:** nessuno — i comandi documentati vanno eseguiti almeno una
  volta.
- **Done:** chi legge il README sa eseguire la verifica di regressione senza
  chiedere nulla; gates verdi; `STATE.md` e `resume.md` aggiornati.

---

## Phase gates

- **Fmt:** `cargo fmt --all -- --check`
- **Lint:** `cargo clippy --all-targets -- -D warnings`
- **Test subset:** `cargo test --lib`, `cargo test --test api`,
  `PATH="$PWD/target/ffmpeg9/bin:$PATH" cargo test --test e2e_conversion`
- **Regression guard:** T-E2E-ALL (tutti gli otto test end-to-end esistenti),
  I-2.
- **README:** aggiornato con la procedura di verifica, o esplicitamente
  verificato come già accurato.

## Phase done criterion

L'intera suite `e2e_conversion` passa con i binari di FFmpeg 9.0.1 senza
adattamenti al codice di produzione, `tune=5` in CRF e `tune=0` a due passaggi
funzionano con la nuova toolchain, e una conversione reale eseguita dentro il
container applicativo produce un MP4 AV1 con tag `av01` all'altezza richiesta.
