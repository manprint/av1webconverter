"use strict";

// Stato locale: i job arrivano dal backend e vengono aggiornati via SSE.
const store = {
  jobs: new Map(),
  defaults: null,
  expanded: new Set(),
  editing: new Set(),
  selection: new Map(), // job_id -> Set(file_id)
  logTarget: null, // { jobId, fileId }
};

const PARAM_FIELDS = [
  {
    key: "mode",
    label: "Modalità",
    type: "select",
    options: [
      ["crf", "Qualità costante (CRF)"],
      ["target_size", "Dimensione target (VBR 2 passaggi)"],
      ["vmaf_analysis", "Analisi VMAF + Qualità Costante"],
    ],
  },
  {
    key: "crf",
    label: "CRF",
    type: "number",
    min: 0,
    max: 63,
    modes: ["crf", "vmaf_analysis"],
  },
  {
    key: "vmaf_segments",
    label: "VMAF Segments",
    type: "number",
    min: 1,
    max: 10,
    modes: ["vmaf_analysis"],
  },
  {
    key: "crf_test_steps",
    label: "CRF Test Steps",
    type: "number",
    min: 1,
    max: 10,
    modes: ["vmaf_analysis"],
  },
  {
    key: "vmaf_target",
    label: "VMAF Target",
    type: "number",
    min: 0,
    max: 100,
    step: 0.5,
    modes: ["vmaf_analysis"],
  },
  {
    key: "vmaf_segment_duration_seconds",
    label: "Durata segmento VMAF (s)",
    type: "number",
    min: 1,
    max: 300,
    step: 0.5,
    modes: ["vmaf_analysis"],
  },
  { key: "target_size_mb", label: "Dimensione target (MB)", type: "number", min: 1, step: 1, mode: "target_size" },
  { key: "size_margin_percent", label: "Margine overhead (%)", type: "number", min: 0, max: 50, step: 0.1, mode: "target_size" },
  { key: "preset", label: "Preset", type: "number", min: 0, max: 13 },
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
  {
    key: "fps_target",
    label: "Interpolazione fps",
    type: "select",
    numeric: true,
    options: [
      ["0", "Invariato"],
      ["50", "50 fps"],
      ["60", "60 fps"],
    ],
  },
  { key: "audio_bitrate", label: "Bitrate audio", type: "text" },
  {
    key: "svtav1_params",
    label: "svtav1-params",
    type: "text",
    modes: ["crf", "vmaf_analysis"],
  },
  { key: "pixel_format", label: "Formato pixel", type: "text" },
  { key: "keyint", label: "Keyframe ogni (frame)", type: "number", min: 1, max: 3000 },
  { key: "cpus", label: "Thread ffmpeg", type: "number", min: 1, max: 256, placeholder: "automatico" },
  { key: "progress_interval", label: "Aggiornamento progresso (s)", type: "number", min: 0.5, step: 0.5 },
  { key: "measure_vmaf", label: "Misura VMAF dopo la conversione", type: "checkbox" },
  { key: "vmaf_model", label: "Modello VMAF", type: "text" },
  { key: "overwrite", label: "Sovrascrivi gli output già presenti", type: "checkbox" },
];

const JOB_STATUS_LABEL = {
  pending: "in attesa",
  running: "in corso",
  completed: "completato",
  failed: "con errori",
  stopped: "interrotto",
};

const FILE_STATUS_LABEL = {
  pending: "in attesa",
  running: "in conversione",
  completed: "convertito",
  skipped: "saltato",
  failed: "errore",
  cancelled: "annullato",
};

// -- utilità ---------------------------------------------------------------

function element(id) {
  return document.getElementById(id);
}

