# API HTTP

Tutte le rotte, incluso il frontend, sono protette da basic auth quando
`auth.enabled = true`. Unica eccezione: `GET /healthz`.

Gli errori hanno sempre corpo `{"error": "descrizione"}` con lo stato HTTP
corrispondente: `400` input non valido, `401` credenziali mancanti o errate,
`404` risorsa inesistente, `409` operazione incompatibile con lo stato corrente,
`413` upload oltre il limite, `500` errore interno (il dettaglio resta nei log).

## Salute e configurazione

### `GET /healthz`

Senza autenticazione. `{"status": "ok", "version": "0.1.0"}`.

### `GET /api/defaults`

Valori proposti dal backend a ogni nuovo job.

```json
{
  "params": { "mode": "crf", "crf": 47, "preset": 1, "scale_height": 480, "...": "..." },
  "concurrency": 1,
  "allowed_extensions": ["mp4", "mov", "mkv"],
  "max_upload_mb": 8192
}
```

## Job

Un job è un contenitore logico di conversioni. Il corpo restituito (`JobView`) è
il job più i campi derivati `overall_progress` e `counts`.

```json
{
  "id": "f4a09854-c2a0-40be-8f9c-facce9cc67ad",
  "name": "Serie TV — stagione 1",
  "status": "running",
  "params": { "mode": "crf", "crf": 47, "preset": 1, "...": "..." },
  "concurrency": 1,
  "files": [
    {
      "id": "ad1c9172-4eb9-4c47-a7a1-f962302c8a54",
      "original_name": "stagione1/ep01.mp4",
      "stored_name": "ep01.mp4",
      "input_size_bytes": 4501234567,
      "output_size_bytes": null,
      "status": "running",
      "progress": {
        "percent": 42.3,
        "elapsed_seconds": 51.0,
        "total_seconds": 120.0,
        "frame": 1275,
        "total_frames": 3000,
        "written_bytes": 12400000,
        "bitrate": "1234kbits/s",
        "speed": "2.1x",
        "eta_seconds": 33.0,
        "pass": 1,
        "total_passes": 1
      },
      "error": null,
      "uploaded_at": "2026-07-25T21:31:47.651890991Z",
      "started_at": "2026-07-25T21:31:47.652561718Z",
      "finished_at": null
    }
  ],
  "error": null,
  "created_at": "2026-07-25T21:31:47.648692794Z",
  "started_at": "2026-07-25T21:31:47.652546982Z",
  "finished_at": null,
  "overall_progress": 42.3,
  "counts": { "total": 1, "pending": 0, "running": 1, "completed": 0, "skipped": 0, "failed": 0, "cancelled": 0 }
}
```

Stati del job: `pending`, `running`, `completed`, `failed`, `stopped`.
Stati del file: `pending`, `running`, `completed`, `skipped` (output già presente
con `overwrite` disattivo), `failed`, `cancelled`.

### `GET /api/jobs`

Elenco dei job, dal più recente.

### `POST /api/jobs`

```json
{ "name": "Serie TV", "params": { "crf": 40, "suffix": "_av1" }, "concurrency": 2 }
```

`params` e `concurrency` sono opzionali: se assenti si usano i default del
backend. I campi di `params` omessi restano al loro default. Risponde con la
`JobView` creata.

`params.suffix` viene aggiunto allo stem di ogni output, prima di `.mp4`;
il default è `_av1`. Può contenere lettere e numeri ASCII, `.`, `-` e `_`
(massimo 128 caratteri).

`params.scale_height` è l'altezza massima dell'output in pixel: `0` significa
dimensione originale, cioè nessun ridimensionamento e rapporto dei pixel del
sorgente conservato. Sono ammessi `0` e i valori fra 16 e 4320; l'interfaccia
web propone un sottoinsieme di valori comuni, l'API accetta tutto l'intervallo.

### `GET /api/jobs/{job_id}`

Un singolo job.

### `PATCH /api/jobs/{job_id}`

```json
{ "name": "nuovo nome", "params": { "crf": 45, "preset": 6, "suffix": "_web" }, "concurrency": 3 }
```

