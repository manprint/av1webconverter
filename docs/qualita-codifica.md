# Qualità di codifica

Questo documento raccoglie le misure su SVT-AV1 4.2.0 che hanno guidato i
default di codifica e le modifiche introdotte di conseguenza. Il criterio è la
massima qualità percepita con file piccoli: un'opzione che ingrandisce il file è
accettata solo se la qualità che aggiunge vale più dei byte che costa.

## Decisioni in breve

| Opzione | Esito | Motivo |
| ------- | ----- | ------ |
| `tune=5` | resta il default | `tune=0` riduce il file solo dell'1,9% a parità di CRF e perde il 6–8,5% su VMAF-NEG e VMAF v1 |
| `enable-variance-boost=1` | escluso | +46–56% di file a parità di CRF; a parità di dimensione guadagna solo su XPSNR, resta in pari su SSIMULACRA2 e perde su VMAF |
| `qp-scale-compress-strength`, `tf-strength`, `ac-bias`, `sharpness` | esclusi | BD-rate su VMAF peggiore del 13–19% |
| `enable-qm=1` | resta escluso | non sposta i risultati; le misure precedenti mostravano SSIM e PSNR in calo |
| GOP di 10 s a ogni frame rate (`keyint_seconds`) | adottato | un GOP da 4 s, quello di un sorgente a 60 fps con `keyint = 240`, costa il 14% di file e peggiora la qualità |
| `film-grain=8` | opzione per job | elimina il banding (CAMBI medio da 1,09 a 0,01) ma costa il 19% di file, il 41% su una scena statica |
| downmix stereo dell'audio | adottato | i sorgenti AC3, E-AC3 e DTS 5.1 facevano fallire l'intero file |
| tag BT.709 sulle sorgenti HD senza tag | adottato | corregge i colori nei player senza aggiungere byte |
| modello VMAF HFR da 45 fps | adottato | il modello standard sottostima di 0,75 punti un sorgente a 60 fps |

## Metodo

### Clip

Tre spezzoni da 10 s a 24 fps, scelti per contenuti diversi:

| Clip | Origine | Contenuto |
| ---- | ------- | --------- |
| `tos` | *Tears of Steel* 720p, da 3:15 | riprese dal vero con effetti: volti, interni illuminati da lampade, dettaglio fine |
| `snow` | *Sintel* 1080p, da 1:35 | titoli su un paesaggio innevato: quasi statico, grandi campiture chiare e sfumate |
| `dark` | *Sintel* 1080p, da 8:35 | caverna del drago: scena molto scura con gradienti e rumore |

I sorgenti sono stati ridotti a 480 righe, l'altezza di default
dell'applicazione (1150×480 per `tos`, 1126×480 per gli altri due), e salvati
come riferimento YUV 4:2:0 a 10 bit.

### Codifica

```bash
ffmpeg -i ref.y4m -c:v libsvtav1 -preset 1 -svtav1-params "$PARAMS" -g 240 \
  -pix_fmt yuv420p10le -crf "$CRF" -b:v 0 -an -f matroska out.mkv
```

`-preset 1` e CRF 47 sono i default dell'applicazione. Ogni configurazione è
stata codificata a CRF 39, 43, 47, 51 e 55; `tune=5` anche a CRF 35 e le
configurazioni con variance boost anche a 59 e 63, per arrivare alla dimensione
di `tune=5` a CRF 47. In totale 147 codifiche, con FFmpeg 9.0.1, SVT-AV1 4.2.0
e libvmaf 3.2.0: lo stesso encoder dell'immagine attuale.

| Nome | `svtav1_params` |
| ---- | --------------- |
| `t5` | `tune=5` (default) |
| `t0` | `tune=0` |
| `t0vb` | `tune=0:enable-variance-boost=1` |
| `t5vb` | `tune=5:enable-variance-boost=1` |
| `t0psy` | `tune=0:enable-variance-boost=1:qp-scale-compress-strength=1:tf-strength=1` |
| `t0psyac` | `t0psy` + `ac-bias=1.0:sharpness=1` |
| `t0psyqm` | `t0psyac` + `enable-qm=1` |
| `t5fg8` | `tune=5:film-grain=8`, solo CRF 47 |
| `t5g96` | `tune=5:keyint=96`, cioè un GOP di 4 s a 24 fps, solo CRF 47 |

