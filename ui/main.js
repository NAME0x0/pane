const invoke = window.__TAURI__.core.invoke;
const $ = (id) => document.getElementById(id);
const logEl = $("log");
const WS_URL = "ws://127.0.0.1:5700";

let rfb = null;
let retries = 0;
let specs = null;
const STORAGE_ROOT_KEY = "pane.storageRoot";
const ACTIVE_DESKTOP_KEY = "pane.activeDesktop.v2";
const DISPLAY_BACKEND_KEY = "pane.displayBackend";

function runtimeRoot() {
  return $("in-storage-root").value.trim();
}

function rememberRuntimeRoot() {
  const value = runtimeRoot();
  if (value) {
    localStorage.setItem(STORAGE_ROOT_KEY, value);
  } else {
    localStorage.removeItem(STORAGE_ROOT_KEY);
  }
}

function log(msg) {
  logEl.textContent += (logEl.textContent ? "\n" : "") + msg;
  logEl.scrollTop = logEl.scrollHeight;
}

function setStatus(kind, text) {
  const pill = $("status");
  pill.className = "pill " + kind;
  pill.textContent = text;
}

function busy(on) {
  document.querySelectorAll("button").forEach((b) => (b.disabled = on));
  if (on) setStatus("pill-busy", "Working…");
}

function updateTuningLabels() {
  $("out-vcpus").textContent = $("in-vcpus").value;
  $("out-memory").textContent = $("in-memory").value + " MiB";
  $("out-disk").textContent = $("in-disk").value + " GiB";
}

function applyRecommended() {
  if (!specs) return;
  $("in-vcpus").value = specs.recommended_vcpus;
  $("in-memory").value = specs.recommended_memory_mb;
  $("in-disk").value = specs.recommended_disk_gib;
  $("in-resolution").value = specs.recommended_resolution;
  $("in-gpu").checked = specs.gpu_acceleration_supported;
  updateTuningLabels();
}

async function loadRecommendedSpecs() {
  try {
    specs = await invoke("recommended_specs");
    $("host-summary").textContent =
      `${specs.logical_cores} logical cores, ${Math.round(specs.total_memory_mb / 1024)} GiB RAM`;
    $("gpu-summary").textContent = specs.gpu_name || "GPU detected by Windows";
    $("disk-summary").textContent =
      specs.free_disk_gib == null ? "Unknown" : `${specs.free_disk_gib} GiB free`;
    applyRecommended();
    log(`Recommended: ${specs.recommended_vcpus} vCPU, ${specs.recommended_memory_mb} MiB RAM, ${specs.recommended_disk_gib} GiB disk.`);
  } catch (e) {
    $("host-summary").textContent = "Could not detect host specs";
    $("gpu-summary").textContent = "Unknown";
    $("disk-summary").textContent = "Unknown";
    log("spec detection error: " + e);
    updateTuningLabels();
  }
}

async function engine(args, label) {
  busy(true);
  if (label) log("» " + label);
  try {
    rememberRuntimeRoot();
    const out = await invoke("engine_run", { args, runtimeRoot: runtimeRoot() || null });
    if (out && out.trim()) log(out.trim());
    return out || "";
  } catch (e) {
    log("error: " + e);
    throw e;
  } finally {
    busy(false);
    await refresh();
  }
}

async function refresh() {
  try {
    const out = await invoke("engine_run", { args: ["status"], runtimeRoot: runtimeRoot() || null });
    const running = /QEMU-WHPX VM: running/.test(out);
    setStatus(running ? "pill-run" : "pill-off", running ? "Running" : "Stopped");
    $("vm-detail").textContent = running
      ? "Your Linux desktop is running."
      : "Click Launch to start your Arch Linux desktop.";
    return running;
  } catch (e) {
    setStatus("pill-off", "Stopped");
    return false;
  }
}

async function ensureSelectedDesktop() {
  const de = $("sel-de").value;
  const diskGib = Number.parseInt($("in-disk").value, 10);
  const active = localStorage.getItem(ACTIVE_DESKTOP_KEY);
  if (active === de) return;
  const label = active
    ? `Switching desktop from ${active.toUpperCase()} to ${de.toUpperCase()}…`
    : `Preparing ${de.toUpperCase()} desktop…`;
  const args = ["install-desktop", "--de", de, "--disk-gib", String(diskGib)];
  await engine(args, label);
  localStorage.setItem(ACTIVE_DESKTOP_KEY, de);
}

// ---- embedded display (noVNC, loaded lazily so a failure can't break the UI) ----
function screenShown() {
  return $("screen-view").classList.contains("show");
}
function showScreen(on) {
  $("screen-view").classList.toggle("show", on);
}

