# Phase 2 — Propagazione tag e riferimenti di versione

> **Intent:** far puntare l'immagine applicativa, il workflow di pubblicazione e
> tutta la documentazione alla nuova base ffmpeg, e correggere i riferimenti di
> versione ormai falsi.
> **Shippable alone?** sì — al termine il repository costruisce e pubblica
> l'immagine applicativa sulla base nuova, senza altre modifiche di
> comportamento.
> **Preconditions:** fase 1 `DONE` (l'immagine
> `local/ffmpeg-svtav1:9.0.1-svt4.2.0-vmaf3.2.0` esiste e ha superato
> T-IMG1/T-IMG2/T-IMG3).

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

### 2.1 Immagine applicativa sulla nuova base

> **Cambio di comportamento:** l'immagine applicativa non installa più
> `libopus0`. È corretto solo se il binario ffmpeg copiato dalla base è quello
> costruito nella fase 1 (opus statico): T-IMG1 lo ha già provato con `ldd`.

- **Model:** `agent:gpt5.6-luna`
- **Assignment:** `agent:gpt5.6-luna` — sostituzioni testuali puntuali.
- **Files:** `Dockerfile:10-15` (commenti con i comandi di build),
  `Dockerfile:17` (`ARG FFMPEG_IMAGE`), `Dockerfile:44-47` (apt dello stage
  runtime).
- **Change:**
  1. Sostituire in `Dockerfile:11`, `Dockerfile:15` e `Dockerfile:17` il tag
     `8.1.2-svt4.2.0-vmaf3.1.0` con `9.0.1-svt4.2.0-vmaf3.2.0` (tre
     occorrenze: due nei commenti, una nel valore di default di
     `ARG FFMPEG_IMAGE`).
  2. Nello stage runtime togliere `libopus0` dalla riga `apt-get install`,
     lasciando `ca-certificates` e `curl`.
  3. Aggiornare il commento sopra quel comando: oggi dice che `libopus0` è
     l'unica libreria dinamica richiesta dal ffmpeg statico; ora deve dire che
     ffmpeg è completamente statico e che restano solo `ca-certificates` per le
     notifiche Slack in HTTPS e `curl` per l'healthcheck.
  4. `ARG RUST_IMAGE=rust:1.96-bookworm` resta invariato (D6).
- **Unit tests:** nessuno (file di build).
- **e2e tests:** verifica di costruzione e avvio:
  ```
  docker build --build-arg FFMPEG_IMAGE=local/ffmpeg-svtav1:9.0.1-svt4.2.0-vmaf3.2.0 \
    -t av1webconverter:local .
  docker run --rm --entrypoint sh av1webconverter:local -c \
    'ffmpeg -hide_banner -version | head -1; ldd /usr/local/bin/ffmpeg 2>&1 | grep -c libopus'
  ```
  Asserzioni: la build completa; `ffmpeg version 9.0.1`; il conteggio delle
  righe con `libopus` è `0` (I-3).
- **Done:** `grep -n "8\.1\.2" Dockerfile` non restituisce nulla, l'immagine
  applicativa si costruisce e il suo ffmpeg parte senza `libopus0` installato +
  `STATE.md` e `resume.md` aggiornati.

### 2.2 Workflow, script di riferimento e documentazione tecnica

- **Model:** `agent:gpt5.6-luna`
- **Assignment:** `agent:gpt5.6-luna` — sostituzioni puntuali; rileggere in autorevisione la correzione della nota su `reset_sar`, che oggi è falsa.
- **Files:** `.github/workflows/docker-publish.yml:29`,
  `start_av1_docker.sh:19`, `docs/architettura.md:44-46`, `CLAUDE.md:52`.