### Metriche

- **SSIMULACRA2**: media dei frame e 10° percentile, che pesa i frame peggiori.
- **XPSNR**: filtro `xpsnr` di FFmpeg, piani Y, U e V pesati 6:1:1.
- **VMAF-NEG** (`vmaf_v0.6.1neg`): VMAF che non premia sharpening e aumento di
  contrasto; verifica che il vantaggio di `tune=5` non venga solo da lì.
- **VMAF v1** (`vmaf_v1.0.16_5d0h`): il modello di default dell'applicazione.
- **CAMBI**: banding, 0 = nessun banding.

Il VMAF v0.6.1 classico non è usato nei confronti, perché premia sharpening e
contrasto.

Due dettagli cambiano i numeri. Il Matroska arrotonda i timestamp al
millisecondo e a 24 fps un frame su tre cade appena prima del suo gemello nel
`.y4m`: con la sincronizzazione di default FFmpeg lo confronta con il frame
precedente, e il VMAF-NEG di `tos` a CRF 47 scende da 90,1 a 75,2. VMAF usa
quindi `ts_sync_mode=nearest`, mentre XPSNR e SSIMULACRA2 leggono il distorto
decodificato in `.y4m` e confrontano i frame per indice. Il filtro `xpsnr`
calcola inoltre i pesi dall'attività del primo input, che deve essere il
riferimento; libvmaf vuole invece per primo il distorto. Con gli input
invertiti la grana sintetica guadagna 0,7 dB di XPSNR invece di perderne 1,2.

### Lettura dei risultati

Per ogni configurazione e clip la curva qualità/bitrate è interpolata (PCHIP sul
logaritmo del bitrate). Tre letture:

- **A parità di CRF**: variazione di dimensione rispetto a `tune=5` a CRF 47 e
  **valore** della qualità ottenuta, cioè quanto file in più servirebbe a
  `tune=5` per raggiungerla. Un'opzione che ingrandisce il file conviene solo se
  il valore supera l'aumento di dimensione.
- **A parità di dimensione**: CRF con cui la configurazione produce un file
  grande quanto `tune=5` a CRF 47, e valore della qualità che ottiene. Un valore
  positivo è qualità guadagnata gratis.
- **BD-rate**: differenza media di bitrate a pari qualità sull'intervallo comune
  delle curve. Negativo = file più piccolo a pari qualità.

I valori sono medie delle tre clip. Riferimento: `tune=5` a CRF 47 produce 289,
38 e 193 kbit/s (`tos`, `snow`, `dark`); un punto di CRF vale circa il 4,5% di
dimensione.

## Risultati

### A parità di CRF (47)

Dimensione e valore della qualità in % di file di `tune=5`; CAMBI come
differenza assoluta (negativo = meno banding).

| Config | Dimensione | SSIMULACRA2 | XPSNR | VMAF-NEG | VMAF v1 | CAMBI |
| ------ | ---------: | ----------: | ----: | -------: | ------: | ----: |
| `t0` | −1,9% | −0,8% | −1,7% | −6,1% | −8,5% | −0,07 |
| `t0vb` | +55,9% | +51,8% | +54,5%¹ | +38,3% | +37,2% | −0,13 |
| `t5vb` | +46,3% | +48,1% | +54,2% | +39,8% | +39,5% | −0,04 |
| `t0psy` | +42,9% | +37,7% | +38,2%¹ | +22,6% | +20,0% | −0,12 |
| `t0psyac` | +46,5% | +40,0% | +39,9%¹ | +27,9% | +27,5% | −0,24 |
| `t0psyqm` | +46,0% | +40,8% | +39,9%¹ | +27,7% | +28,2% | −0,24 |
| `t5fg8` | +19,3% | −1,4% | −24,4% | −0,4% | +8,1% | −1,07 |
| `t5g96` | +14,2% | −4,0% | −6,0% | −3,3% | −2,7% | +0,01 |