async function connectDisplay() {
  disconnectDisplay();
  try {
    const mod = await import("./novnc/core/rfb.js");
    const RFB = mod.default;
    rfb = new RFB($("screen"), WS_URL, {});
    rfb.scaleViewport = true;
    rfb.resizeSession = false;
    rfb.addEventListener("connect", () => {
      retries = 0;
      log("Display connected.");
    });
    rfb.addEventListener("disconnect", () => {
      rfb = null;
      if (screenShown() && retries < 30) {
        retries += 1;
        setTimeout(connectDisplay, 1000);
      }
    });
  } catch (e) {
    log("display error: " + e);
  }
}

function disconnectDisplay() {
  if (rfb) {
    try { rfb.disconnect(); } catch (e) {}
    rfb = null;
  }
}

// ---- actions ----
$("btn-launch").onclick = async () => {
  const persist = $("sel-mode").value === "persistent";
  const vcpus = Number.parseInt($("in-vcpus").value, 10);
  const memoryMb = Number.parseInt($("in-memory").value, 10);
  const diskGib = Number.parseInt($("in-disk").value, 10);
  const resolution = $("in-resolution").value.trim();
  const gpuAcceleration = $("in-gpu").checked;
  const displayBackend = $("sel-display").value;
  localStorage.setItem(DISPLAY_BACKEND_KEY, displayBackend);
  busy(true);
  log(`» Opening desktop: ${vcpus} vCPU, ${memoryMb} MiB RAM, ${diskGib} GiB disk, ${resolution || "default resolution"}, ${displayBackend.toUpperCase()} window, GPU ${gpuAcceleration ? "on" : "off"}…`);
  try {
    busy(false);
    if (persist) {
      await ensureSelectedDesktop();
    }
    busy(true);
    rememberRuntimeRoot();
    await invoke("launch_vm", {
      persist,
      vcpus,
      memoryMb,
      diskGib,
      resolution,
      gpuAcceleration,
      displayBackend,
      runtimeRoot: runtimeRoot() || null
    });
    log("Desktop window opening. It may take a moment to reach the login screen.");
  } catch (e) {
    log("error: " + e);
  } finally {
    busy(false);
    setTimeout(refresh, 5000);
  }
};

async function stopVm() {
  busy(true);
  log("» Stopping…");
  try {
    await invoke("stop_vm");
    log("Stopped.");
  } catch (e) {
    log("error: " + e);
  } finally {
    busy(false);
    await refresh();
  }
}

$("btn-stop").onclick = stopVm;
$("btn-screen-stop").onclick = stopVm;
$("btn-back").onclick = () => showScreen(false);
$("btn-refresh").onclick = refresh;
$("btn-recommended").onclick = applyRecommended;
$("in-vcpus").oninput = updateTuningLabels;
$("in-memory").oninput = updateTuningLabels;
$("in-disk").oninput = updateTuningLabels;

$("btn-install").onclick = () => {
  const de = $("sel-de").value;
  const diskGib = $("in-disk").value;
  log("» Installing " + de.toUpperCase() + " desktop — downloads packages, can take a while…");
  engine(["install-desktop", "--de", de, "--disk-gib", diskGib], null)
    .then(() => localStorage.setItem(ACTIVE_DESKTOP_KEY, de))
    .catch(() => {});
};

$("btn-provision").onclick = () => {
  const args = ["provision"];
  const user = $("in-user").value.trim();
  const pass = $("in-pass").value;
  if (user) args.push("--username", user);
  if (pass) args.push("--password", pass);
  engine(args, "Setting credentials…");
};

$("btn-doctor").onclick = () => engine(["doctor"], "Running diagnostics…");
$("btn-reset").onclick = () => engine(["workspace", "--reset"], "Resetting workspace (back to a clean image)…");
$("btn-clear").onclick = () => (logEl.textContent = "");
$("in-storage-root").onchange = () => {
  rememberRuntimeRoot();
  log(runtimeRoot()
    ? `Storage root set to ${runtimeRoot()}\\Pane for future commands.`
    : "Storage root reset to the Windows default.");
  refresh();
};

log("Pane ready.");
// On startup, if a VM is already running, attach its display automatically.
(async () => {
  $("in-storage-root").value = localStorage.getItem(STORAGE_ROOT_KEY) || "";
  $("sel-display").value = localStorage.getItem(DISPLAY_BACKEND_KEY) || "sdl";
  await loadRecommendedSpecs();
  const running = await refresh();
  if (running) log("A Linux VM is already running — its window is open. Use Stop to power it off.");
})();