function formatSize(bytes) {
  if (bytes === null || bytes === undefined) return "—";
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(2)} GB`;
  if (bytes >= 1e6) return `${(bytes / 1e6).toFixed(2)} MB`;
  if (bytes >= 1e3) return `${(bytes / 1e3).toFixed(1)} kB`;
  return `${bytes} B`;
}

function formatClock(seconds) {
  if (seconds === null || seconds === undefined || !Number.isFinite(seconds) || seconds < 0) {
    return "--:--:--";
  }
  const total = Math.floor(seconds);
  const pad = (value) => String(value).padStart(2, "0");
  return `${pad(Math.floor(total / 3600))}:${pad(Math.floor(total / 60) % 60)}:${pad(total % 60)}`;
}

function formatDateTime(value) {
  if (!value) return "—";
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? "—" : date.toLocaleString("it-IT");
}

function toast(message, kind = "info") {
  const node = document.createElement("div");
  node.className = `toast ${kind}`;
  node.textContent = message;
  element("toasts").appendChild(node);
  setTimeout(() => node.remove(), kind === "error" ? 8000 : 4000);
}

async function api(path, options = {}) {
  const response = await fetch(path, {
    headers: options.body instanceof FormData ? {} : { "Content-Type": "application/json" },
    ...options,
  });
  if (!response.ok) {
    let message = `errore HTTP ${response.status}`;
    try {
      const body = await response.json();
      if (body && body.error) message = body.error;
    } catch (_) {
      /* corpo non JSON: si tiene il messaggio generico */
    }
    throw new Error(message);
  }
  if (response.status === 204) return null;
  const text = await response.text();
  return text ? JSON.parse(text) : null;
}

// -- parametri -------------------------------------------------------------

function paramsFromDefaults() {
  return store.defaults ? { ...store.defaults.params } : {};
}

/** Legge i campi di un contenitore e produce l'oggetto dei parametri. */
function readParams(root) {
  const params = {};
  for (const field of PARAM_FIELDS) {
    const input = root.querySelector(`[name="${field.key}"]`);
    if (!input) continue;
    if (field.type === "checkbox") {
      params[field.key] = input.checked;
    } else if (field.type === "number" || field.numeric) {
      // I select numerici devono partire come numero: serde rifiuta "480" per un u32.
      params[field.key] = input.value === "" ? null : Number(input.value);
    } else {
      params[field.key] = input.value;
    }
  }
  const suffix = root.querySelector('[name="suffix"]');
  if (suffix) params.suffix = suffix.value;
  // I campi opzionali vuoti restano nulli; gli altri tornano al default.
  const fallback = paramsFromDefaults();
  for (const field of PARAM_FIELDS) {
    if (field.key === "cpus" || field.key === "target_size_mb") continue;
    if (params[field.key] === null || params[field.key] === "") {
      params[field.key] = fallback[field.key];
    }
  }
  if (params.mode !== "target_size") params.target_size_mb = null;
  return params;
}

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

function writeParams(root, params) {
  for (const field of PARAM_FIELDS) {
    const input = root.querySelector(`[name="${field.key}"]`);
    if (!input) continue;
    const value = params[field.key];
    if (field.type === "checkbox") {
      input.checked = Boolean(value);
    } else {
      if (field.dynamicOptionLabel) ensureOption(input, value, field.dynamicOptionLabel);
      input.value = value === null || value === undefined ? "" : value;
    }
  }
  const suffix = root.querySelector('[name="suffix"]');
  if (suffix) suffix.value = params.suffix ?? "";
  applyModeVisibility(root);
}

/** Mostra solo i campi pertinenti alla modalità selezionata. */
function applyModeVisibility(root) {
  const mode = root.querySelector('[name="mode"]')?.value ?? "crf";
  root.querySelectorAll("[data-mode]").forEach((node) => {
    const modes = node.dataset.mode.split(/\s+/).filter(Boolean);
    node.classList.toggle("hidden", !modes.includes(mode));
  });
}

/** Costruisce l'editor dei parametri di un job già esistente. */
function buildParamsEditor(job) {
  const wrapper = document.createElement("div");
  wrapper.innerHTML = `
    <div class="grid"></div>
    <div class="actions" style="margin-top:0.9rem">
      <button type="button" class="button primary" data-action="save-params">Salva parametri</button>
      <button type="button" class="button" data-action="cancel-params">Annulla</button>
    </div>`;
  const grid = wrapper.querySelector(".grid");

  const concurrency = document.createElement("div");
  concurrency.className = "field";
  concurrency.innerHTML = `
    <label>Conversioni simultanee</label>
    <input type="number" name="concurrency" min="1" max="64" value="${job.concurrency}" />
    <small>1 = i file vengono convertiti in sequenza.</small>`;
  grid.appendChild(concurrency);

  const suffix = document.createElement("div");
  suffix.className = "field";
  suffix.innerHTML = `
    <label>Suffix</label>
    <input type="text" name="suffix" maxlength="128" pattern="[A-Za-z0-9._-]*" />
    <small>Aggiunto al nome del file prima di <code>.mp4</code>.</small>`;
  grid.appendChild(suffix);

  for (const field of PARAM_FIELDS) {
    const node = document.createElement("div");
    node.className = [
      "field",
      field.type === "checkbox" ? "checkbox" : "",
      field.key === "mode" ? "mode-field" : "",
    ]
      .filter(Boolean)
      .join(" ");
    const modes = field.modes ?? (field.mode ? [field.mode] : []);
    if (modes.length) node.dataset.mode = modes.join(" ");

    if (field.type === "select") {
      const options = field.options
        .map(([value, label]) => `<option value="${value}">${label}</option>`)
        .join("");
      node.innerHTML = `<label>${field.label}</label><select name="${field.key}">${options}</select>`;
    } else if (field.type === "checkbox") {
      node.innerHTML = `<label><input type="checkbox" name="${field.key}" /> ${field.label}</label>`;
    } else {
      const attributes = [
        field.min !== undefined ? `min="${field.min}"` : "",
        field.max !== undefined ? `max="${field.max}"` : "",
        field.step !== undefined ? `step="${field.step}"` : "",
        field.placeholder ? `placeholder="${field.placeholder}"` : "",
      ]
        .filter(Boolean)
        .join(" ");
      node.innerHTML = `<label>${field.label}</label><input type="${field.type}" name="${field.key}" ${attributes} />`;
    }
    grid.appendChild(node);
  }

  writeParams(wrapper, job.params);
  wrapper.querySelector('[name="mode"]').addEventListener("change", () => applyModeVisibility(wrapper));
  return wrapper;
}

// -- rendering -------------------------------------------------------------

function selectionFor(jobId) {
  if (!store.selection.has(jobId)) store.selection.set(jobId, new Set());
  return store.selection.get(jobId);
}

function render() {
  const container = element("jobs");
  const jobs = [...store.jobs.values()].sort(
    (left, right) => new Date(right.created_at) - new Date(left.created_at),
  );
  element("empty-state").classList.toggle("hidden", jobs.length > 0);

  container.replaceChildren();
  for (const job of jobs) {
    container.appendChild(renderJob(job));
  }
}

function renderJob(job) {
  const node = element("job-template").content.firstElementChild.cloneNode(true);
  node.dataset.jobId = job.id;

  node.querySelector('[data-field="name"]').textContent = job.name;
  const status = node.querySelector('[data-field="status"]');
  status.textContent = JOB_STATUS_LABEL[job.status] ?? job.status;
  status.className = `badge badge-${job.status}`;

  const percent = job.overall_progress ?? 0;
  node.querySelector('[data-field="overall-bar"]').style.width = `${percent}%`;
  node.querySelector('[data-field="overall-value"]').textContent = `${percent.toFixed(1)}%`;

  const counts = job.counts ?? {};
  node.querySelector('[data-field="meta"]').textContent = [
    `${counts.total ?? 0} file`,
    `${counts.completed ?? 0} convertiti`,
    counts.skipped ? `${counts.skipped} saltati` : null,
    counts.failed ? `${counts.failed} falliti` : null,
    counts.cancelled ? `${counts.cancelled} annullati` : null,
    `simultanee: ${job.concurrency}`,
    job.params.mode === "target_size"
      ? `target ${job.params.target_size_mb ?? "?"} MB · preset ${job.params.preset}`
      : `CRF ${job.params.crf} · preset ${job.params.preset}`,
    job.params.scale_height === 0
      ? "dimensione originale"
      : `altezza ${job.params.scale_height}px`,
    job.params.fps_target ? `interpolazione ${job.params.fps_target} fps` : null,
    `creato ${formatDateTime(job.created_at)}`,
  ]
    .filter(Boolean)
    .join(" · ");

  const error = node.querySelector('[data-field="error"]');
  if (job.error) {
    error.textContent = job.error;
    error.classList.remove("hidden");
  }

  const running = job.status === "running";
  const hasPending = (counts.pending ?? 0) > 0;
  const downloadable = (counts.completed ?? 0) + (counts.skipped ?? 0) > 0;
  node.querySelector('[data-action="start"]').disabled = running || !hasPending;
  node.querySelector('[data-action="stop"]').disabled = !running;
  node.querySelector('[data-action="download-all"]').disabled = !downloadable;

  const body = node.querySelector('[data-field="body"]');
  body.classList.toggle("hidden", !store.expanded.has(job.id));

  const editor = node.querySelector('[data-field="params-editor"]');
  if (store.editing.has(job.id)) {
    editor.replaceChildren(buildParamsEditor(job));
    editor.classList.remove("hidden");
  }

  node.querySelector('[data-action="download-selection"]').disabled = selectionFor(job.id).size === 0;
  node.querySelector('[data-field="files"]').replaceChildren(
    ...job.files.map((file) => renderFileRow(job, file)),
  );
  return node;
}

function renderFileRow(job, file) {
  const row = document.createElement("tr");
  row.dataset.fileId = file.id;

  const selectable = file.status === "completed" || file.status === "skipped";
  const checked = selectionFor(job.id).has(file.id);
  const progress = file.progress ?? {};
  const passLabel =
    progress.total_passes > 1 ? ` · passaggio ${progress.pass}/${progress.total_passes}` : "";

  row.innerHTML = `
    <td class="col-select">
      <input type="checkbox" data-action="select-file" ${selectable ? "" : "disabled"} ${checked ? "checked" : ""} />
    </td>
    <td>
      <span class="file-name" title="${escapeHtml(file.original_name)}">${escapeHtml(file.original_name)}</span>
      ${file.error ? `<span class="file-detail" style="color:var(--error)">${escapeHtml(file.error)}</span>` : ""}
    </td>
    <td><span class="badge badge-${file.status}">${FILE_STATUS_LABEL[file.status] ?? file.status}</span></td>
    <td class="col-progress">
      <div class="progress"><div class="progress-bar" style="width:${(progress.percent ?? 0).toFixed(1)}%"></div></div>
      <span class="file-detail" data-field="file-progress">${fileProgressText(file, passLabel)}</span>
    </td>
    <td>
      <span class="file-detail">in ${formatSize(file.input_size_bytes)}</span>
      <span class="file-detail">out ${formatSize(file.output_size_bytes)}</span>
       ${file.vmaf ? `<span class="file-detail">${vmafText(file.vmaf)}</span>` : ""}
       ${file.vmaf_analysis ? `<span class="file-detail" title="${escapeHtml(analisiTooltip(file.vmaf_analysis))}">${analisiText(file.vmaf_analysis)}</span>` : ""}
    </td>
    <td class="col-actions">
      <div class="row-actions">
        <button class="button" data-action="show-log">Log</button>
        <button class="button" data-action="download-file" ${selectable ? "" : "disabled"}>Scarica</button>
        <button class="button danger" data-action="remove-file" ${file.status === "running" ? "disabled" : ""}>Rimuovi</button>
      </div>
    </td>`;
  return row;
}

function fileProgressText(file, passLabel) {
  const progress = file.progress ?? {};
  if (file.status === "running") {
    return `${(progress.percent ?? 0).toFixed(1)}% · ${formatClock(progress.elapsed_seconds)} / ${formatClock(
      progress.total_seconds,
    )} · ${progress.speed ?? "N/D"} · ETA ${formatClock(progress.eta_seconds)}${passLabel}`;
  }
  if (file.status === "completed" || file.status === "skipped") {
    return `terminato ${formatDateTime(file.finished_at)}`;
  }
  return FILE_STATUS_LABEL[file.status] ?? file.status;
}

/** Punteggio VMAF del file, o il motivo per cui manca. */
function vmafText(vmaf) {
  if (vmaf.error) return `VMAF non disponibile: ${escapeHtml(vmaf.error)}`;
  if (typeof vmaf.score !== "number") return "VMAF non disponibile";
  return `VMAF ${vmaf.score.toFixed(2)} (${escapeHtml(vmaf.model)})`;
}

/** Esito dell'analisi: CRF scelto e motivo della scelta. */
function analisiText(analisi) {
  const nota = {
    sopra_soglia: "",
    tutti_sopra_soglia: " (tutti gli step sopra il target)",
    nessuno_sopra_soglia: " (nessuno step sopra il target: usato il CRF impostato)",
    analisi_non_eseguibile: " (analisi non eseguibile)",
  }[analisi.motivo] ?? "";
  return `analisi VMAF: CRF ${escapeHtml(analisi.crf_scelto)}, target ${escapeHtml(analisi.target)}${nota}`;
}

/** Dettaglio per CRF mostrato nel tooltip della riga del file. */
function analisiTooltip(analisi) {
  const risultati = Array.isArray(analisi.risultati) ? analisi.risultati : [];
  const righe = risultati.map((risultato) => {
    const vmaf = typeof risultato.vmaf_medio === "number" ? risultato.vmaf_medio.toFixed(2) : "N/D";
    return `CRF ${risultato.crf}: VMAF ${vmaf}`;
  });
  return righe.length ? righe.join("\n") : "nessun risultato VMAF";
}

function escapeHtml(value) {
  return String(value ?? "").replace(/[&<>"']/g, (character) => {
    return { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[character];
  });
}

// -- aggiornamenti mirati --------------------------------------------------

/** Aggiorna solo le barre di progresso: evita di ricostruire il DOM ogni 5s. */
function patchProgress(event) {
  const job = store.jobs.get(event.job_id);
  if (job) {
    job.overall_progress = event.overall_progress;
    const file = job.files.find((item) => item.id === event.file_id);
    if (file) file.progress = event.progress;
  }

  const jobNode = document.querySelector(`.job[data-job-id="${event.job_id}"]`);
  if (!jobNode) return;
  jobNode.querySelector('[data-field="overall-bar"]').style.width = `${event.overall_progress}%`;
  jobNode.querySelector('[data-field="overall-value"]').textContent =
    `${event.overall_progress.toFixed(1)}%`;

  const row = jobNode.querySelector(`tr[data-file-id="${event.file_id}"]`);
  if (!row) return;
  row.querySelector(".col-progress .progress-bar").style.width = `${event.progress.percent}%`;
  const passLabel =
    event.progress.total_passes > 1
      ? ` · passaggio ${event.progress.pass}/${event.progress.total_passes}`
      : "";
  row.querySelector('[data-field="file-progress"]').textContent = fileProgressText(
    { status: "running", progress: event.progress },
    passLabel,
  );
}

// -- log -------------------------------------------------------------------

async function openLog(jobId, fileId, fileName) {
  store.logTarget = { jobId, fileId };
  element("log-title").textContent = `Log di conversione — ${fileName}`;
  const content = element("log-content");
  content.textContent = "caricamento…";
  element("log-overlay").classList.remove("hidden");
  try {
    const body = await api(`/api/jobs/${jobId}/files/${fileId}/log`);
    content.textContent = body.lines.length ? body.lines.join("\n") : "nessuna riga di log";
    content.scrollTop = content.scrollHeight;
  } catch (error) {
    content.textContent = `impossibile leggere il log: ${error.message}`;
  }
}

function closeLog() {
  store.logTarget = null;
  element("log-overlay").classList.add("hidden");
}

function appendLogLine(event) {
  const target = store.logTarget;
  if (!target || target.jobId !== event.job_id || target.fileId !== event.file_id) return;
  const content = element("log-content");
  const atBottom = content.scrollHeight - content.scrollTop - content.clientHeight < 40;
  if (content.textContent === "nessuna riga di log" || content.textContent === "caricamento…") {
    content.textContent = event.line;
  } else {
    content.textContent += `\n${event.line}`;
  }
  if (atBottom) content.scrollTop = content.scrollHeight;
}

// -- azioni ----------------------------------------------------------------

async function uploadFiles(jobId, files, statusNode) {
  if (!files.length) return;
  const form = new FormData();
  for (const file of files) {
    // Con l'upload di cartelle il percorso relativo è l'identità del file.
    form.append("files", file, file.webkitRelativePath || file.name);
  }
  statusNode.textContent = `caricamento di ${files.length} file…`;
  try {
    const result = await api(`/api/jobs/${jobId}/files`, { method: "POST", body: form });
    upsertJob(result.job);
    statusNode.textContent = "";
    const rejected = result.rejected ?? [];
    if (result.uploaded) toast(`${result.uploaded} file caricati.`, "success");
    for (const item of rejected) {
      toast(`${item.name}: ${item.reason}`, "error");
    }
  } catch (error) {
    statusNode.textContent = "";
    toast(`Upload non riuscito: ${error.message}`, "error");
  }
}

function downloadSelection(jobId) {
  const ids = [...selectionFor(jobId)];
  if (!ids.length) return;
  // La selezione va nel corpo JSON, quindi il download passa da fetch e da un
  // URL oggetto invece che da una semplice navigazione.
  fetch(`/api/jobs/${jobId}/download`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ file_ids: ids }),
  })
    .then(async (response) => {
      if (!response.ok) {
        const body = await response.json().catch(() => ({}));
        throw new Error(body.error ?? `errore HTTP ${response.status}`);
      }
      const blob = await response.blob();
      const url = URL.createObjectURL(blob);
      const link = document.createElement("a");
      link.href = url;
      link.download = `${jobId}.zip`;
      link.click();
      URL.revokeObjectURL(url);
    })
    .catch((error) => toast(`Download non riuscito: ${error.message}`, "error"));
}

function upsertJob(view) {
  store.jobs.set(view.id, view);
  render();
}

async function handleJobAction(action, jobId, node, event) {
  const job = store.jobs.get(jobId);
  if (!job) return;

  switch (action) {
    case "toggle-files":
      if (store.expanded.has(jobId)) store.expanded.delete(jobId);
      else store.expanded.add(jobId);
      render();
      break;

    case "edit":
      if (store.editing.has(jobId)) store.editing.delete(jobId);
      else {
        store.editing.add(jobId);
        store.expanded.add(jobId);
      }
      render();
      break;

    case "cancel-params":
      store.editing.delete(jobId);
      render();
      break;

    case "save-params": {
      const editor = node.querySelector('[data-field="params-editor"]');
      const concurrency = Number(editor.querySelector('[name="concurrency"]').value);
      try {
        const updated = await api(`/api/jobs/${jobId}`, {
          method: "PATCH",
          body: JSON.stringify({ params: readParams(editor), concurrency }),
        });
        store.editing.delete(jobId);
        upsertJob(updated);
        toast("Parametri aggiornati.", "success");
      } catch (error) {
        toast(`Salvataggio non riuscito: ${error.message}`, "error");
      }
      break;
    }

    case "start":
      try {
        upsertJob(await api(`/api/jobs/${jobId}/start`, { method: "POST" }));
      } catch (error) {
        toast(`Avvio non riuscito: ${error.message}`, "error");
      }
      break;

    case "stop":
      try {
        upsertJob(await api(`/api/jobs/${jobId}/stop`, { method: "POST" }));
        toast("Job interrotto.");
      } catch (error) {
        toast(`Stop non riuscito: ${error.message}`, "error");
      }
      break;

    case "delete":
      if (!confirm(`Eliminare il job "${job.name}" e tutti i suoi file dal server?`)) return;
      try {
        await api(`/api/jobs/${jobId}`, { method: "DELETE" });
        store.jobs.delete(jobId);
        store.selection.delete(jobId);
        render();
        toast("Job eliminato.", "success");
      } catch (error) {
        toast(`Eliminazione non riuscita: ${error.message}`, "error");
      }
      break;

    case "download-all":
      window.location.href = `/api/jobs/${jobId}/download`;
      break;

    case "download-selection":
      downloadSelection(jobId);
      break;

    case "select-all": {
      const selection = selectionFor(jobId);
      const selectable = job.files.filter(
        (file) => file.status === "completed" || file.status === "skipped",
      );
      if (event.target.checked) selectable.forEach((file) => selection.add(file.id));
      else selection.clear();
      render();
      break;
    }

    default:
      break;
  }
}

async function handleFileAction(action, jobId, fileId, event) {
  const job = store.jobs.get(jobId);
  const file = job?.files.find((item) => item.id === fileId);
  if (!file) return;

  switch (action) {
    case "show-log":
      openLog(jobId, fileId, file.original_name);
      break;

    case "download-file":
      window.location.href = `/api/jobs/${jobId}/files/${fileId}/download`;
      break;

    case "remove-file":
      if (!confirm(`Rimuovere "${file.original_name}" dal job?`)) return;
      try {
        upsertJob(await api(`/api/jobs/${jobId}/files/${fileId}`, { method: "DELETE" }));
      } catch (error) {
        toast(`Rimozione non riuscita: ${error.message}`, "error");
      }
      break;

    case "select-file": {
      const selection = selectionFor(jobId);
      if (event.target.checked) selection.add(fileId);
      else selection.delete(fileId);
      render();
      break;
    }

    default:
      break;
  }
}

// -- eventi SSE ------------------------------------------------------------

function connectEvents() {
  const indicator = element("connection");
  const source = new EventSource("/api/events");

  source.addEventListener("open", () => {
    indicator.textContent = "in ascolto";
    indicator.className = "badge badge-completed";
  });

  source.addEventListener("error", () => {
    indicator.textContent = "riconnessione…";
    indicator.className = "badge badge-stopped";
  });

  source.addEventListener("message", (message) => {
    let event;
    try {
      event = JSON.parse(message.data);
    } catch (_) {
      return;
    }
    switch (event.type) {
      case "job_updated":
        store.jobs.set(event.job.id, event.job);
        render();
        break;
      case "job_deleted":
        store.jobs.delete(event.job_id);
        store.selection.delete(event.job_id);
        render();
        break;
      case "file_progress":
        patchProgress(event);
        break;
      case "file_log":
        appendLogLine(event);
        break;
      default:
        break;
    }
  });
}

// -- avvio -----------------------------------------------------------------

function bindNewJobForm() {
  const panel = element("new-job-panel");
  const form = element("new-job-form");

  element("new-job-button").addEventListener("click", () => {
    panel.classList.toggle("hidden");
    if (!panel.classList.contains("hidden")) element("job-name").focus();
  });
  element("new-job-cancel").addEventListener("click", () => panel.classList.add("hidden"));
  element("param-mode").addEventListener("change", () => applyModeVisibility(form));

  form.addEventListener("submit", async (submit) => {
    submit.preventDefault();
    const concurrency = Number(element("job-concurrency").value) || store.defaults.concurrency;
    try {
      const created = await api("/api/jobs", {
        method: "POST",
        body: JSON.stringify({
          name: element("job-name").value,
          params: readParams(form),
          concurrency,
        }),
      });
      upsertJob(created);
      store.expanded.add(created.id);
      panel.classList.add("hidden");
      form.reset();
      resetNewJobForm();
      render();
      toast("Job creato. Aggiungi i file da convertire.", "success");
    } catch (error) {
      toast(`Creazione non riuscita: ${error.message}`, "error");
    }
  });
}

function resetNewJobForm() {
  const form = element("new-job-form");
  writeParams(form, paramsFromDefaults());
  element("job-concurrency").value = store.defaults.concurrency;
}

function bindDelegatedEvents() {
  const jobs = element("jobs");

  jobs.addEventListener("click", (event) => {
    const button = event.target.closest("[data-action]");
    if (!button || button.tagName === "INPUT") return;
    const jobNode = button.closest(".job");
    if (!jobNode) return;
    const row = button.closest("tr[data-file-id]");
    if (row) handleFileAction(button.dataset.action, jobNode.dataset.jobId, row.dataset.fileId, event);
    else handleJobAction(button.dataset.action, jobNode.dataset.jobId, jobNode, event);
  });

  jobs.addEventListener("change", (event) => {
    const input = event.target.closest("[data-action]");
    if (!input) return;
    const jobNode = input.closest(".job");
    if (!jobNode) return;
    const jobId = jobNode.dataset.jobId;
    const action = input.dataset.action;

    if (action === "upload-files" || action === "upload-folder") {
      const statusNode = jobNode.querySelector('[data-field="upload-status"]');
      uploadFiles(jobId, [...input.files], statusNode);
      input.value = "";
      return;
    }
    const row = input.closest("tr[data-file-id]");
    if (row) handleFileAction(action, jobId, row.dataset.fileId, event);
    else handleJobAction(action, jobId, jobNode, event);
  });
}

async function main() {
  element("log-close").addEventListener("click", closeLog);
  element("log-overlay").addEventListener("click", (event) => {
    if (event.target === element("log-overlay")) closeLog();
  });
  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape") closeLog();
  });

  try {
    store.defaults = await api("/api/defaults");
    resetNewJobForm();
  } catch (error) {
    toast(`Impossibile leggere la configurazione: ${error.message}`, "error");
    store.defaults = { params: {}, concurrency: 1 };
  }

  bindNewJobForm();
  bindDelegatedEvents();

  try {
    const jobs = await api("/api/jobs");
    jobs.forEach((job) => store.jobs.set(job.id, job));
  } catch (error) {
    toast(`Impossibile leggere i job: ${error.message}`, "error");
  }
  render();
  connectEvents();
}

main();