¹ Media di `tos` e `dark`, dove il file cresce del 42% (`t0vb`), 29%
(`t0psy`), 33% (`t0psyac`) e 32% (`t0psyqm`): su `snow` la qualità supera
quella del punto più alto misurato per `tune=5` (CRF 35).

Con il variance boost il valore supera l'aumento di dimensione su XPSNR, con
ogni variante, e su SSIMULACRA2 solo con `t5vb`, di 2 punti; mai su VMAF-NEG e
VMAF v1. Sulla scena statica `snow` il file cresce dal 70 all'84%.

### A parità di dimensione

| Config | CRF | SSIMULACRA2 | SSIMULACRA2 p10 | XPSNR | VMAF-NEG | VMAF v1 | CAMBI |
| ------ | --: | ----------: | --------------: | ----: | -------: | ------: | ----: |
| `t0` | 46,6 | +1,1% | −0,2% | +0,3% | −4,3% | −6,7% | −0,07 |
| `t0vb` | 55,1 | +1,3% | −0,8% | +14,1% | −10,1% | −13,4% | −0,12 |
| `t5vb` | 54,4 | +3,2% | +0,9% | +8,6% | −4,4% | −5,9% | −0,02 |
| `t0psy` | 53,6 | +0,2% | −0,4% | +10,7% | −12,3% | −15,6% | −0,13 |
| `t0psyac` | 54,0 | −0,3% | −0,9% | +10,2% | −11,4% | −15,3% | −0,19 |
| `t0psyqm` | 54,0 | +0,1% | +0,2% | +9,8% | −11,0% | −15,2% | −0,20 |

A pari byte le varianti con variance boost guadagnano su XPSNR (+9–14%),
restano in pari su SSIMULACRA2 (fra −0,3 e +3,2%) e perdono su VMAF-NEG e
VMAF v1 (dal 4 al 16%). XPSNR pesa di più l'errore nelle zone piatte, proprio
quelle a cui il variance boost sposta i bit; le altre due metriche non vedono lo
stesso guadagno. `t5vb` è la più vicina al pareggio, ma su `snow` perde su tutte
le metriche. Per arrivare a quella dimensione servirebbe inoltre un CRF di 53–55
al posto di 47.

### BD-rate rispetto a `tune=5`

| Config | SSIMULACRA2 | XPSNR | VMAF-NEG | VMAF v1 |
| ------ | ----------: | ----: | -------: | ------: |
| `t0` | −0,5% | −1,1% | +4,5% | +7,0% |
| `t0vb` | −0,2% | −11,6% | +12,0% | +15,6% |
| `t5vb` | −2,4% | −7,6% | +4,6% | +6,3% |
| `t0psy` | +0,9% | −9,0% | +15,0% | +18,8% |
| `t0psyac` | +1,5% | −8,2% | +13,6% | +17,7% |
| `t0psyqm` | +1,2% | −8,4% | +13,0% | +17,0% |

`tune=0` è in pari su SSIMULACRA2 e XPSNR ma chiede dal 4,5 al 7% di bitrate in
più a pari VMAF: `tune=5` resta il default. Le varianti con variance boost
risparmiano l'8–12% a pari XPSNR e lo pagano con il 5–19% in più a pari VMAF. Fra
`t0psyac` e `t0psyqm` le differenze restano sotto il punto percentuale:
`enable-qm=1` non cambia l'esito.

## Modifiche adottate

### GOP in secondi

`t5g96` mette un keyframe ogni 96 frame, 4 s a 24 fps: è il GOP che otteneva un
sorgente a 60 fps con il vecchio `keyint = 240` fisso. Rispetto ai 10 s di
`-g 240`:

| Clip | Bitrate | SSIMULACRA2 |
| ---- | ------- | ----------- |
| `tos` | 289 → 301 kbit/s (+4,0%) | 60,4 → 59,2 |
| `snow` | 38 → 50 kbit/s (+33,9%) | 81,0 → 81,0 |
| `dark` | 193 → 202 kbit/s (+4,9%) | 66,4 → 64,6 |

