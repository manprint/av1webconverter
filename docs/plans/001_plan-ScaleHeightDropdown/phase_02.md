# Phase 2 — Frontend: menu a tendina per l'altezza massima

> **Intent:** il campo "Altezza massima (px)" diventa un `select` con nove scelte
> fisse, in entrambi i punti in cui compare (form di creazione job e editor dei
> parametri di un job esistente), inviando al backend un numero e non una stringa.
> **Shippable alone?** yes — ma richiede la fase 1: senza di essa il backend
> rifiuterebbe `scale_height = 0` con `400`.
> **Preconditions:** phase_01 DONE.

## State contract (mandatory)

1. Prima di toccare qualunque cosa: leggi [STATE.md](STATE.md) e verifica che
   punti a una sub-fase di questa fase. Esegui i comandi di gate elencati in
   `STATE.md` **§3** e confronta il risultato con quanto dichiarano §1 e §7. Il
   repo ha ragione: se divergono, correggi il file.
2. Dopo **ogni** sub-fase qui sotto: aggiorna `STATE.md` (posizione, ledger, file
   toccati, lavoro a metà, verifica, deviazioni, `Next action:`, timestamp) e
   allinea `resume.md`. Una sub-fase non è finita prima che questo sia scritto.
3. Se la sessione finisce a metà sub-fase, scrivi in `STATE.md` §6 esattamente
   cosa è rimasto a metà, prima di fermarti.

**Nota sul frontend:** il repo non ha harness di test JavaScript. Le sub-fasi 2.1
e 2.3 chiudono con una verifica manuale esplicita, il cui esito va registrato in
`STATE.md` §7 come riga dedicata. Non dichiarare verde ciò che non hai aperto nel
browser. Il frontend è embedded via `include_str!`, quindi ogni verifica manuale
richiede un `cargo run` nuovo, non un semplice refresh.

---

## Sub-phases

### 2.1 Descrittore `select` numerico e coercizione in `readParams`

> **Cambio di comportamento.** `readParams` acquisisce un ramo nuovo: i campi
> `select` marcati `numeric: true` vengono convertiti con `Number()`. Senza
> questo, `scale_height` partirebbe come stringa `"480"` e l'API risponderebbe
> `400` (serde rifiuta una stringa per un `u32`). L'etichetta del campo perde il
> suffisso `(px)`, perché le unità sono ora dentro le singole voci e la voce
> "Dimensione originale" non ha unità.

- **Model:** `agent:gpt5.6-luna`
- **Assignment:** `agent:gpt5.6-luna` — implementazione; **self-review obbligatoria**
  sulla coercizione numerica: è il punto in cui una regressione si manifesta come
  `400` alla creazione del job.
- **Files:** `src/web/app.js:27` (voce di `PARAM_FIELDS`), `src/web/app.js:117-141`
  (`readParams`)
- **Change:**
  1. In `src/web/app.js`, sostituisci la riga 27, oggi:
     ```js
     { key: "scale_height", label: "Altezza massima (px)", type: "number", min: 16, max: 4320 },
     ```
     con il descrittore completo, mantenendo la posizione nell'array (subito dopo
     `preset` e prima di `audio_bitrate`, così l'ordine dei campi nella UI non
     cambia):
     ```js
     {
       key: "scale_height",
       label: "Altezza massima",
       type: "select",
       numeric: true,
       options: [
         ["0", "Dimensione originale"],
         ["144", "144px"],
         ["240", "240px"],
         ["320", "320px"],
         ["480", "480px"],
         ["720", "720px"],
         ["1080", "1080px"],
         ["1440", "1440px"],
         ["2160", "2160px"],
       ],
       dynamicOptionLabel: (value) => `${value}px`,
     },
     ```
     I valori delle `options` sono stringhe perché finiscono nell'attributo
     `value` dell'`option`; la conversione a numero avviene in `readParams`.
  2. In `readParams` (`src/web/app.js:117`), estendi il ramo numerico. Oggi le
     righe 122-129 sono:
     ```js
     if (field.type === "checkbox") {
       params[field.key] = input.checked;
     } else if (field.type === "number") {
       params[field.key] = input.value === "" ? null : Number(input.value);
     } else {
       params[field.key] = input.value;
     }
     ```
     Diventano:
     ```js
     if (field.type === "checkbox") {
       params[field.key] = input.checked;
     } else if (field.type === "number" || field.numeric) {
       // I select numerici devono partire come numero: serde rifiuta "480" per un u32.
       params[field.key] = input.value === "" ? null : Number(input.value);
     } else {
       params[field.key] = input.value;
     }
     ```
  3. **Non** toccare il ciclo di fallback alle righe 133-139. Quel ciclo
     rimpiazza con il default del server ogni campo rimasto `null` o `""`: la
     condizione è `params[field.key] === null || params[field.key] === ""`, e `0`
     non soddisfa nessuna delle due, quindi la scelta "Dimensione originale"
     sopravvive e non viene silenziosamente riportata a 480. Rileggi la condizione
     e verifica che sia ancora esattamente questa prima di proseguire.
