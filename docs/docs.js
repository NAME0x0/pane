(() => {
  const documents = {
    "mvp-arch": { title: "Arch MVP Guide", file: "mvp-arch.md" },
    "product-contract": { title: "Product Contract", file: "product-contract.md" },
    "native-runtime-architecture": {
      title: "Native Runtime Architecture",
      file: "native-runtime-architecture.md",
    },
    "clean-machine-validation": {
      title: "Clean Machine Validation",
      file: "clean-machine-validation.md",
    },
    "vision": { title: "Vision", file: "vision.md" },
    "phase-1-audit": { title: "Phase 1 Audit", file: "phase-1-audit.md" },
    "phase-1-brief": { title: "Phase 1 Brief", file: "phase-1-brief.md" },
  };

  const nav = document.querySelector("#docs-nav");
  const content = document.querySelector("#doc-content");
  const params = new URLSearchParams(window.location.search);
  const requested = params.get("doc") || "mvp-arch";
  const activeSlug = documents[requested] ? requested : "mvp-arch";

  function escapeHtml(value) {
    return value
      .replaceAll("&", "&amp;")
      .replaceAll("<", "&lt;")
      .replaceAll(">", "&gt;")
      .replaceAll('"', "&quot;");
  }

  function slugify(value) {
    return value
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-|-$/g, "");
  }

  function renderInline(value) {
    const escaped = escapeHtml(value);
    return escaped.replace(/\[([^\]]+)\]\(([^)]+)\)/g, (_match, label, href) => {
      const safeLabel = escapeHtml(label);
      let safeHref = escapeHtml(href);
      const markdownMatch = href.match(/^([a-z0-9-]+)\.md(#.*)?$/i);
      if (markdownMatch) {
        safeHref = `doc.html?doc=${markdownMatch[1]}${markdownMatch[2] || ""}`;
      }
      return `<a href="${safeHref}">${safeLabel}</a>`;
    });
  }

  function renderTable(lines) {
    const rows = lines
      .filter((line) => !/^\|\s*:?-{3,}:?\s*(\|\s*:?-{3,}:?\s*)+\|?$/.test(line))
      .map((line) =>
        line
          .replace(/^\||\|$/g, "")
          .split("|")
          .map((cell) => renderInline(cell.trim()))
      );

    if (rows.length === 0) return "";

    const [head, ...body] = rows;
    const header = `<thead><tr>${head.map((cell) => `<th>${cell}</th>`).join("")}</tr></thead>`;
    const bodyRows = body
      .map((row) => `<tr>${row.map((cell) => `<td>${cell}</td>`).join("")}</tr>`)
      .join("");
    return `<div class="doc-table-wrap"><table>${header}<tbody>${bodyRows}</tbody></table></div>`;
  }

  function renderMarkdown(markdown) {
    const lines = markdown.replace(/\r\n/g, "\n").split("\n");
    const html = [];
    let paragraph = [];
    let list = [];
    let code = [];
    let table = [];
    let inCode = false;

    function flushParagraph() {
      if (paragraph.length > 0) {
        html.push(`<p>${renderInline(paragraph.join(" "))}</p>`);
        paragraph = [];
      }
    }

    function flushList() {
      if (list.length > 0) {
        html.push(`<ul>${list.map((item) => `<li>${renderInline(item)}</li>`).join("")}</ul>`);
        list = [];
      }
    }

    function flushTable() {
      if (table.length > 0) {
        html.push(renderTable(table));
        table = [];
      }
    }

    for (const line of lines) {
      if (line.startsWith("```")) {
        if (inCode) {
          html.push(`<pre><code>${escapeHtml(code.join("\n"))}</code></pre>`);
          code = [];
          inCode = false;
        } else {
          flushParagraph();
          flushList();
          flushTable();
          inCode = true;
        }
        continue;
      }

      if (inCode) {
        code.push(line);
        continue;
      }

      if (/^\s*$/.test(line)) {
        flushParagraph();
        flushList();
        flushTable();
        continue;
      }

      if (line.startsWith("|")) {
        flushParagraph();
        flushList();
        table.push(line);
        continue;
      }

      const heading = line.match(/^(#{1,4})\s+(.+)$/);
      if (heading) {
        flushParagraph();
        flushList();
        flushTable();
        const level = heading[1].length;
        const text = heading[2].trim();
        html.push(`<h${level} id="${slugify(text)}">${renderInline(text)}</h${level}>`);
        continue;
      }

      const bullet = line.match(/^\s*-\s+(.+)$/);
      if (bullet) {
        flushParagraph();
        flushTable();
        list.push(bullet[1]);
        continue;
      }

      paragraph.push(line.trim());
    }

    flushParagraph();
    flushList();
    flushTable();

    return html.join("\n");
  }

  function renderNav() {
    nav.innerHTML = Object.entries(documents)
      .map(([slug, doc]) => {
        const current = slug === activeSlug ? ' aria-current="page"' : "";
        return `<a href="doc.html?doc=${slug}"${current}>${escapeHtml(doc.title)}</a>`;
      })
      .join("");
  }

  async function loadDoc() {
    renderNav();
    const doc = documents[activeSlug];
    document.title = `${doc.title} - Pane Docs`;

    try {
      const response = await fetch(doc.file);
      if (!response.ok) {
        throw new Error(`HTTP ${response.status}`);
      }
      const markdown = await response.text();
      content.innerHTML = `
        <p class="doc-kicker">Pane documentation</p>
        ${renderMarkdown(markdown)}
      `;
    } catch (error) {
      content.innerHTML = `
        <p class="doc-kicker">Pane documentation</p>
        <h1>Documentation could not load</h1>
        <p>The source Markdown exists in the repository, but this page could not fetch it from GitHub Pages.</p>
        <pre><code>${escapeHtml(String(error))}</code></pre>
      `;
    }
  }

  loadDoc();
})();
