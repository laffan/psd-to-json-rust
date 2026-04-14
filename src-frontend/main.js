import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";

// ── State ──────────────────────────────────────────────────
let psdSelected = false;
let outputSelected = false;
let processing = false;
let isMobile = false; // set during init

// ── DOM refs ───────────────────────────────────────────────
const btnSelectPsd = document.getElementById("btn-select-psd");
const btnSelectOutput = document.getElementById("btn-select-output");
const btnProcess = document.getElementById("btn-process");
const psdPathDisplay = document.getElementById("psd-path-display");
const outputPathDisplay = document.getElementById("output-path-display");
const terminal = document.getElementById("terminal");
const statusBar = document.getElementById("status-bar");
const thumbnailGrid = document.getElementById("thumbnail-grid");
const thumbnailEmpty = document.getElementById("thumbnail-empty");

// Option inputs
const optTileSize = document.getElementById("opt-tile-size");
const optTileScales = document.getElementById("opt-tile-scales");
const optPngLow = document.getElementById("opt-png-low");
const optPngHigh = document.getElementById("opt-png-high");
const optJpgQuality = document.getElementById("opt-jpg-quality");
const optIgnore = document.getElementById("opt-ignore");
const optMetadataOnly = document.getElementById("opt-metadata-only");

// ── Options toggle ─────────────────────────────────────────
const optionsToggle = document.getElementById("options-toggle");
const optionsBody = document.getElementById("options-body");
const optionsArrow = optionsToggle.querySelector(".options-arrow");

optionsToggle.addEventListener("click", () => {
  const isOpen = optionsBody.classList.toggle("open");
  optionsArrow.classList.toggle("open", isOpen);
});

// ── Init: detect mobile and auto-set output dir ───────────
(async () => {
  try {
    const defaultDir = await invoke("get_default_output_dir");
    if (defaultDir) {
      // Mobile: auto-set output dir to app sandbox
      isMobile = true;
      await invoke("set_output_dir", { path: defaultDir });
      outputPathDisplay.textContent = "App Documents";
      outputPathDisplay.classList.add("has-value");
      outputSelected = true;
      btnSelectOutput.textContent = "App Documents";
      btnSelectOutput.disabled = true;
      btnSelectOutput.classList.add("btn-disabled");
      updateProcessButton();
    }
  } catch (e) {
    console.error("Init error:", e);
  }
})();

// ── Tab switching ──────────────────────────────────────────
document.querySelectorAll(".tab-btn").forEach((btn) => {
  btn.addEventListener("click", () => {
    document.querySelectorAll(".tab-btn").forEach((b) => b.classList.remove("active"));
    document.querySelectorAll(".tab-panel").forEach((p) => p.classList.remove("active"));
    btn.classList.add("active");
    document.getElementById("panel-" + btn.dataset.tab).classList.add("active");

    if (btn.dataset.tab === "thumbnails") {
      refreshThumbnails();
    }
  });
});

// ── Select PSD ─────────────────────────────────────────────
btnSelectPsd.addEventListener("click", async () => {
  // On iOS, using image/* MIME types triggers the photo picker even with
  // pickerMode "document". Use com.adobe.photoshop-image (the UTType for
  // PSD files) as the extension on mobile, which the document picker
  // understands. On desktop, the bare "psd" extension works fine.
  const filters = isMobile
    ? [{ name: "Photoshop", extensions: ["com.adobe.photoshop-image"] }]
    : [{ name: "Photoshop", extensions: ["psd"] }];

  const path = await open({
    multiple: false,
    filters,
    pickerMode: "document",
  });
  if (path) {
    try {
      const name = await invoke("set_psd_path", { path });
      psdPathDisplay.textContent = name;
      psdPathDisplay.classList.add("has-value");
      psdSelected = true;
      updateProcessButton();
      logLine("info", `PSD selected: ${path}`);
    } catch (e) {
      logLine("error", `Error: ${e}`);
    }
  }
});