- **Unit tests:** nessun harness JavaScript nel repo — non sono possibili. La
  copertura automatica equivalente è lato backend: **T-SH5** in `phase_03.md § 3.1`
  asserisce che un `scale_height` inviato come stringa produce `400`, così una
  regressione della coercizione diventa visibile in un test.
- **e2e tests:** none in questa sub-fase.
- **Verifica manuale (obbligatoria, da registrare in `STATE.md` §7):**
  `cargo run`, apri l'interfaccia, crea un job lasciando i valori di default e
  controlla in `data/jobs/<id>.json` che `"scale_height"` sia il **numero** `480`
  e non la stringa `"480"`.
- **Done:** gates verdi (`cargo fmt --all -- --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test --lib`) + verifica
  manuale eseguita e annotata + `STATE.md` e `resume.md` aggiornati.

---

### 2.2 `select` statica nella form di creazione job

- **Model:** `agent:gpt5.6-luna`
- **Assignment:** `agent:gpt5.6-luna` — implementazione e self-review.
- **Files:** `src/web/index.html:73-76`
- **Change:** sostituisci il blocco del campo, oggi:
  ```html
  <div class="field">
    <label for="param-scale-height">Altezza massima (px)</label>
    <input id="param-scale-height" name="scale_height" type="number" min="16" max="4320" />
  </div>
  ```
  con:
  ```html
  <div class="field">
    <label for="param-scale-height">Altezza massima</label>
    <select id="param-scale-height" name="scale_height">
      <option value="0">Dimensione originale</option>
      <option value="144">144px</option>
      <option value="240">240px</option>
      <option value="320">320px</option>
      <option value="480">480px</option>
      <option value="720">720px</option>
      <option value="1080">1080px</option>
      <option value="1440">1440px</option>
      <option value="2160">2160px</option>
    </select>
  </div>
  ```
  Le voci e le etichette devono coincidere **esattamente** con le `options` del
  descrittore di 2.1: sono due sorgenti della stessa lista e una divergenza
  romperebbe la selezione del valore corrente. Il `div.field` e la classe restano
  invariati; `style.css` non richiede modifiche, perché il selettore `select`
  esiste già per il campo `Modalità` (`src/web/index.html:50-55`).
  L'editor per-job non richiede modifiche: `buildParamsEditor`
  (`src/web/app.js:207-211`) costruisce già la `select` a partire da
  `field.options`.
- **Unit tests:** none (markup).
- **e2e tests:** none.
- **Verifica manuale (da registrare in `STATE.md` §7):** con `cargo run`, la form
  di creazione job mostra nove voci nell'ordine dichiarato, con `480px`
  preselezionata (default del server, D6).
- **Done:** gates verdi + il campo appare come tendina con la voce corretta
  preselezionata + `STATE.md` e `resume.md` aggiornati.

---

### 2.3 Valori fuori lista conservati, e riepilogo del job leggibile

> **Cambio di comportamento.** Aprire l'editor dei parametri di un job che ha
> un'altezza fuori dalla lista (per esempio 500, arrivata da `AV1WC_SCALE_HEIGHT`
> o dall'API) non deve alterarla: senza intervento il browser selezionerebbe la
> prima voce e un salvataggio dei parametri porterebbe silenziosamente il job a
> "Dimensione originale". Invariante I-4.

- **Model:** `agent:gpt5.6-luna`
- **Assignment:** `agent:gpt5.6-luna` — implementazione; **self-review obbligatoria**:
  è il punto in cui si perderebbero dati di un job esistente.
- **Files:** `src/web/app.js:145-157` (`writeParams`), nuovo helper accanto ad
  esso, `src/web/app.js:277` (riepilogo in `renderJob`)
