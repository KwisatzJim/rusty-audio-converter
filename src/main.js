import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";

// ---- State ----------------------------------------------------------------
let ffmpegPath = null;
let ffprobePath = null;
let queue = []; // { id, path, name, status: 'pending'|'converting'|'done'|'error', percent, error }
let outputFolder = null; // null = same as source

// ---- DOM refs ---------------------------------------------------------------
const el = (id) => document.getElementById(id);
const badge = el("ffmpeg-badge");
const warningPanel = el("ffmpeg-warning");
const installOptionsEl = el("install-options");
const mainApp = el("main-app");
const dropzone = el("dropzone");
const browseBtn = el("browse-btn");
const queueSection = el("queue");
const queueList = el("queue-list");
const queueCount = el("queue-count");
const optionsPanel = el("options-panel");
const convertBtn = el("convert-btn");
const formatSelect = el("format-select");
const bitrateLabel = el("bitrate-label");
const bitrateSelect = el("bitrate-select");
const samplerateSelect = el("samplerate-select");
const channelsSelect = el("channels-select");
const chooseFolderBtn = el("choose-folder-btn");
const outputFolderDisplay = el("output-folder-display");
const logPanel = el("log-panel");
const logOutput = el("log-output");
const locateBtn = el("locate-btn");

const LOSSLESS_FORMATS = new Set(["flac", "wav", "alac", "aiff"]);

function log(msg) {
  const time = new Date().toLocaleTimeString();
  logOutput.textContent += `[${time}] ${msg}\n`;
  logOutput.scrollTop = logOutput.scrollHeight;
  logPanel.classList.remove("hidden");
}

// ---- ffmpeg detection -------------------------------------------------------
async function checkFfmpeg() {
  const status = await invoke("check_ffmpeg");
  applyFfmpegStatus(status);
}

function applyFfmpegStatus(status) {
  if (status.ffmpeg_found && status.ffprobe_found) {
    ffmpegPath = status.ffmpeg_path;
    ffprobePath = status.ffprobe_path;
    badge.textContent = status.version || "ffmpeg found";
    badge.className = "badge ok";
    warningPanel.classList.add("hidden");
    mainApp.classList.remove("hidden");
  } else {
    ffmpegPath = null;
    ffprobePath = null;
    badge.textContent = "ffmpeg missing";
    badge.className = "badge missing";
    warningPanel.classList.remove("hidden");
    mainApp.classList.add("hidden");
    loadInstallOptions();
  }
}

async function loadInstallOptions() {
  const info = await invoke("get_install_info");
  installOptionsEl.innerHTML = "";
  if (info.options.length === 0) {
    const p = document.createElement("p");
    p.textContent = `No package manager detected. Download ffmpeg manually from ${info.manual_download_url}`;
    installOptionsEl.appendChild(p);
    return;
  }
  for (const opt of info.options) {
    const card = document.createElement("div");
    card.className = "install-option";
    card.innerHTML = `
      <div class="label">${opt.label}</div>
      <div class="note">${opt.note}</div>
      <code>${opt.command}</code>
      <div class="actions">
        <button class="secondary copy-btn">Copy command</button>
        <button class="secondary recheck-btn">I ran it — recheck</button>
      </div>
    `;
    card.querySelector(".copy-btn").addEventListener("click", () => {
      navigator.clipboard.writeText(opt.command);
      log(`Copied install command for ${opt.label}`);
    });
    card.querySelector(".recheck-btn").addEventListener("click", checkFfmpeg);
    installOptionsEl.appendChild(card);
  }
  const manualNote = document.createElement("p");
  manualNote.innerHTML = `Prefer a manual download? Get a static build from <code>${info.manual_download_url}</code>`;
  installOptionsEl.appendChild(manualNote);
}

locateBtn.addEventListener("click", async () => {
  const selected = await open({
    multiple: false,
    title: "Locate your ffmpeg binary",
  });
  if (!selected) return;
  try {
    const status = await invoke("verify_custom_ffmpeg_path", { path: selected });
    log(`Verified custom ffmpeg at ${selected}`);
    applyFfmpegStatus(status);
  } catch (e) {
    log(`Couldn't verify that file: ${e}`);
    alert(`That doesn't look like a working ffmpeg binary:\n${e}`);
  }
});

// ---- File intake -------------------------------------------------------------
function basename(path) {
  return path.split(/[\\/]/).pop();
}

function addFiles(paths) {
  for (const path of paths) {
    if (queue.some((q) => q.path === path)) continue;
    queue.push({
      id: `${Date.now()}-${Math.random().toString(36).slice(2)}`,
      path,
      name: basename(path),
      status: "pending",
      percent: 0,
      error: null,
    });
  }
  renderQueue();
}

browseBtn.addEventListener("click", async () => {
  const selected = await open({
    multiple: true,
    title: "Choose audio files",
    filters: [
      {
        name: "Audio",
        extensions: ["mp3", "wav", "flac", "aac", "m4a", "ogg", "opus", "wma", "aiff", "aif", "amr"],
      },
    ],
  });
  if (!selected) return;
  const paths = Array.isArray(selected) ? selected : [selected];
  addFiles(paths);
});