// ── Select Output Dir ──────────────────────────────────────
btnSelectOutput.addEventListener("click", async () => {
  const path = await open({ directory: true, pickerMode: "document" });
  if (path) {
    try {
      const display = await invoke("set_output_dir", { path });
      outputPathDisplay.textContent = display;
      outputPathDisplay.classList.add("has-value");
      outputSelected = true;
      updateProcessButton();
      logLine("info", `Output directory: ${path}`);
    } catch (e) {
      logLine("error", `Error: ${e}`);
    }
  }
});

// ── Process ────────────────────────────────────────────────
btnProcess.addEventListener("click", async () => {
  if (processing) return;
  processing = true;
  updateProcessButton();
  statusBar.textContent = "Processing...";

  terminal.innerHTML = "";
  logLine("info", "Starting PSD processing...");

  try {
    await invoke("process_psd", { options: gatherOptions() });
    logLine("success", "Processing complete!");
    statusBar.textContent = "Done";

    // Auto-switch to thumbnails tab
    document.querySelectorAll(".tab-btn").forEach((b) => b.classList.remove("active"));
    document.querySelectorAll(".tab-panel").forEach((p) => p.classList.remove("active"));
    document.querySelector('[data-tab="thumbnails"]').classList.add("active");
    document.getElementById("panel-thumbnails").classList.add("active");
    refreshThumbnails();
  } catch (e) {
    logLine("error", `Processing failed: ${e}`);
    statusBar.textContent = "Error";
  } finally {
    processing = false;
    updateProcessButton();
  }
});

// ── Log events from backend ────────────────────────────────
let treeBuffer = null; // collects lines between LAYER_TREE_START / END

listen("log-line", (event) => {
  const msg = event.payload;

  // Tree block accumulation
  if (msg === "LAYER_TREE_START") {
    treeBuffer = [];
    return;
  }
  if (msg === "LAYER_TREE_END") {
    if (treeBuffer) {
      renderTree(treeBuffer);
    }
    treeBuffer = null;
    return;
  }
  if (treeBuffer !== null) {
    treeBuffer.push(msg);
    return;
  }

  // Normal log lines
  let cls = "info";
  const lower = msg.toLowerCase();
  if (lower.includes("error") || lower.includes("fail")) {
    cls = "error";
  } else if (lower.includes("done") || lower.includes("complete") || lower.includes("finished")) {
    cls = "success";
  } else if (msg.startsWith("{") || msg.startsWith("[")) {
    cls = "data";
  }
  logLine(cls, msg);
});

// ── Thumbnail refresh ──────────────────────────────────────
async function refreshThumbnails() {
  try {
    const files = await invoke("list_output_files");
    if (!files || files.length === 0) {
      thumbnailGrid.style.display = "none";
      thumbnailEmpty.style.display = "flex";
      return;
    }

    thumbnailGrid.style.display = "grid";
    thumbnailEmpty.style.display = "none";
    thumbnailGrid.innerHTML = "";

    for (const file of files) {
      const card = document.createElement("div");
      card.className = "thumb-card";

      const imgWrap = document.createElement("div");
      imgWrap.className = "thumb-img-wrap";

      if (file.is_json) {
        const icon = document.createElement("div");
        icon.className = "thumb-json-icon";
        icon.textContent = "{ }";
        imgWrap.appendChild(icon);
      } else {
        const img = document.createElement("img");
        img.alt = file.filename;
        img.loading = "lazy";
        loadThumb(file.absolute_path, img);
        imgWrap.appendChild(img);
      }

      const label = document.createElement("div");
      label.className = "thumb-label";

      const parts = file.relative_path.split("/");
      const dir = parts.length > 1 ? parts.slice(0, -1).join("/") : "";
      label.innerHTML = `${file.filename}${dir ? `<span class="thumb-dir">${dir}</span>` : ""}`;

      card.appendChild(imgWrap);
      card.appendChild(label);
      thumbnailGrid.appendChild(card);
    }
  } catch (e) {
    console.error("Thumbnail refresh error:", e);
  }
}