Il file cresce e la qualità non migliora; su `tos` e `dark` peggiora.
`keyint_seconds = 10` (`EncodingParams::effective_keyint`) rende il GOP
indipendente dal frame rate: `-g = round(fps × 10)`, quindi `-g 240` a 24 fps
come prima e `-g 600` a 60 fps invece di 240. Con frame rate assente o fuori da
1–1000 fps, o con `keyint_seconds = 0`, vale `keyint`. Il prezzo è un seek fino a
10 s dal keyframe precedente, come già accadeva a 24 fps; per un seek più
reattivo basta abbassare `keyint_seconds`.

### Audio stereo

FFmpeg 9 con libopus rifiuta il layout 5.1(side) di AC3, E-AC3 e DTS: prima della
modifica questi sorgenti fallivano per intero. Ora ogni traccia passa da
`aformat=channel_layouts=mono|stereo` (`AUDIO_DOWNMIX_FILTER`) prima di libopus:
diventa stereo, il mono resta mono. A 48 kbit/s per traccia uno stereo suona
meglio di sei canali compressi. Lo script `start_av1_docker.sh` applica lo
stesso filtro.

In modalità `target_size` il bitrate video riserva `audio_bitrate` a ogni
traccia audio del sorgente, perché `-map 0:a?` le ricodifica tutte; prima ne
contava una sola e un file con più tracce superava il target.

### Tag colore BT.709

Molte sorgenti HD non dichiarano matrice, primari e trasferimento. Senza tag i
player scelgono la matrice dalla risoluzione: un HD ridotto a 480p viene
mostrato come BT.601, con rossi e verdi spostati. Per le sorgenti HD (almeno
1280 pixel di larghezza o 720 di altezza) `color_tag_filter` chiude la catena
`-vf` con un `setparams` che aggiunge i tag BT.709 mancanti. `setparams` cambia
solo i metadati: i frame decodificati restano identici bit per bit e il file non
cresce. Se un tag presente non è BT.709, la sorgente resta com'è.

Le opzioni di output `-colorspace`, `-color_primaries` e `-color_trc` non sono
un'alternativa: fanno convertire i pixel a `scale` e marcano solo la matrice.

Durante le prove è emerso che con tag diversi fra distorto e riferimento FFmpeg 9
converte uno dei due rami prima di libvmaf, e il punteggio crolla. Test su un
1280×720 a 30 fps (`testsrc2`), CRF 40, modello `vmaf_v1.0.16_5d0h`:

| Output | Riferimento senza tag | Riferimento con tag BT.709 |
| ------ | --------------------: | -------------------------: |
| 480p senza tag | 94,38 | — |
| 480p con tag | 51,61 | 94,38 |
| 720p senza tag | 91,60 | 69,12 |
| 720p con tag | 57,78 | 91,60 |

Con tag uguali il punteggio coincide con quello senza tag, prima o dopo lo
`scale` del riferimento. La misura VMAF e le sonde di `vmaf_analysis` applicano
quindi al riferimento lo stesso `setparams` del distorto
(`VmafRequest::tag_colore`).

### Modello VMAF ad alto frame rate

libvmaf 3.2.1 include le varianti `vmaf_v1.0.16_hfr_5d0h`, `_hfr_3d0h`,
`_hfr_3d0h_2160` e `_hfr_1d5h_2160`, tarate su 50/60 fps. Da 45 fps in su
`modello_vmaf_effettivo` sostituisce il modello v1 configurato con la sua
variante HFR, nella misura come nell'analisi; la soglia separa i 24–30 fps di
cinema e TV dai 50–60 fps delle varianti. Misura su `tos` interpolato a 60 fps
(`minterpolate` in modalità blend), preset 8, `tune=0`, `-g 600`:

| CRF | `5d0h` | `hfr_5d0h` | `3d0h` | `hfr_3d0h` |
| --: | -----: | ---------: | -----: | ---------: |
| 35 | 95,714 | 96,459 | 94,633 | 95,380 |
| 47 | 92,633 | 93,385 | 91,150 | 91,905 |