// Native drag-drop delivers real filesystem paths (dragDropEnabled: true in config),
// unlike HTML5 drag events which only expose sandboxed File objects without paths.
getCurrentWebview().onDragDropEvent((event) => {
  if (event.payload.type === "over") {
    dropzone.classList.add("drag-over");
  } else if (event.payload.type === "drop") {
    dropzone.classList.remove("drag-over");
    const audioPaths = event.payload.paths.filter((p) =>
      /\.(mp3|wav|flac|aac|m4a|ogg|opus|wma|aiff?|alac|amr)$/i.test(p)
    );
    if (audioPaths.length) addFiles(audioPaths);
  } else {
    dropzone.classList.remove("drag-over");
  }
});

// ---- Queue rendering ---------------------------------------------------------
function renderQueue() {
  queueList.innerHTML = "";
  if (queue.length === 0) {
    queueSection.classList.add("hidden");
    optionsPanel.classList.add("hidden");
    return;
  }
  queueSection.classList.remove("hidden");
  optionsPanel.classList.remove("hidden");
  queueCount.textContent = `${queue.length} file${queue.length === 1 ? "" : "s"}`;

  for (const item of queue) {
    const row = document.createElement("div");
    row.className = "queue-item";
    const statusText = {
      pending: "Ready",
      converting: `${item.percent.toFixed(0)}%`,
      done: "✓ Done",
      error: `✗ ${item.error || "Failed"}`,
    }[item.status];

    row.innerHTML = `
      <div class="queue-item-row">
        <span class="queue-item-name" title="${item.path}">${item.name}</span>
        <span class="queue-item-status ${item.status === "done" ? "done" : ""} ${item.status === "error" ? "error" : ""}">${statusText}</span>
        <button class="remove-item-btn" data-id="${item.id}">✕</button>
      </div>
      ${item.status === "converting" ? `<div class="progress-track"><div class="progress-fill" style="width:${item.percent}%"></div></div>` : ""}
    `;
    row.querySelector(".remove-item-btn").addEventListener("click", () => {
      queue = queue.filter((q) => q.id !== item.id);
      renderQueue();
    });
    queueList.appendChild(row);
  }

  convertBtn.disabled = queue.length === 0 || queue.every((q) => q.status === "done");
}

// ---- Format / bitrate UI -------------------------------------------------------
formatSelect.addEventListener("change", () => {
  const isLossless = LOSSLESS_FORMATS.has(formatSelect.value);
  bitrateLabel.style.display = isLossless ? "none" : "flex";
});

chooseFolderBtn.addEventListener("click", async () => {
  const selected = await open({ directory: true, title: "Choose output folder" });
  if (selected) {
    outputFolder = selected;
    outputFolderDisplay.textContent = selected;
  }
});

// ---- Conversion ---------------------------------------------------------------
function outputPathFor(item, format) {
  const dir = outputFolder || item.path.substring(0, item.path.lastIndexOf(item.path.includes("\\") ? "\\" : "/"));
  const sep = item.path.includes("\\") ? "\\" : "/";
  const stem = item.name.replace(/\.[^.]+$/, "");
  return `${dir}${sep}${stem}.${format}`;
}

listen("conversion-progress", (event) => {
  const { job_id, percent, done, error } = event.payload;
  const item = queue.find((q) => q.id === job_id);
  if (!item) return;
  if (error) {
    item.status = "error";
    item.error = error;
    log(`✗ ${item.name}: ${error}`);
  } else if (done) {
    item.status = "done";
    item.percent = 100;
    log(`✓ Converted ${item.name}`);
  } else {
    item.status = "converting";
    item.percent = percent;
  }
  renderQueue();
});

convertBtn.addEventListener("click", async () => {
  if (!ffmpegPath || !ffprobePath) {
    alert("ffmpeg isn't available yet.");
    return;
  }
  const format = formatSelect.value;
  const bitrate = LOSSLESS_FORMATS.has(format) ? null : parseInt(bitrateSelect.value, 10);
  const sampleRate = samplerateSelect.value ? parseInt(samplerateSelect.value, 10) : null;
  const channels = channelsSelect.value ? parseInt(channelsSelect.value, 10) : null;

  convertBtn.disabled = true;
  const pending = queue.filter((q) => q.status === "pending" || q.status === "error");

  for (const item of pending) {
    item.status = "converting";
    item.percent = 0;
    item.error = null;
    renderQueue();

    const outputPath = outputPathFor(item, format);
    log(`Converting ${item.name} → ${format}…`);

    try {
      await invoke("convert_audio", {
        jobId: item.id,
        ffmpegPath,
        ffprobePath,
        options: {
          input_path: item.path,
          output_path: outputPath,
          format,
          bitrate_kbps: bitrate,
          sample_rate: sampleRate,
          channels: channels,
        },
      });
    } catch (e) {
      item.status = "error";
      item.error = String(e);
      log(`✗ ${item.name}: ${e}`);
      renderQueue();
    }
  }

  convertBtn.disabled = queue.every((q) => q.status === "done");
});

// ---- Init ---------------------------------------------------------------------
checkFfmpeg();
