# Architettura

Un solo binario Rust: server HTTP (axum), frontend embedded, motore di
conversione che invoca `ffmpeg`/`ffprobe` come processi figli. Nessun database,
nessun servizio esterno obbligatorio.

## Moduli

| Modulo             | Ruolo                                                                 |
| ------------------ | --------------------------------------------------------------------- |
| `config`           | configurazione TOML + override da variabili d'ambiente, validazione    |
| `models`           | `Job`, `JobFile`, stati, `Progress`, progresso complessivo             |
| `store`            | persistenza: un JSON per job, scritto in modo atomico                  |
| `state`            | `AppState`: registro dei job, percorsi, mutazioni, ripristino a caldo  |
| `worker`           | esecuzione dei job: probe, codifica, remux, stati finali              |
| `ffmpeg::probe`    | `ffprobe` in JSON → durata, frame rate, frame stimati, presenza audio  |
| `ffmpeg::encode`   | righe di comando ffmpeg ed esecuzione con progresso e cancellazione    |
| `ffmpeg::progress` | parser del flusso `-progress` e formattazione delle righe di log       |
| `joblog`           | log di conversione per file: buffer circolare + append su disco        |
| `events`           | canale broadcast degli eventi verso SSE                                |
| `slack`            | notifiche via Incoming Webhook                                         |
| `auth`             | basic auth con confronto a tempo costante                              |
| `api`              | router, handler HTTP, upload, download, SSE, asset del frontend        |

## Catena di conversione

Riproduce `start_av1_docker.sh`, senza la parte SSH:

1. **`ffprobe`** → durata e `avg_frame_rate`, da cui i frame stimati. Un sorgente
   senza durata valida o senza stream video fallisce subito.
2. **Codifica** in Matroska temporaneo con `libsvtav1`:
   `-b:v 0 -crf N -preset P -svtav1-params ... -g 240 -pix_fmt yuv420p10le`,
   audio `libopus`, filtro
   `scale=-2:trunc(min(H\,ih)/2)*2:reset_sar=1`.
   In modalità `target_size` due passaggi (`-pass 1` / `-pass 2`) con `-b:v`
   calcolato dal target.
3. **Remux** in MP4 senza ricodifica: `-c copy -tag:v av01 -movflags +faststart`.
4. **Rinomina atomica** del temporaneo nella cartella di output: il file finale
   compare solo quando è completo e valido.

Un output già presente viene saltato (stato `skipped`) a meno che `overwrite` sia
attivo — come negli script di riferimento.

Il filtro usa `reset_sar=1`, disponibile nella build FFmpeg 9.0.2 inclusa
nell'immagine applicativa. I test end-to-end locali vengono saltati se il binario
di sistema non espone questa opzione o l'encoder `libsvtav1`.

## Progresso

ffmpeg scrive su `-progress pipe:1` blocchi `chiave=valore` chiusi da
`progress=continue|end`, con periodo `-stats_period`. Il parser replica la
funzione awk `show_progress` degli script:

- la percentuale deriva dal tempo elaborato (`out_time_us`), che non regredisce;
- quando il conteggio frame è noto, `frame / frame_totali` spesso è più avanti del
  timestamp: si prende il maggiore dei due;
- non si mostra 100% prima della fine effettiva del processo (tetto a 99,9%);
- l'ETA usa la velocità dichiarata da ffmpeg.

Il progresso del job è la media delle percentuali dei file **pesata sulla
dimensione dei sorgenti**: un file da 4 GB non vale quanto uno da 40 MB.

## Concorrenza e ordine

Il worker di un job elabora i file con `for_each_concurrent(concurrency, …)` sullo
stream dei file in ordine di caricamento. Con `concurrency = 1` si ottiene la
sequenza richiesta: il prossimo file parte al termine del precedente. Il valore è
un default di configurazione, modificabile per singolo job dal frontend.

Un flag atomico per job (`worker_running`) impedisce due worker sullo stesso job.

## Stop e cancellazione

Ogni job ha un canale `watch<bool>`. `stop` lo porta a `true`; il loop di lettura
di ffmpeg lo osserva in `select!`, termina il processo figlio e restituisce
`Cancelled`. I file in corso passano a `cancelled`; i temporanei vengono rimossi
da una guardia `Drop`, quindi anche in caso di errore o panico.

`DELETE` di un job richiede lo stop, attende che il worker si fermi (con un tetto
massimo, per non bloccare la richiesta HTTP) e poi rimuove sorgenti, output, log e
stato.

## Persistenza e ripristino

Lo stato di ogni job è un JSON in `data_dir/jobs/<id>.json`, scritto su file
temporaneo e poi rinominato: un'interruzione non lascia mai un JSON troncato.

All'avvio i job vengono ricaricati. Un job che risulta `running` non ha più il
processo ffmpeg corrispondente: i file `running` tornano `pending` con progresso
azzerato e il job torna `pending`, così l'utente può riavviarlo. Un JSON corrotto
viene segnalato e saltato, senza impedire l'avvio.

## Eventi verso il frontend

Un canale `broadcast` alimenta `GET /api/events` (SSE). Gli snapshot completi
(`job_updated`) accompagnano i cambi di stato; il progresso viaggia su eventi
leggeri (`file_progress`) usati dal frontend per aggiornare solo le barre. Un
client lento perde eventi intermedi ma si riallinea al successivo snapshot.

## Sicurezza

- Basic auth su frontend e API, con confronto delle credenziali a tempo costante.
  `/healthz` resta libero per il probe del container.
- I nomi dei file caricati sono ripuliti e resi univoci: il percorso relativo
  degli upload di cartelle non raggiunge il filesystem, quindi non c'è path
  traversal.
- ffmpeg è invocato senza shell, con argomenti passati come vettore. I parametri
  che finiscono nella riga di comando sono comunque validati contro un alfabeto
  limitato, così un valore come `tune=5; rm -rf /` viene rifiutato in ingresso.
- Il dettaglio degli errori interni resta nei log; al client va un messaggio
  generico.

## Frontend

`src/web/{index.html,style.css,app.js}` sono inclusi nel binario con
`include_str!`: nessun asset da distribuire a parte, nessun build step JavaScript.
Vanilla JS con delega degli eventi, `EventSource` per gli aggiornamenti live e
tema chiaro/scuro via `prefers-color-scheme`.

## Test

- **Unitari** (`cargo test --lib`): parser del progresso, costruzione delle righe
  di comando ffmpeg, calcolo del bitrate a dimensione target, validazione dei
  parametri, sanificazione dei nomi, persistenza, log, auth, progresso pesato.
- **API** (`cargo test --test api`): server reale su porta effimera, senza
  conversioni — auth, validazione, ciclo di vita di job e upload, SSE.
- **End-to-end** (`cargo test --test e2e_conversion`): conversioni reali con
  ffmpeg, dall'upload al download — CRF, dimensione target, sequenzialità con
  `concurrency = 1`, stop a metà codifica, salto dell'output esistente,
  fallimento isolato di un sorgente non valido. Se manca un ffmpeg con
  `libsvtav1` i test si autoescludono con un messaggio.
