(() => {
  const $ = (sel) => document.querySelector(sel);
  const output = $("#output");
  const prompt = $("#prompt");
  const promptText = $("#prompt-text");
  const choices = $("#choices");
  const hero = $("#hero");
  const heroText = $("#hero-text");
  const heroLinks = $("#hero-links");
  const noResponse = $("#no-response");
  const footer = $("#footer");
  const btnYes = $("#btn-yes");
  const btnNo = $("#btn-no");

  let resolved = false;

  function wait(ms) {
    return new Promise((r) => setTimeout(r, ms));
  }

  function addLine(container, text, cls = "") {
    const div = document.createElement("div");
    div.className = `line fade-in ${cls}`.trim();
    div.textContent = text;
    container.appendChild(div);
    return div;
  }

  async function typeText(element, text, speed = 35) {
    for (const char of text) {
      element.textContent += char;
      await wait(speed);
    }
  }

  function show(el) {
    el.classList.remove("hidden");
    el.classList.add("fade-in");
  }

  function hide(el) {
    el.classList.add("hidden");
    el.classList.remove("fade-in");
  }

  async function intro() {
    await wait(400);
    addLine(output, "pane v0.1.0", "line-dim");
    await wait(300);
    addLine(output, "");
    await wait(200);

    show(prompt);
    await typeText(promptText, "Tired of WSL2 being just a CLI?", 40);
    await wait(600);

    show(choices);
    btnYes.focus();

    document.addEventListener("keydown", handleKey);
    btnYes.addEventListener("click", () => respond(true));
    btnNo.addEventListener("click", () => respond(false));
  }

  function handleKey(e) {
    if (resolved) return;
    if (e.key === "y" || e.key === "Y") respond(true);
    if (e.key === "n" || e.key === "N") respond(false);
    if (e.key === "Enter") {
      const focused = document.activeElement;
      if (focused === btnYes) respond(true);
      else if (focused === btnNo) respond(false);
    }
    if (e.key === "ArrowLeft" || e.key === "ArrowRight" || e.key === "Tab") {
      e.preventDefault();
      const next = document.activeElement === btnYes ? btnNo : btnYes;
      next.focus();
    }
  }

  async function respond(yes) {
    if (resolved) return;
    resolved = true;

    const selected = yes ? btnYes : btnNo;
    selected.classList.add("selected");

    document.removeEventListener("keydown", handleKey);

    await wait(200);
    hide(choices);
    hide(prompt);

    if (yes) {
      await yesPath();
    } else {
      await noPath();
    }
  }

  async function yesPath() {
    show(hero);

    await wait(300);
    addLine(heroText, "We get you.", "line-bright");
    await wait(500);
    addLine(heroText, "");
    await wait(200);
    addLine(heroText, "That's why we built Pane.", "line-accent");
    await wait(400);
    addLine(heroText, "");
    await wait(200);
    addLine(heroText, "A full Linux desktop on Windows.", "line");
    addLine(heroText, "One click. No dual boot. No VM hassle.", "line");
    await wait(400);
    addLine(heroText, "");
    addLine(heroText, "We're just getting started.", "line-dim");
    await wait(500);

    show(heroLinks);
    show(footer);
  }

  async function noPath() {
    show(noResponse);

    await wait(300);
    addLine(noResponse, "Fair enough.", "line-bright");
    await wait(500);
    addLine(noResponse, "");
    await wait(200);
    addLine(noResponse, "We'll be here when you change your mind.", "line-dim");
    await wait(600);
    addLine(noResponse, "");

    const link = document.createElement("a");
    link.href = "https://github.com/NAME0x0/pane";
    link.className = "link fade-in";
    link.innerHTML = '<span class="link-icon">*</span> github.com/pane';
    noResponse.appendChild(link);

    show(footer);
  }

  intro();
})();