Lo scarto è costante, circa 0,75 punti. Su questo clip un punto di CRF vale circa
0,26 punti VMAF e il 6% di file: 0,75 punti corrispondono a circa 3 punti di CRF,
quasi il 20% di file. In modalità `vmaf_analysis` il modello standard avrebbe
quindi scelto CRF più bassi del necessario.

## Grana sintetica per job

`film-grain=8` fa stimare all'encoder la grana del sorgente e la fa sintetizzare
al decoder in riproduzione; SVT-AV1 4.2 usa per default `film-grain-denoise=0` e
`adaptive-film-grain=1`. A CRF 47:

| Clip | Bitrate | CAMBI medio (max) | SSIMULACRA2 | VMAF v1 |
| ---- | ------- | ----------------- | ----------- | ------- |
| `tos` | 289 → 308 kbit/s (+6,5%) | 0,27 (0,38) → 0,00 (0,00) | 60,4 → 60,1 | 94,9 → 95,0 |
| `snow` | 38 → 53 kbit/s (+40,8%) | 1,60 (6,86) → 0,04 (0,28) | 81,0 → 80,8 | 93,7 → 94,4 |
| `dark` | 193 → 213 kbit/s (+10,5%) | 1,38 (2,62) → 0,00 (0,29) | 66,4 → 66,2 | 93,0 → 93,1 |

Il banding sparisce su tutte e tre le clip. SSIMULACRA2 e VMAF restano quasi
fermi, mentre XPSNR cala di 1,2 dB: confronta i pixel, e la grana sintetica non
coincide con quella del sorgente. Come default costa troppo: in media il 19% di
file, e su una sorgente con poco banding come `tos` il 6,5% in più non migliora
né SSIMULACRA2 né VMAF. Conviene invece per job, su sorgenti con cieli,
gradienti, scene scure o animazione:

```json
{ "name": "Animazione", "params": { "svtav1_params": "tune=5:film-grain=8" } }
```

Con gli script: `SVTAV1_PARAMS=tune=5:film-grain=8 ./execute_docker.sh`.

In modalità `target_size` l'opzione non ha effetto: `svtav1_params` viene
sostituito per intero da `tune=0`, l'unico valore compatibile con il VBR a due
passaggi.

## Limiti delle misure

- Tre clip da 10 s, tutte a 24 fps e ridotte a 480 righe: mancano output 1080p,
  sorgenti HDR, animazione 2D, sport e riprese molto rumorose.
- Il confronto HFR usa un clip interpolato, non una ripresa nativa a 60 fps.
- La prova sui tag colore usa un sorgente sintetico.
- Le metriche non sostituiscono la visione: grana e banding vanno giudicati a
  occhio sullo schermo di destinazione.

## Come ripetere le misure

Distorto e riferimento devono avere la stessa risoluzione.

```bash
# VMAF-NEG, VMAF v1 e CAMBI: prima il distorto, poi il riferimento
ffmpeg -i out.mkv -i ref.y4m -lavfi \
  "[0:v][1:v]libvmaf=model='version=vmaf_v0.6.1neg\:name=vmafneg|version=vmaf_v1.0.16_5d0h\:name=v1':feature='name=cambi':ts_sync_mode=nearest:log_fmt=json:log_path=vmaf.json" \
  -f null -

# XPSNR per piano, da pesare 6:1:1: prima il riferimento, poi il distorto
ffmpeg -v error -i out.mkv -pix_fmt yuv420p10le -strict -1 -f yuv4mpegpipe - |
  ffmpeg -i ref.y4m -f yuv4mpegpipe -i - -lavfi "[0:v][1:v]xpsnr" -f null -
```

Il distorto arriva a `xpsnr` da una pipe `.y4m`: con i frame di libdav1d come
secondo input il filtro di FFmpeg 9 restituisce valori senza senso (3 dB invece
di 35).

SSIMULACRA2 è calcolato frame per frame con la crate Rust `ssimulacra2` 0.5 sui
due `.y4m` a 10 bit, riferimento per primo.