Tutti i campi sono opzionali. `params` e `concurrency` non sono modificabili
mentre il job è in esecuzione (`409`): quei valori sono già in uso da ffmpeg.

### `POST /api/jobs/{job_id}/start`

Avvia la conversione dei file in stato `pending`. `400` se il job non ha file o
non ha nulla in attesa, `409` se è già in esecuzione.

### `POST /api/jobs/{job_id}/stop`

Interrompe il job: i processi ffmpeg attivi vengono terminati, i file in corso
passano a `cancelled` e nessun output parziale resta sul disco. I file già
convertiti restano scaricabili. Un job riavviato riparte dai file `pending`.

### `DELETE /api/jobs/{job_id}`

Elimina il job in qualsiasi stato: interrompe il lavoro in corso e rimuove
sorgenti, output, log e stato persistito.

```json
{ "deleted": "f4a09854-c2a0-40be-8f9c-facce9cc67ad" }
```

## File

### `POST /api/jobs/{job_id}/files`

`multipart/form-data`, una o più parti con un `filename`. Per l'upload di
cartelle il frontend usa il percorso relativo come nome della parte: viene
conservato in `original_name` per la visualizzazione, mentre su disco si usa un
nome ripulito e reso univoco (`stored_name`).

I file con estensione non ammessa vengono scartati singolarmente senza far
fallire la richiesta.

```json
{
  "uploaded": 2,
  "rejected": [{ "name": "note.txt", "reason": "estensione .txt non ammessa (ammesse: mp4, mkv)" }],
  "job": { "...": "JobView aggiornata" }
}
```

`409` se il job è in esecuzione. Un job già terminato che riceve nuovi file torna
in stato `pending`.

### `DELETE /api/jobs/{job_id}/files/{file_id}`

Rimuove il file dal job ed elimina sorgente, eventuale output e log. `409` se il
file è in conversione. Risponde con la `JobView` aggiornata.

### `GET /api/jobs/{job_id}/files/{file_id}/log`

Log di conversione del file, nello stesso formato degli script bash.

```json
{
  "file_id": "ad1c9172-...",
  "lines": [
    "2026-07-25T21:31:47Z Avvio conversione: stagione1/ep01.mp4",
    "2026-07-25T21:31:48Z Sorgente: durata 120.00s, 3000 frame stimati, audio presente",
    "2026-07-25T21:31:53Z [Codifica]  42.3% | 00:00:51 / 00:02:00 | frame 1275 | file 12.40 MB | 1234kbits/s | 2.1x | ETA 00:00:33"
  ]
}
```

Sono restituite le ultime `jobs.log_buffer_lines` righe; su disco restano tutte.

## Download

### `GET /api/jobs/{job_id}/files/{file_id}/download`

Il singolo file convertito. `409` se non è ancora stato convertito.

### `GET /api/jobs/{job_id}/download`

Tutti i file convertiti del job, in un archivio ZIP (senza ricompressione).
`409` se non c'è nulla da scaricare.

### `POST /api/jobs/{job_id}/download`

Una selezione di file in ZIP.

```json
{ "file_ids": ["ad1c9172-...", "5ea0f754-..."] }
```

Con `file_ids` vuoto o assente si ottiene l'intero job.

## Eventi

### `GET /api/events`

Stream `text/event-stream`. Ogni messaggio è un JSON con campo `type`:

| `type`                | Contenuto                                                           |
| --------------------- | ------------------------------------------------------------------- |
| `job_updated`         | `job`: snapshot completo della `JobView`                            |
| `job_deleted`         | `job_id`                                                            |
| `job_status_changed`  | `job_id`, `status`                                                  |
| `file_status_changed` | `job_id`, `file_id`, `status`, `error`                              |
| `file_progress`       | `job_id`, `file_id`, `progress`, `overall_progress`                 |
| `file_log`            | `job_id`, `file_id`, `line`                                         |

`file_progress` arriva a ogni `progress_interval` ed è pensato per aggiornare le
barre senza ricostruire l'interfaccia. Un client lento può perdere eventi
intermedi: i successivi `job_updated` lo riportano allineato.