async function loadThumb(absolutePath, imgEl) {
  try {
    const dataUrl = await invoke("get_thumbnail", { path: absolutePath, maxSize: 200 });
    imgEl.src = dataUrl;
  } catch (e) {
    imgEl.alt = "Error";
  }
}

// ── Tree renderer ─────────────────────────────────────────

function renderTree(lines) {
  const block = document.createElement("div");
  block.className = "tree-block";

  for (const raw of lines) {
    if (raw === "") continue;

    const row = document.createElement("div");
    row.className = "tree-line";

    // Match the tag pattern: [X]
    const tagMatch = raw.match(/\[([GSTZP?])\]/);
    if (tagMatch) {
      const tagChar = tagMatch[1];
      const tagIdx = raw.indexOf(tagMatch[0]);

      // Prefix: the tree connectors (├── │ └──)
      const prefix = raw.slice(0, tagIdx);
      const prefixSpan = document.createElement("span");
      prefixSpan.className = "tree-prefix";
      prefixSpan.textContent = prefix;
      row.appendChild(prefixSpan);

      // Tag badge
      const badge = document.createElement("span");
      badge.className = "tree-tag";
      badge.textContent = tagChar;

      row.appendChild(badge);

      // Rest of the line after the tag
      const rest = raw.slice(tagIdx + tagMatch[0].length);
      // Split into name vs annotations (parenthesized type, %, blend, [mask])
      const nameMatch = rest.match(/^\s*(\S+)(.*)/);
      if (nameMatch) {
        const nameSpan = document.createElement("span");
        nameSpan.className = "tree-name";
        nameSpan.textContent = " " + nameMatch[1];
        row.appendChild(nameSpan);

        if (nameMatch[2]) {
          const annoSpan = document.createElement("span");
          annoSpan.className = "tree-annotation";
          annoSpan.textContent = nameMatch[2];
          row.appendChild(annoSpan);
        }
      }
    } else {
      // Header line (e.g. "demo (4096x2048)")
      row.className = "tree-line tree-header";
      row.textContent = raw;
    }

    block.appendChild(row);
  }

  terminal.appendChild(block);
  terminal.scrollTop = terminal.scrollHeight;
}

// ── Helpers ────────────────────────────────────────────────
function logLine(cls, text) {
  const line = document.createElement("div");
  line.className = `line ${cls}`;
  line.textContent = text;
  terminal.appendChild(line);
  terminal.scrollTop = terminal.scrollHeight;
}

function gatherOptions() {
  // Parse tile scaled versions: comma-separated numbers
  const scalesRaw = optTileScales.value.trim();
  const tileScaledVersions = scalesRaw
    ? scalesRaw.split(",").map((s) => parseInt(s.trim(), 10)).filter((n) => !isNaN(n) && n > 0)
    : [];

  // Parse ignore layers: comma-separated names
  const ignoreRaw = optIgnore.value.trim();
  const ignoreLayers = ignoreRaw
    ? ignoreRaw.split(",").map((s) => s.trim()).filter((s) => s.length > 0)
    : [];

  return {
    tile_slice_size: parseInt(optTileSize.value, 10) || 512,
    tile_scaled_versions: tileScaledVersions,
    png_quality_low: parseInt(optPngLow.value, 10) || 45,
    png_quality_high: parseInt(optPngHigh.value, 10) || 65,
    jpg_quality: parseInt(optJpgQuality.value, 10) || 85,
    ignore_layers: ignoreLayers,
    metadata_only: optMetadataOnly.checked,
  };
}

function updateProcessButton() {
  const canProcess = psdSelected && outputSelected && !processing;
  btnProcess.disabled = !canProcess;
  if (processing) {
    btnProcess.innerHTML = '<span class="spinner"></span> Processing...';
  } else {
    btnProcess.textContent = "Process PSD";
  }
}
