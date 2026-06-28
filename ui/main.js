import RFB from "./novnc/core/rfb.js";

const invoke = window.__TAURI__.core.invoke;
const $ = (id) => document.getElementById(id);
const logEl = $("log");
const WS_URL = "ws://127.0.0.1:5700";

let rfb = null;
let retries = 0;

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
  document.querySelectorAll("#control-actions button, .grid button").forEach((b) => (b.disabled = on));
  if (on) setStatus("pill-busy", "Working…");
}

async function engine(args, label) {
  busy(true);
  if (label) log("» " + label);
  try {
    const out = await invoke("engine_run", { args });
    if (out && out.trim()) log(out.trim());
    return out || "";
  } catch (e) {
    log("error: " + e);
    return "";
  } finally {
    busy(false);
    await refresh();
  }
}

async function refresh() {
  try {
    const out = await invoke("engine_run", { args: ["status"] });
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

// ---- embedded display (noVNC) ----
function showScreen(on) {
  $("screen-view").hidden = !on;
}

function connectDisplay() {
  disconnectDisplay();
  try {
    rfb = new RFB($("screen"), WS_URL, {});
    rfb.scaleViewport = true;
    rfb.resizeSession = false;
    rfb.addEventListener("connect", () => {
      retries = 0;
      log("Display connected.");
    });
    rfb.addEventListener("disconnect", (e) => {
      rfb = null;
      // The VM may still be booting; retry a few times before giving up.
      if (!$("screen-view").hidden && retries < 20) {
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
  const args = ["launch", "--runtime", "qemu-whpx", "--display", "vnc", "--detach"];
  if ($("sel-mode").value === "persistent") args.push("--persist-root");
  await engine(args, "Launching Linux desktop…");
  retries = 0;
  showScreen(true);
  setTimeout(connectDisplay, 1500);
};

async function stopVm() {
  disconnectDisplay();
  showScreen(false);
  await engine(["stop"], "Stopping…");
}

$("btn-stop").onclick = stopVm;
$("btn-screen-stop").onclick = stopVm;
$("btn-back").onclick = () => showScreen(false);
$("btn-refresh").onclick = refresh;

$("btn-install").onclick = () => {
  const de = $("sel-de").value;
  log("» Installing " + de.toUpperCase() + " desktop — downloads packages, can take a while…");
  engine(["install-desktop", "--de", de], null);
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

refresh();
