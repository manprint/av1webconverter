# av1webconverter

## Specifiche generali

1. nome app: av1webconverter
2. tipologia: frontend/backend
3. deploy: docker, single container

## Sceta delle tecnologie

Rust, ultima versione. Il frontend deve essere realizzato embedded.
L'applicazione prodotta deve essere self contained, single binary.

## Descrizione

Si vuole realizzare un applicazione specializzata per la codifica video in av1, accessibile tramite un frontend. 
L'applicazione deve usare ffmpeg, versione custom, definita in `Dockerfile.ffmpeg-svtav1`.
Lato backend esistono gli script execute_docker.sh e start_av1_docker.sh come esempio per invocare ffmpeg.
Il backend deve prevedere due cartelle di lavoro: in ed out.
In `in` vanno i file caricati da convertire, on `out` i file convertiti.
L'applicazione verrà usata dockerizzata; quindi nel docker finale deve esserci tutto il necessario.
Al momento gestiamo mp4 come file di input, altri formati predisponiamoci.
Il backend deve avere funzionalità di invio notifiche di terminazione/errore verso slack.
Il backend deve avere dei file di log strutturati ed esaustivi da consultare in caso di debug.

## Frontend 

Il frontend deve essere molto semplice, pulito e moderno.
Il frontend deve essere protetto da un basic auth, con parametri configurabili in config backend.
Le funzionalità devono essere raccolte per job:

1. Job di conversione.
    - il job deve il contenitore logico di un task di conversione, deve essere possibile specificare un nome per il job.
    - Per ogni job, parametri di conversione di default devono essere impostati prendendoli da un file di configurazione del backend. Questi possono essere editati e sovrascritti dal frontend.
    - per ogni job, devo poter fare l'uplaod di file/cartelle (contenenti video).
    - per ogni job devo poter settare i parametri di conversione (come quelli definiti in start_av1_docker.sh, chiaramente la parte di invio al server ssh non ti serve, non dobbiamo usarla).
    - all'interno del job, i file vengono convertiti in sequenza. il prossimo parte al termine de precedente completato.
    - per ogni giorno devo poter vedere, per ogni file, i log di conversione, come mostrati nello script bash di esempio.
    - per ogni job voglio vedere l'avanzamento totale.
    - un job può essere stoppato e rimosso.
    - per ogni job terminato, devo poter scaricare i file convertiti (singolarmente o in selezione multipla, oppure tutti file convertiti nel job)
    - devo poter, in qualsiasi momento elimanre un job in qualsiasi stato esso si trovi e i file sul backend devono essere puliti.
    - all'interno del job, è consentita di default una sola conversione concorrente. questo parametro deve poter essere definito da config backend ed editabile nel frontend.

Iniziamo dall'implementare queste specifiche.
Se hai domande chiedi.
Prevedi test e documentazione.