- **Change:**
  1. `.github/workflows/docker-publish.yml:29`: tag della base ffmpeg da
     `8.1.2-svt4.2.0-vmaf3.1.0` a `9.0.1-svt4.2.0-vmaf3.2.0`. Nessun'altra
     modifica al workflow: costruisce già `Dockerfile.ffmpeg-svtav1` e poi
     l'immagine applicativa passando `FFMPEG_IMAGE`.
  2. `start_av1_docker.sh:19`: aggiornare il tag citato nel commento. Lo script
     resta il riferimento del contratto di codifica, la sua metà SSH resta
     fuori scopo: non modificare nient'altro.
  3. `docs/architettura.md:44-46`: la nota dice che `reset_sar=1` è disponibile
     nella build FFmpeg 8.1.2; aggiornarla a 9.0.1 mantenendo l'informazione
     utile, cioè che l'opzione è presente nell'immagine applicativa e che gli
     e2e locali si saltano se il binario di sistema non la espone o non ha
     `libsvtav1`.
  4. `CLAUDE.md:52`: la voce "The scale filter deliberately differs from the
     bash scripts" è **falsa** rispetto al codice attuale
     (`src/ffmpeg/encode.rs:47` emette `scale=-2:trunc(min(H\,ih)/2)*2:reset_sar=1`,
     quindi usa proprio `reset_sar`). Riscrivere la voce in modo veritiero: il
     filtro usa `reset_sar=1`, opzione introdotta in ffmpeg 7.1 e presente
     nell'immagine (FFmpeg 9.0.1, R5); la virgola dentro `min()` resta
     protetta; con `scale_height == 0` il filtro non viene passato affatto.
     Aggiornare nella stessa passata gli eventuali riferimenti a versione nel
     resto del file, se presenti.
- **Unit tests:** nessuno (documentazione e configurazione CI).
- **e2e tests:** nessuno — verificare la sintassi del workflow con
  `python3 -c "import yaml,sys;yaml.safe_load(open('.github/workflows/docker-publish.yml'))"`.
- **Done:** `grep -rn "8\.1\.2\|vmaf3\.1\.0" .github start_av1_docker.sh docs
  CLAUDE.md` non restituisce nulla (salvo riferimenti storici espliciti alle
  misure), la nota su `reset_sar` in `CLAUDE.md` descrive il codice reale, il
  workflow resta YAML valido + `STATE.md` e `resume.md` aggiornati.

### 2.3 Update README.md

- **Model:** `agent:gpt5.6-luna`
- **Assignment:** `agent:gpt5.6-luna` — documentazione.
- **Files:** `README.md` (sezioni di installazione ed esecuzione con Docker),
  `README.image.md` (riferimenti residui al tag).
- **Change:** completare l'aggiornamento cominciato in 1.4 per la parte
  applicativa:
  - `README.md`, sezione con `docker pull` / `docker compose up` e ogni comando
    che nomina l'immagine ffmpeg: tag `9.0.1-svt4.2.0-vmaf3.2.0`.
  - `README.md`, note o requisiti che citano `libopus0` come pacchetto
    necessario: rimuoverli, l'immagine non ne ha più bisogno.
  - Se il README indica la versione di FFmpeg fornita, aggiornarla a 9.0.1 e
    citare libvmaf 3.2.0 e dav1d 1.5.4.
  Preservare struttura, tono e lingua; modificare, non riscrivere. Solo
  comportamento già disponibile: la misura VMAF nell'applicazione **non** va
  documentata qui, arriva nelle fasi 4 e 5.
- **Unit tests:** nessuno (documentazione).
- **e2e tests:** nessuno — i comandi documentati vanno eseguiti almeno una
  volta.
- **Done:** un utente nuovo scarica o costruisce entrambe le immagini e avvia
  l'applicazione seguendo solo il README; nessun dettaglio implementativo;
  nessun riferimento residuo al vecchio tag; `STATE.md` e `resume.md`
  aggiornati.

---

## Phase gates

- **Fmt:** non applicabile (nessun codice Rust modificato).
- **Lint:** `docker build --build-arg FFMPEG_IMAGE=local/ffmpeg-svtav1:9.0.1-svt4.2.0-vmaf3.2.0 -t av1webconverter:local .` completa senza errori.
- **Test subset:** verifica di 2.1 (`ldd` senza libopus, `ffmpeg version 9.0.1`
  dentro l'immagine applicativa) e YAML valido per il workflow.
- **Regression guard:** I-3.
- **README:** `README.md` e `README.image.md` allineati al nuovo tag e privi di
  riferimenti a `libopus0`.

## Phase done criterion

Costruendo con i comandi documentati nel README si ottiene un'immagine
applicativa basata su `9.0.1-svt4.2.0-vmaf3.2.0`, il workflow pubblica quello
stesso tag, e nessun file del repository afferma più versioni o comportamenti
non veri (incluse la nota su `reset_sar` in `CLAUDE.md` e quella in
`docs/architettura.md`).