- **Change:**
  1. Aggiungi un helper immediatamente **prima** di `writeParams`
     (`src/web/app.js:145`):
     ```js
     /** Conserva un valore fuori dalle scelte fisse aggiungendo un'opzione dedicata. */
     function ensureOption(select, value, label) {
       if (value === null || value === undefined || select.tagName !== "SELECT") return;
       const wanted = String(value);
       if ([...select.options].some((option) => option.value === wanted)) return;
       const option = document.createElement("option");
       option.value = wanted;
       option.textContent = label(value);
       // Inserimento in ordine numerico, così la tendina resta leggibile.
       const next = [...select.options].find((existing) => Number(existing.value) > Number(wanted));
       select.insertBefore(option, next ?? null);
     }
     ```
  2. In `writeParams` (`src/web/app.js:145-157`), chiama l'helper prima di
     assegnare il valore. Il ramo `else` oggi è:
     ```js
     } else {
       input.value = value === null || value === undefined ? "" : value;
     }
     ```
     Diventa:
     ```js
     } else {
       if (field.dynamicOptionLabel) ensureOption(input, value, field.dynamicOptionLabel);
       input.value = value === null || value === undefined ? "" : value;
     }
     ```
     `writeParams` è l'unico punto di scrittura dei parametri, usato sia da
     `resetNewJobForm` (`src/web/app.js:706`) sia da `buildParamsEditor`
     (`src/web/app.js:228`): una sola modifica copre entrambi i percorsi.
  3. In `renderJob` (`src/web/app.js:277`), sostituisci la riga:
     ```js
     `altezza ${job.params.scale_height}px`,
     ```
     con:
     ```js
     job.params.scale_height === 0
       ? "dimensione originale"
       : `altezza ${job.params.scale_height}px`,
     ```
     La riga fa parte dell'array passato a `.filter(Boolean).join(" · ")`: la
     stringa `"dimensione originale"` è truthy, quindi non viene filtrata.
- **Unit tests:** nessun harness JavaScript nel repo — non sono possibili.
- **e2e tests:** none.
- **Verifica manuale (obbligatoria, da registrare in `STATE.md` §7):**
  1. Avvia con `AV1WC_SCALE_HEIGHT=500 cargo run`, crea un job, apri l'editor dei
     parametri: la tendina mostra una voce `500px` selezionata, collocata fra
     `480px` e `720px`. Salva senza altre modifiche e verifica in
     `data/jobs/<id>.json` che `"scale_height"` sia ancora `500`.
  2. Crea un job con `Dimensione originale`: il riepilogo del job mostra
     `dimensione originale` al posto di `altezza 0px`.
- **Done:** gates verdi + le due verifiche manuali eseguite e annotate +
  `STATE.md` e `resume.md` aggiornati.

---

### 2.4 Update README.md

Sub-fase di chiusura obbligatoria. Solo guida utente, nessun dettaglio
implementativo.

- **Model:** `agent:gpt5.6-luna`
- **Assignment:** `agent:gpt5.6-luna` — documentazione e self-review.
- **Files:** `README.md` (radice del repo)
- **Change:** aggiorna queste sezioni per ciò che **questa fase rende
  effettivamente usabile** (la scelta dell'altezza dalla tendina):
  - **`## Funzionalità`** (`README.md:15`): se l'elenco cita i parametri
    modificabili dall'interfaccia, precisa che l'altezza si sceglie fra valori
    predefiniti, `Dimensione originale` inclusa. Se non li cita, non aggiungere
    nulla qui.
  - **`## Parametri di conversione`** (`README.md:141`): aggiungi una frase che
    elenchi le scelte disponibili nell'interfaccia — `Dimensione originale`, 144,
    240, 320, 480, 720, 1080, 1440, 2160 px — chiarendo che il default è 480px e
    che l'API accetta anche altezze diverse da queste, fra 16 e 4320.
  - **Limiti noti**: se un job è stato creato con un'altezza fuori dall'elenco,
    l'interfaccia la mostra come voce aggiuntiva e la conserva.
  Escludi: nomi di file o funzioni, `PARAM_FIELDS`, `readParams`, riferimenti a
  fasi o a questo piano. Preserva struttura, tono e lingua (italiano): modifica,
  non riscrivere. Nessuna emoji.
- **Unit tests:** none (documentazione).
- **e2e tests:** none — le schermate descritte sono state aperte con `cargo run`
  durante 2.2 e 2.3.
- **Done:** un utente nuovo capisce dal solo README quali altezze può scegliere e
  cosa comporta `Dimensione originale`; nessun dettaglio implementativo presente;
  gates verdi; `STATE.md` e `resume.md` aggiornati.

---

## Phase gates

- **Fmt:** `cargo fmt --all -- --check`
- **Lint:** `cargo clippy --all-targets -- -D warnings`
- **Test subset:** `cargo test --lib`
- **Regression guard:** tutti i test di fase 1 restano verdi; la form di
  creazione job resta funzionante per tutti gli altri parametri (`Modalità`,
  `CRF`, `preset`, `suffix`), verificato creando un job in modalità
  `Dimensione target` durante la verifica manuale
- **README:** aggiornato per il comportamento utente introdotto da questa fase,
  privo di dettagli implementativi

## Phase done criterion

Nella pagina principale il campo dell'altezza è una tendina con esattamente nove
voci nell'ordine `Dimensione originale`, 144px, 240px, 320px, 480px, 720px,
1080px, 1440px, 2160px, con 480px preselezionata; la creazione di un job invia
`scale_height` come numero; un job con altezza fuori elenco conserva il proprio
valore all'apertura dell'editor; il riepilogo mostra `dimensione originale` quando
l'altezza è 0. Le verifiche manuali sono annotate in `STATE.md` §7. `README.md`
riflette il comportamento rilasciato da questa fase.
