const invoke = window.__TAURI__.core.invoke;

const $ = (id) => document.getElementById(id);
const logEl = $("log");

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

async function run(args, label) {
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
    if (/QEMU-WHPX VM: running/.test(out)) {
      setStatus("pill-run", "Running");
      $("vm-detail").textContent = "Your Linux desktop is running. Use Stop to power it off.";
    } else {
      setStatus("pill-off", "Stopped");
      $("vm-detail").textContent = "Click Launch to start your Arch Linux desktop.";
    }
  } catch (e) {
    setStatus("pill-off", "Stopped");
  }
}

$("btn-launch").onclick = () => {
  const args = ["launch", "--runtime", "qemu-whpx", "--detach"];
  if ($("sel-mode").value === "persistent") args.push("--persist-root");
  run(args, "Launching Linux desktop…");
};

$("btn-stop").onclick = () => run(["stop"], "Stopping…");
$("btn-refresh").onclick = () => refresh();

$("btn-install").onclick = () => {
  const de = $("sel-de").value;
  log("» Installing " + de.toUpperCase() + " desktop — this downloads packages and can take a while…");
  run(["install-desktop", "--de", de], null);
};

$("btn-provision").onclick = () => {
  const args = ["provision"];
  const user = $("in-user").value.trim();
  const pass = $("in-pass").value;
  if (user) args.push("--username", user);
  if (pass) args.push("--password", pass);
  run(args, "Setting credentials…");
};

$("btn-doctor").onclick = () => run(["doctor"], "Running diagnostics…");
$("btn-reset").onclick = () => log("Reset workspace: coming soon (will clear the root overlay).");
$("btn-clear").onclick = () => (logEl.textContent = "");

refresh();
