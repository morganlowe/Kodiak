(function () {
  "use strict";

  // 1. Lock viewport-wide dark color-scheme and prevent Dark Reader double-inversion
  let csMeta = document.querySelector('meta[name="color-scheme"]');
  if (!csMeta) {
    csMeta = document.createElement("meta");
    csMeta.name = "color-scheme";
    document.head.prepend(csMeta);
  }
  csMeta.content = "dark";

  if (!document.querySelector('meta[name="darkreader-lock"]')) {
    const drLock = document.createElement("meta");
    drLock.name = "darkreader-lock";
    document.head.prepend(drLock);
  }

  try {
    for (let i = localStorage.length - 1; i >= 0; i--) {
      const k = localStorage.key(i);
      if (k && k.includes("__palette")) localStorage.removeItem(k);
    }
  } catch (_) {}
  document.body?.setAttribute("data-md-color-scheme", "slate");

  // 2. Feature-detect Invoker Commands on HTMLButtonElement.prototype and provide fallback
  const hasInvokerSupport = "commandForElement" in HTMLButtonElement.prototype;
  if (!hasInvokerSupport) {
    document.addEventListener("click", (event) => {
      const btn = event.composedPath().find((el) => el.matches?.("button[commandfor]"));
      if (!btn) return;
      const target = document.getElementById(btn.getAttribute("commandfor"));
      const cmd = btn.getAttribute("command");
      if (target instanceof HTMLDialogElement) {
        if (cmd === "close") target.close();
        else if (cmd === "show-modal") target.showModal();
      }
    });
  }

  let cachedCatalog = null;
  const pageCache = new Map();

  function runWithViewTransition(updateFn, focusTargetEl) {
    if (!document.startViewTransition) {
      updateFn();
      focusTargetEl?.focus({ preventScroll: true });
      return;
    }
    const transition = document.startViewTransition(() => {
      updateFn();
    });
    if (focusTargetEl) {
      transition.finished.finally(() => {
        focusTargetEl.focus({ preventScroll: true });
      });
    }
  }

  function escapeHtml(str) {
    const div = document.createElement("div");
    div.textContent = str ?? "";
    return div.innerHTML;
  }

  async function loadCatalog(forceRefresh = false) {
    if (cachedCatalog && !forceRefresh) return cachedCatalog;
    try {
      const url = forceRefresh ? `projects.json?t=${Date.now()}` : "projects.json";
      const res = await fetch(url, {
        headers: { Accept: "application/json" },
        cache: forceRefresh ? "no-store" : "default",
      });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      cachedCatalog = await res.json();
      return cachedCatalog;
    } catch (_err) {
      // Fallback: derive catalog from MkDocs navigation links if projects.json is unavailable
      const links = Array.from(document.querySelectorAll(".md-nav__link[href]"))
        .filter((a) => {
          const href = a.getAttribute("href");
          return href && !href.startsWith("#") && href !== "." && href !== "index.html" && !href.endsWith("index/");
        });
      const seen = new Set();
      cachedCatalog = [];
      for (const a of links) {
        const url = a.getAttribute("href");
        if (seen.has(url)) continue;
        seen.add(url);
        const name = a.textContent.trim();
        cachedCatalog.push({
          name,
          title: name,
          url,
          category: "Projects",
          summary: `Project documentation and build reference for ${name}.`,
          tags: ["Project"],
        });
      }
      return cachedCatalog;
    }
  }

  function rewriteFileUrls(rootEl = document) {
    const host = window.location.hostname || "benchbear.localhost";
    rootEl.querySelectorAll('a[href*=":8765/"], img[src*=":8765/"]').forEach((el) => {
      if (el.hasAttribute("href")) {
        el.setAttribute(
          "href",
          el.getAttribute("href").replace(/:\/\/(?:wiki\.local|bench\.bear|benchbear\.localhost):8765\//g, `://${host}:8765/`)
        );
      }
      if (el.hasAttribute("src")) {
        el.setAttribute(
          "src",
          el.getAttribute("src").replace(/:\/\/(?:wiki\.local|bench\.bear|benchbear\.localhost):8765\//g, `://${host}:8765/`)
        );
      }
    });
  }

  async function initExplorer() {
    rewriteFileUrls(document);
    const app = document.getElementById("wiki-explorer-app");
    if (!app || app.dataset.initialized === "true") return;
    app.dataset.initialized = "true";

    const searchInput = document.getElementById("project-search");
    const sortSelect = document.getElementById("project-sort");
    const gridBtn = document.getElementById("view-grid-btn");
    const listBtn = document.getElementById("view-list-btn");
    const bootBtn = document.getElementById("boot-toggle-btn");
    const syncBtn = document.getElementById("sync-projects-btn");
    const randomPeekBtn = document.getElementById("random-peek-btn");
    const categoryPillsEl = document.getElementById("category-pills");
    const resultsMetaEl = document.getElementById("results-meta");
    const resetFiltersBtn = document.getElementById("reset-filters-btn");
    const gridEl = document.getElementById("projects-grid");
    const dialog = document.getElementById("project-preview-dialog");
    const modalTitle = document.getElementById("modal-project-title");
    const modalCategory = document.getElementById("modal-project-category");
    const modalBody = document.getElementById("modal-project-body");
    const modalFullLink = document.getElementById("modal-fullpage-link");
    const modalRandomBtn = document.getElementById("modal-random-btn");
    const modalTuneBtn = document.getElementById("modal-tune-btn");
    const tuneDrawer = document.getElementById("modal-tune-drawer");
    const tunePromptInput = document.getElementById("tune-prompt-input");
    const tuneAiBtn = document.getElementById("tune-ai-btn");
    const tuneTitleInput = document.getElementById("tune-title-input");
    const tuneCategorySelect = document.getElementById("tune-category-select");
    const tuneOverviewInput = document.getElementById("tune-overview-input");
    const tuneSaveBtn = document.getElementById("tune-save-btn");
    const tuneCancelBtn = document.getElementById("tune-cancel-btn");
    const tuneStatusMsg = document.getElementById("tune-status-msg");

    let currentModalItem = null;
    const apiBase = `http://${window.location.hostname || "127.0.0.1"}:8765`;

    let catalog = await loadCatalog();

    const state = {
      query: "",
      category: "All",
      sort: "name-asc",
      view: "grid",
    };

    let counts = { All: 0 };
    let categories = ["All"];

    function rebuildCategoryCounts() {
      counts = { All: catalog.length };
      for (const item of catalog) {
        counts[item.category] = (counts[item.category] || 0) + 1;
      }
      categories = ["All", ...Object.keys(counts).filter((k) => k !== "All")];
    }

    rebuildCategoryCounts();

    function renderCategoryPills() {
      categoryPillsEl.innerHTML = categories
        .map((cat) => {
          const activeClass = state.category === cat ? "is-active" : "";
          const pressed = state.category === cat ? "true" : "false";
          return `
            <button type="button" class="category-pill ${activeClass}" data-category="${escapeHtml(cat)}" aria-pressed="${pressed}">
              <span>${escapeHtml(cat)}</span>
              <span class="category-pill__count">${counts[cat] || 0}</span>
            </button>
          `;
        })
        .join("");
    }

    function getFilteredProjects() {
      const q = state.query.trim().toLowerCase();
      const filtered = catalog.filter((item) => {
        if (state.category !== "All" && item.category !== state.category) {
          return false;
        }
        if (!q) return true;
        const haystack = `${item.name} ${item.title} ${item.category} ${item.summary} ${(item.tags || []).join(" ")}`.toLowerCase();
        return haystack.includes(q);
      });

      filtered.sort((a, b) => {
        if (state.sort === "name-desc") {
          return b.name.localeCompare(a.name, undefined, { sensitivity: "base", numeric: true });
        }
        if (state.sort === "modified-desc") {
          const diff = (b.modified_ts || 0) - (a.modified_ts || 0);
          if (diff !== 0) return diff;
        }
        if (state.sort === "modified-asc") {
          const diff = (a.modified_ts || 0) - (b.modified_ts || 0);
          if (diff !== 0) return diff;
        }
        if (state.sort === "created-desc") {
          const diff = (b.created_ts || 0) - (a.created_ts || 0);
          if (diff !== 0) return diff;
        }
        if (state.sort === "created-asc") {
          const diff = (a.created_ts || 0) - (b.created_ts || 0);
          if (diff !== 0) return diff;
        }
        if (state.sort === "category") {
          const catCmp = a.category.localeCompare(b.category);
          if (catCmp !== 0) return catCmp;
        }
        return a.name.localeCompare(b.name, undefined, { sensitivity: "base", numeric: true });
      });

      return filtered;
    }

    function renderGrid(withTransition = false) {
      const filtered = getFilteredProjects();
      const isFiltered = state.query.trim() !== "" || state.category !== "All";
      resetFiltersBtn.hidden = !isFiltered;

      const updateDOM = () => {
        resultsMetaEl.textContent = isFiltered
          ? `Showing ${filtered.length} of ${catalog.length} projects`
          : `Showing all ${catalog.length} indexed projects`;

        gridEl.classList.toggle("is-list-view", state.view === "list");

        if (filtered.length === 0) {
          gridEl.innerHTML = `
            <div class="empty-state">
              <p><strong>No projects matched "${escapeHtml(state.query)}"</strong> in <em>${escapeHtml(state.category)}</em>.</p>
              <button type="button" class="explorer-btn explorer-btn--ghost" id="empty-reset-btn">Clear Search & Filters</button>
            </div>
          `;
          return;
        }

        gridEl.innerHTML = filtered
          .map((item) => {
            const tagsHtml = (item.tags || [])
              .map(
                (t) =>
                  `<button type="button" class="card-tag" data-tag="${escapeHtml(t)}" title="Filter by ${escapeHtml(t)}">${escapeHtml(t)}</button>`
              )
              .join("");

            const datesHtml =
              item.modified_date || item.created_date
                ? `<div class="project-card__dates" title="Project filesystem timestamps">
                    ${
                      item.modified_date
                        ? `<span>Modified: ${escapeHtml(item.modified_date)}</span>`
                        : ""
                    }
                    ${
                      item.created_date && item.created_date !== item.modified_date
                        ? `<span>Created: ${escapeHtml(item.created_date)}</span>`
                        : ""
                    }
                  </div>`
                : "";

            return `
              <article class="project-card" data-url="${escapeHtml(item.url)}">
                <div class="project-card__header">
                  <span class="card-category-badge">${escapeHtml(item.category)}</span>
                  <h3 class="project-card__title">
                    <a href="${escapeHtml(item.url)}">${escapeHtml(item.name)}</a>
                  </h3>
                  ${datesHtml}
                </div>
                <p class="project-card__summary">${escapeHtml(item.summary)}</p>
                <div class="project-card__footer">
                  <div class="card-tags">${tagsHtml}</div>
                  <div class="card-actions">
                    <button
                      type="button"
                      class="explorer-btn explorer-btn--ghost quick-peek-btn"
                      data-url="${escapeHtml(item.url)}"
                      data-name="${escapeHtml(item.name)}"
                      data-category="${escapeHtml(item.category)}"
                    >
                      Quick Peek
                    </button>
                    <a href="${escapeHtml(item.url)}" class="card-action-link">
                      Open Page →
                    </a>
                  </div>
                </div>
              </article>
            `;
          })
          .join("");
      };

      if (withTransition) {
        runWithViewTransition(updateDOM);
      } else {
        updateDOM();
      }
    }

    async function openQuickPeek(item) {
      if (!dialog) {
        window.location.href = item.url;
        return;
      }

      currentModalItem = item;
      if (tuneDrawer) {
        tuneDrawer.hidden = true;
        modalTuneBtn?.setAttribute("aria-expanded", "false");
      }

      modalTitle.textContent = item.name;
      modalCategory.textContent = item.category || "Project";
      modalFullLink.href = item.url;
      modalBody.innerHTML = `<div class="modal-loading">Loading <strong>${escapeHtml(item.name)}</strong> via AJAX...</div>`;

      if (!dialog.open) {
        dialog.showModal();
      }
      modalTitle.focus({ preventScroll: true });

      try {
        let articleHtml = pageCache.get(item.url);
        if (!articleHtml) {
          const response = await fetch(item.url, { headers: { Accept: "text/html" } });
          if (!response.ok) throw new Error(`HTTP ${response.status}`);
          const htmlText = await response.text();
          const parser = new DOMParser();
          const doc = parser.parseFromString(htmlText, "text/html");
          const article = doc.querySelector("article.md-content__inner") || doc.querySelector("main") || doc.body;
          article.querySelectorAll(".headerlink, .md-content__button").forEach((el) => el.remove());
          articleHtml = article.innerHTML;
          pageCache.set(item.url, articleHtml);
        }
        modalBody.innerHTML = articleHtml;
        rewriteFileUrls(modalBody);
        modalBody.scrollTop = 0;
      } catch (_err) {
        modalBody.innerHTML = `
          <div class="empty-state">
            <p>${escapeHtml(item.summary)}</p>
            <p><a href="${escapeHtml(item.url)}" class="explorer-btn explorer-btn--primary">Open ${escapeHtml(item.name)} Full Page →</a></p>
          </div>
        `;
      }
    }

    function openRandomProject() {
      const pool = getFilteredProjects();
      const source = pool.length > 0 ? pool : catalog;
      if (source.length === 0) return;
      const randomItem = source[Math.floor(Math.random() * source.length)];
      openQuickPeek(randomItem);
    }

    // Event Listeners
    searchInput?.addEventListener("input", (e) => {
      state.query = e.target.value;
      renderGrid(false);
    });

    sortSelect?.addEventListener("change", (e) => {
      state.sort = e.target.value;
      renderGrid(true);
    });

    gridBtn?.addEventListener("click", () => {
      if (state.view === "grid") return;
      state.view = "grid";
      gridBtn.classList.add("is-active");
      gridBtn.setAttribute("aria-pressed", "true");
      listBtn.classList.remove("is-active");
      listBtn.setAttribute("aria-pressed", "false");
      renderGrid(true);
    });

    listBtn?.addEventListener("click", () => {
      if (state.view === "list") return;
      state.view = "list";
      listBtn.classList.add("is-active");
      listBtn.setAttribute("aria-pressed", "true");
      gridBtn.classList.remove("is-active");
      gridBtn.setAttribute("aria-pressed", "false");
      renderGrid(true);
    });

    categoryPillsEl?.addEventListener("click", (e) => {
      const pill = e.target.closest(".category-pill");
      if (!pill) return;
      state.category = pill.dataset.category || "All";
      renderCategoryPills();
      renderGrid(true);
    });

    resetFiltersBtn?.addEventListener("click", () => {
      state.query = "";
      state.category = "All";
      if (searchInput) searchInput.value = "";
      renderCategoryPills();
      renderGrid(true);
      searchInput?.focus();
    });

    gridEl?.addEventListener("click", (e) => {
      const peekBtn = e.target.closest(".quick-peek-btn");
      if (peekBtn) {
        const url = peekBtn.dataset.url;
        const item = catalog.find((p) => p.url === url) || {
          name: peekBtn.dataset.name,
          category: peekBtn.dataset.category,
          url,
          summary: "",
        };
        openQuickPeek(item);
        return;
      }

      const tagBtn = e.target.closest(".card-tag");
      if (tagBtn) {
        const tag = tagBtn.dataset.tag || "";
        state.query = tag;
        if (searchInput) searchInput.value = tag;
        renderGrid(true);
        return;
      }

      if (e.target.id === "empty-reset-btn") {
        resetFiltersBtn?.click();
      }
    });

    async function triggerSync() {
      if (!syncBtn || syncBtn.disabled) return;
      const labelEl = syncBtn.querySelector(".sync-label");
      syncBtn.disabled = true;
      syncBtn.classList.add("is-syncing");
      syncBtn.setAttribute("aria-busy", "true");
      if (labelEl) labelEl.textContent = "Syncing...";
      resultsMetaEl.textContent = "Scanning projects directory and rebuilding catalog...";

      let addedCount = 0;
      const endpoints = [
        `http://${window.location.hostname || "127.0.0.1"}:8765/sync`,
        "http://127.0.0.1:8765/sync",
      ];

      for (const endpoint of endpoints) {
        try {
          const res = await fetch(endpoint, { method: "POST" });
          if (res.ok) {
            const data = await res.json();
            addedCount = data.added_count || 0;
            break;
          }
        } catch (_err) {
          // Try next endpoint or fall back to refreshing projects.json directly
        }
      }

      try {
        pageCache.clear();
        catalog = await loadCatalog(true);
        rebuildCategoryCounts();
        renderCategoryPills();
        renderGrid(true);
        resultsMetaEl.textContent =
          addedCount > 0
            ? `Synced ${catalog.length} projects (+${addedCount} new)`
            : `Synced — all ${catalog.length} projects up to date`;
      } finally {
        syncBtn.disabled = false;
        syncBtn.classList.remove("is-syncing");
        syncBtn.setAttribute("aria-busy", "false");
        if (labelEl) labelEl.textContent = "Sync";
      }
    }

    // Boot at startup service status & toggle
    async function refreshBootStatus() {
      if (!bootBtn) return;
      try {
        const res = await fetch(`${apiBase}/service-status`, { cache: "no-store" });
        if (!res.ok) return;
        const data = await res.json();
        const enabled = Boolean(data.enabled);
        bootBtn.setAttribute("aria-pressed", String(enabled));
        const lbl = bootBtn.querySelector(".boot-label");
        if (lbl) lbl.textContent = enabled ? "Boot: ON" : "Boot: OFF";
      } catch (_err) {
        // API not reachable
      }
    }

    bootBtn?.addEventListener("click", async () => {
      try {
        const res = await fetch(`${apiBase}/service-toggle`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({}),
        });
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        const data = await res.json();
        const enabled = Boolean(data.enabled);
        bootBtn.setAttribute("aria-pressed", String(enabled));
        const lbl = bootBtn.querySelector(".boot-label");
        if (lbl) lbl.textContent = enabled ? "Boot: ON" : "Boot: OFF";
        resultsMetaEl.textContent = enabled
          ? "Enabled automatic Start at Boot (systemd user service)"
          : "Disabled automatic Start at Boot";
      } catch (_err) {
        resultsMetaEl.textContent = "Note: Run 'wiki' in terminal to start the local control API first.";
      }
    });

    // In-Modal Project Tune / Edit Drawer
    async function openTuneDrawer() {
      if (!currentModalItem || !tuneDrawer) return;
      const isHidden = tuneDrawer.hidden;
      if (!isHidden) {
        tuneDrawer.hidden = true;
        modalTuneBtn?.setAttribute("aria-expanded", "false");
        return;
      }

      tuneDrawer.hidden = false;
      modalTuneBtn?.setAttribute("aria-expanded", "true");
      if (tunePromptInput) tunePromptInput.value = "";
      if (tuneStatusMsg) tuneStatusMsg.textContent = "Loading project metadata...";

      // Pre-fill from local catalog first
      if (tuneTitleInput) tuneTitleInput.value = currentModalItem.title || currentModalItem.name;
      if (tuneCategorySelect) tuneCategorySelect.value = currentModalItem.category || "Hardware & Mods";
      if (tuneOverviewInput) tuneOverviewInput.value = currentModalItem.summary || "";

      try {
        const res = await fetch(`${apiBase}/project-meta?name=${encodeURIComponent(currentModalItem.name)}`, {
          cache: "no-store",
        });
        if (res.ok) {
          const meta = await res.json();
          if (tuneTitleInput && meta.title) tuneTitleInput.value = meta.title;
          if (tuneCategorySelect && meta.category) tuneCategorySelect.value = meta.category;
          if (tuneOverviewInput && meta.overview) tuneOverviewInput.value = meta.overview;
        }
        if (tuneStatusMsg) tuneStatusMsg.textContent = "";
      } catch (_err) {
        if (tuneStatusMsg) tuneStatusMsg.textContent = "Editing locally (ensure wiki API is running to save).";
      }
      tunePromptInput?.focus();
    }

    async function submitTune(mode) {
      if (!currentModalItem) return;
      const promptText = tunePromptInput?.value.trim() || "";
      if (mode === "ai" && !promptText) {
        tunePromptInput?.focus();
        if (tuneStatusMsg) tuneStatusMsg.textContent = "Enter a short correction note first.";
        return;
      }

      if (tuneAiBtn) tuneAiBtn.disabled = true;
      if (tuneSaveBtn) tuneSaveBtn.disabled = true;
      if (tuneStatusMsg) {
        tuneStatusMsg.textContent =
          mode === "ai" ? "Tuning description with AI..." : "Saving changes...";
      }

      try {
        const res = await fetch(`${apiBase}/tune`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            name: currentModalItem.name,
            mode,
            prompt: promptText,
            title: tuneTitleInput?.value || currentModalItem.name,
            category: tuneCategorySelect?.value || currentModalItem.category,
            overview: tuneOverviewInput?.value || "",
          }),
        });
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        const updated = await res.json();

        // Update form fields with tuned result
        if (tuneTitleInput) tuneTitleInput.value = updated.title;
        if (tuneCategorySelect) tuneCategorySelect.value = updated.category;
        if (tuneOverviewInput) tuneOverviewInput.value = updated.overview;
        if (tunePromptInput) tunePromptInput.value = "";

        // Update modal live DOM immediately
        modalCategory.textContent = updated.category;
        const h1 = modalBody.querySelector("h1");
        if (h1) h1.textContent = updated.title;

        // Replace overview paragraphs between Overview H2 and Directory Summary H2
        const headings = Array.from(modalBody.querySelectorAll("h2"));
        const ovHeading = headings.find((h) => /overview/i.test(h.textContent));
        if (ovHeading) {
          let next = ovHeading.nextElementSibling;
          while (next && next.tagName !== "H2" && next.tagName !== "HR") {
            const toRemove = next;
            next = next.nextElementSibling;
            toRemove.remove();
          }
          const p = document.createElement("p");
          p.textContent = updated.overview;
          ovHeading.insertAdjacentElement("afterend", p);
        }

        // Clear cached page and refresh catalog & cards
        pageCache.delete(currentModalItem.url);
        catalog = await loadCatalog(true);
        currentModalItem = catalog.find((p) => p.name === currentModalItem.name) || currentModalItem;
        rebuildCategoryCounts();
        renderCategoryPills();
        renderGrid(true);

        if (tuneStatusMsg) tuneStatusMsg.textContent = "Saved & updated!";
        setTimeout(() => {
          if (tuneDrawer) tuneDrawer.hidden = true;
          modalTuneBtn?.setAttribute("aria-expanded", "false");
        }, 700);
      } catch (_err) {
        if (tuneStatusMsg) tuneStatusMsg.textContent = "Could not reach local API on port 8765.";
      } finally {
        if (tuneAiBtn) tuneAiBtn.disabled = false;
        if (tuneSaveBtn) tuneSaveBtn.disabled = false;
      }
    }

    modalTuneBtn?.addEventListener("click", openTuneDrawer);
    tuneCancelBtn?.addEventListener("click", () => {
      if (tuneDrawer) tuneDrawer.hidden = true;
      modalTuneBtn?.setAttribute("aria-expanded", "false");
    });
    tuneAiBtn?.addEventListener("click", () => submitTune("ai"));
    tunePromptInput?.addEventListener("keydown", (e) => {
      if (e.key === "Enter") {
        e.preventDefault();
        submitTune("ai");
      }
    });
    tuneSaveBtn?.addEventListener("click", () => submitTune("manual"));

    syncBtn?.addEventListener("click", triggerSync);
    randomPeekBtn?.addEventListener("click", openRandomProject);
    modalRandomBtn?.addEventListener("click", openRandomProject);
    refreshBootStatus();

    // Dismiss modal when clicking on the backdrop outside the dialog box
    dialog?.addEventListener("click", (e) => {
      if (e.target === dialog) {
        dialog.close();
      }
    });

    // Global '/' shortcut to focus the explorer filter input
    document.addEventListener("keydown", (e) => {
      if (
        e.key === "/" &&
        document.activeElement !== searchInput &&
        !["INPUT", "TEXTAREA", "SELECT"].includes(document.activeElement?.tagName || "") &&
        !dialog?.open
      ) {
        const activeSearch = document.getElementById("project-search");
        if (activeSearch) {
          e.preventDefault();
          activeSearch.focus();
          activeSearch.select();
        }
      }
    });

    renderCategoryPills();
    renderGrid(false);
  }

  // Support both standard DOMContentLoaded and MkDocs Material instant AJAX navigation (document$)
  if (typeof window.document$ !== "undefined" && typeof window.document$.subscribe === "function") {
    window.document$.subscribe(() => {
      initExplorer();
    });
  } else {
    document.addEventListener("DOMContentLoaded", initExplorer);
  }
})();


/* ==========================================================================
   BenchBear Live Inventory Integration on Explorer & Inventory/BOM Pages
   ========================================================================== */
(function () {
  "use strict";

  const invApi = `http://${window.location.hostname || "127.0.0.1"}:8090`;
  let uiInitialized = false;
  let cachedLocations = [];
  let cachedSuppliers = [];

  function esc(str) {
    return String(str ?? "")
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;");
  }

  function showToast(msg, isErr) {
    let el = document.getElementById("bb-global-toast");
    if (!el) {
      el = document.createElement("div");
      el.id = "bb-global-toast";
      el.className = "bb-toast";
      document.body.appendChild(el);
    }
    el.textContent = msg;
    el.style.borderColor = isErr ? "#f87171" : "#e06338";
    el.style.display = "block";
    clearTimeout(el._t);
    el._t = setTimeout(() => {
      el.style.display = "none";
    }, 3600);
  }

  async function invFetch(path, opts = {}) {
    const res = await fetch(invApi + path, opts);
    const ct = res.headers.get("content-type") || "";
    if (!res.ok) {
      let msg = `HTTP ${res.status}`;
      if (ct.includes("json")) {
        const j = await res.json();
        msg = j.error || msg;
      }
      throw new Error(msg);
    }
    return ct.includes("json") ? res.json() : res;
  }

  const bbImgState = {
    selectedImgUrl: "",
    selectedPinoutUrl: "",
    clearPartImg: false,
    clearPinoutImg: false,
    pendingPartFile: null,
    pendingPartObjectUrl: "",
    pendingPinoutFile: null,
    pendingPinoutObjectUrl: "",
    existingPartPath: "",
    existingPinoutPath: "",
    galleryImages: [],
  };

  function openBbLightbox(src, title) {
    ensureModals();
    if (!src) return;
    document.getElementById("bb-lb-title").textContent = title || "Image Preview";
    document.getElementById("bb-lb-img").src = src;
    document.getElementById("bb-lb-open").href = src;
    document.getElementById("bb-img-lightbox").showModal();
  }

  function renderBbImageSlots() {
    const partBox = document.getElementById("bb-pf-part-preview");
    const partCap = document.getElementById("bb-pf-part-cap");
    let partSrc = "";
    if (bbImgState.pendingPartObjectUrl) {
      partSrc = bbImgState.pendingPartObjectUrl;
      partCap.textContent = "Local file ready";
    } else if (bbImgState.selectedImgUrl) {
      partSrc = `${invApi}/api/proxy-image?url=${encodeURIComponent(bbImgState.selectedImgUrl)}`;
      partCap.textContent = "Downloads on Save";
    } else if (bbImgState.existingPartPath && !bbImgState.clearPartImg) {
      partSrc = `${invApi}/api/images/${encodeURIComponent(bbImgState.existingPartPath)}?t=${Date.now()}`;
      partCap.textContent = "Saved locally";
    } else {
      partCap.textContent = bbImgState.clearPartImg ? "Will clear" : "";
    }
    partBox.innerHTML = partSrc
      ? `<img src="${esc(partSrc)}" alt="Part Photo" />`
      : "No photo selected";
    const pImg = partBox.querySelector("img");
    if (pImg) pImg.addEventListener("click", () => openBbLightbox(partSrc, "Part Photo Preview"));

    const pinBox = document.getElementById("bb-pf-pinout-preview");
    const pinCap = document.getElementById("bb-pf-pinout-cap");
    let pinSrc = "";
    if (bbImgState.pendingPinoutObjectUrl) {
      pinSrc = bbImgState.pendingPinoutObjectUrl;
      pinCap.textContent = "Local file ready";
    } else if (bbImgState.selectedPinoutUrl) {
      pinSrc = `${invApi}/api/proxy-image?url=${encodeURIComponent(bbImgState.selectedPinoutUrl)}`;
      pinCap.textContent = "Downloads on Save";
    } else if (bbImgState.existingPinoutPath && !bbImgState.clearPinoutImg) {
      pinSrc = `${invApi}/api/images/${encodeURIComponent(bbImgState.existingPinoutPath)}?t=${Date.now()}`;
      pinCap.textContent = "Saved locally";
    } else {
      pinCap.textContent = bbImgState.clearPinoutImg ? "Will clear" : "";
    }
    pinBox.innerHTML = pinSrc
      ? `<img src="${esc(pinSrc)}" alt="Pinout Diagram" />`
      : "No pinout selected";
    const pinImg = pinBox.querySelector("img");
    if (pinImg) pinImg.addEventListener("click", () => openBbLightbox(pinSrc, "Pinout / Wiring Diagram Preview"));
  }

  function renderBbGalleryStrip() {
    const box = document.getElementById("bb-pf-gallery-box");
    const strip = document.getElementById("bb-pf-gallery-strip");
    const count = document.getElementById("bb-pf-gallery-count");
    if (!box || !strip) return;
    const imgs = bbImgState.galleryImages || [];
    if (!imgs.length) {
      box.style.display = "none";
      strip.innerHTML = "";
      return;
    }
    box.style.display = "block";
    count.textContent = `${imgs.length} images`;
    strip.innerHTML = imgs.map((item, idx) => {
      const proxied = `${invApi}/api/proxy-image?url=${encodeURIComponent(item.url)}`;
      const isPhoto = bbImgState.selectedImgUrl === item.url;
      const isPinout = bbImgState.selectedPinoutUrl === item.url;
      const cls = ["bb-gallery-card", isPhoto ? "is-photo" : "", isPinout ? "is-pinout" : ""].filter(Boolean).join(" ");
      const badge = item.likely_pinout ? `<span class="bb-pinout-badge">Possible Pinout</span>` : "";
      return `<div class="${cls}" data-idx="${idx}">
        <img src="${esc(proxied)}" alt="${esc(item.label || ('Image ' + (idx + 1)))}" loading="lazy" />
        ${badge}
        <div style="display:flex;gap:0.25rem;margin-top:auto">
          <button type="button" class="bb-btn bb-btn--sm" style="flex:1" data-pick="part">${isPhoto ? "Photo *" : "Photo"}</button>
          <button type="button" class="bb-btn bb-btn--sm" style="flex:1" data-pick="pinout">${isPinout ? "Pinout *" : "Pinout"}</button>
        </div>
      </div>`;
    }).join("");
    strip.querySelectorAll(".bb-gallery-card").forEach((card) => {
      const idx = parseInt(card.dataset.idx, 10);
      const item = imgs[idx];
      if (!item) return;
      const proxied = `${invApi}/api/proxy-image?url=${encodeURIComponent(item.url)}`;
      card.querySelector("img")?.addEventListener("click", () => openBbLightbox(proxied, item.label || `Candidate Image ${idx + 1}`));
      card.querySelector('[data-pick="part"]')?.addEventListener("click", () => {
        bbImgState.selectedImgUrl = item.url;
        bbImgState.clearPartImg = false;
        bbImgState.pendingPartFile = null;
        bbImgState.pendingPartObjectUrl = "";
        renderBbImageSlots();
        renderBbGalleryStrip();
      });
      card.querySelector('[data-pick="pinout"]')?.addEventListener("click", () => {
        bbImgState.selectedPinoutUrl = item.url;
        bbImgState.clearPinoutImg = false;
        bbImgState.pendingPinoutFile = null;
        bbImgState.pendingPinoutObjectUrl = "";
        renderBbImageSlots();
        renderBbGalleryStrip();
      });
    });
  }

  function ensureModals() {
    if (uiInitialized) return;
    uiInitialized = true;

    const wrap = document.createElement("div");
    wrap.innerHTML = `
      <datalist id="bb-loc-list"></datalist>
      <datalist id="bb-sup-list"></datalist>
      <dialog id="bb-part-dialog" class="bb-modal">
        <h3 id="bb-part-modal-title">Add Part</h3>
        <div style="background:#171a21;border:1px solid rgba(140,145,155,0.22);border-radius:8px;padding:0.65rem 0.8rem;margin-bottom:0.75rem">
          <label style="display:block;font-size:0.78rem;font-weight:600;color:#9aa0aa;margin-bottom:0.3rem">Import from Product URL (Amazon, Adafruit, SparkFun, LCSC, DigiKey, etc.)</label>
          <div style="display:flex;gap:0.5rem;align-items:center">
            <input id="bb-pf-import-url" type="url" placeholder="https://www.amazon.com/.../dp/... or store link" style="flex:1;padding:0.45rem 0.65rem;border-radius:6px;border:1px solid rgba(140,145,155,0.24);background:#26282c;color:#e8e6e3;font-size:0.9rem" />
            <button type="button" id="bb-pf-import-btn" class="bb-btn bb-btn--primary">Fetch Info</button>
          </div>
          <div id="bb-pf-import-status" style="font-size:0.8rem;color:#9aa0aa;margin-top:0.3rem"></div>
          <div class="bb-cost-calc">
            <div class="bb-cost-calc__field">
              <label>Packs</label>
              <input id="bb-pf-packs" type="number" min="1" value="1" style="width:68px" />
            </div>
            <div class="bb-cost-calc__field">
              <label>Final Qty</label>
              <input id="bb-pf-final-qty" type="number" min="1" placeholder="e.g. 6" style="width:92px" />
            </div>
            <div class="bb-cost-calc__field">
              <label>Final Cost ($ incl. tax)</label>
              <input id="bb-pf-final-cost" type="number" step="0.01" min="0" placeholder="e.g. 40.25" style="width:120px" />
            </div>
            <span id="bb-pf-calc-summary" class="bb-cost-calc__summary"></span>
          </div>
        </div>
        <form id="bb-part-form">
          <div class="bb-form-grid">
            <div class="bb-field"><label>SKU *</label><input id="bb-pf-sku" required placeholder="e.g. MCU-003" /></div>
            <div class="bb-field"><label>Category *</label><input id="bb-pf-cat" value="microcontrollers" required /></div>
            <div class="bb-field"><label>Part Name *</label><input id="bb-pf-name" required /></div>
            <div class="bb-field"><label>Value / Spec</label><input id="bb-pf-spec" /></div>
            <div class="bb-field"><label>Package</label><input id="bb-pf-pkg" /></div>
            <div class="bb-field"><label>Quantity</label><input id="bb-pf-qty" type="number" min="0" value="1" /></div>
            <div class="bb-field"><label>Min Stock</label><input id="bb-pf-min" type="number" min="0" value="1" /></div>
            <div class="bb-field"><label>Unit Cost ($)</label><input id="bb-pf-cost" type="number" step="0.01" min="0" value="0.00" /></div>
            <div class="bb-field"><label>Location / Bin</label><input id="bb-pf-loc" list="bb-loc-list" placeholder="e.g. BIN-A1" /></div>
            <div class="bb-field"><label>Primary Supplier</label><input id="bb-pf-sup" list="bb-sup-list" placeholder="e.g. DigiKey" /></div>
            <div class="bb-field"><label>MPN</label><input id="bb-pf-mpn" /></div>
            <div class="bb-field"><label>Supplier SKU</label><input id="bb-pf-ssku" /></div>
            <div class="bb-field"><label>Product URL</label><input id="bb-pf-url" /></div>
            <div class="bb-field"><label>Projects</label><input id="bb-pf-proj" value="-" /></div>
          </div>
          <div class="bb-field"><label>Description (scraped / searchable)</label><textarea id="bb-pf-desc" rows="3" placeholder="Key specs, features, interfaces..."></textarea></div>
          <div class="bb-field"><label>Notes</label><textarea id="bb-pf-notes" rows="2"></textarea></div>
          <div class="bb-img-slots">
            <div class="bb-img-slot">
              <div class="bb-img-slot__head">
                <span>Part Photo</span>
                <button type="button" class="bb-btn bb-btn--sm" id="bb-pf-part-clear">Clear</button>
              </div>
              <div class="bb-img-preview" id="bb-pf-part-preview">No photo selected</div>
              <div class="bb-img-slot__actions">
                <label class="bb-btn bb-btn--sm" style="cursor:pointer">
                  Upload File<input type="file" id="bb-pf-part-file" accept="image/*" style="display:none" />
                </label>
                <span id="bb-pf-part-cap"></span>
              </div>
            </div>
            <div class="bb-img-slot">
              <div class="bb-img-slot__head">
                <span>Pinout / Wiring Diagram</span>
                <button type="button" class="bb-btn bb-btn--sm" id="bb-pf-pinout-clear">Clear</button>
              </div>
              <div class="bb-img-preview" id="bb-pf-pinout-preview">No pinout selected</div>
              <div class="bb-img-slot__actions">
                <label class="bb-btn bb-btn--sm" style="cursor:pointer">
                  Upload File<input type="file" id="bb-pf-pinout-file" accept="image/*" style="display:none" />
                </label>
                <span id="bb-pf-pinout-cap"></span>
              </div>
            </div>
          </div>
          <div class="bb-gallery-box" id="bb-pf-gallery-box" style="display:none">
            <div class="bb-img-slot__head">
              <span>Product Gallery Candidates (click image to zoom and read pins)</span>
              <span id="bb-pf-gallery-count"></span>
            </div>
            <div class="bb-gallery-strip" id="bb-pf-gallery-strip"></div>
          </div>
          <div class="bb-modal__footer">
            <button type="button" class="bb-btn" onclick="document.getElementById('bb-part-dialog').close()">Cancel</button>
            <button type="submit" class="bb-btn bb-btn--primary">Save Part</button>
          </div>
        </form>
      </dialog>

      <dialog id="bb-loc-dialog" class="bb-modal">
        <h3 id="bb-loc-modal-title">Add Location / Bin</h3>
        <form id="bb-loc-form">
          <div class="bb-form-grid">
            <div class="bb-field"><label>Code *</label><input id="bb-lf-code" required placeholder="BIN-A1" /></div>
            <div class="bb-field"><label>Name</label><input id="bb-lf-name" placeholder="Bin A1" /></div>
            <div class="bb-field"><label>Area</label><input id="bb-lf-area" placeholder="Parts Rack" /></div>
            <div class="bb-field"><label>Rack</label><input id="bb-lf-rack" /></div>
            <div class="bb-field"><label>Drawer</label><input id="bb-lf-drawer" /></div>
            <div class="bb-field"><label>Barcode</label><input id="bb-lf-barcode" placeholder="Auto BIN:<CODE>" /></div>
          </div>
          <div class="bb-field"><label>Description</label><textarea id="bb-lf-desc" rows="2"></textarea></div>
          <div class="bb-modal__footer">
            <button type="button" class="bb-btn" onclick="document.getElementById('bb-loc-dialog').close()">Cancel</button>
            <button type="submit" class="bb-btn bb-btn--primary">Save Location</button>
          </div>
        </form>
      </dialog>

      <dialog id="bb-sup-dialog" class="bb-modal">
        <h3 id="bb-sup-modal-title">Add Supplier</h3>
        <form id="bb-sup-form">
          <input type="hidden" id="bb-sf-id" />
          <div class="bb-form-grid">
            <div class="bb-field"><label>Name *</label><input id="bb-sf-name" required /></div>
            <div class="bb-field"><label>Website</label><input id="bb-sf-web" placeholder="https://..." /></div>
            <div class="bb-field"><label>Currency</label><input id="bb-sf-curr" value="USD" /></div>
            <div class="bb-field"><label>Lead Time (days)</label><input id="bb-sf-lead" type="number" min="0" value="3" /></div>
          </div>
          <div class="bb-field"><label>Shipping Notes</label><input id="bb-sf-ship" /></div>
          <div class="bb-modal__footer">
            <button type="button" class="bb-btn" onclick="document.getElementById('bb-sup-dialog').close()">Cancel</button>
            <button type="submit" class="bb-btn bb-btn--primary">Save Supplier</button>
          </div>
        </form>
      </dialog>

      <dialog id="bb-scan-dialog" class="bb-modal">
        <h3>Quick Barcode Lookup &amp; Stock Scan</h3>
        <form id="bb-scan-form">
          <div class="bb-form-grid">
            <div class="bb-field">
              <label>Mode</label>
              <select id="bb-scan-mode">
                <option value="lookup">Lookup</option>
                <option value="checkin">Check-In (+1)</option>
                <option value="checkout">Check-Out (-1)</option>
              </select>
            </div>
            <div class="bb-field">
              <label>Scan or Type SKU / BIN / MPN</label>
              <input id="bb-scan-input" placeholder="e.g. MCU-001 or BIN:BIN-A1" required />
            </div>
          </div>
          <div id="bb-scan-result" style="font-size:0.9rem;color:#c2c6d0;margin-top:0.4rem"></div>
          <div class="bb-modal__footer">
            <button type="button" class="bb-btn" onclick="document.getElementById('bb-scan-dialog').close()">Close</button>
            <button type="submit" class="bb-btn bb-btn--primary">Run</button>
          </div>
        </form>
      </dialog>

      <dialog id="bb-img-lightbox" class="bb-modal bb-modal--lightbox">
        <h3 id="bb-lb-title">Image Preview</h3>
        <div style="text-align:center">
          <img id="bb-lb-img" alt="Preview" />
        </div>
        <div class="bb-modal__footer">
          <a id="bb-lb-open" class="bb-btn" target="_blank" rel="noopener">Open Full Image</a>
          <button type="button" class="bb-btn bb-btn--primary" onclick="document.getElementById('bb-img-lightbox').close()">Close</button>
        </div>
      </dialog>

      <dialog id="bb-update-dialog" class="bb-modal" style="max-width:620px">
        <h3>BenchBear Updates</h3>
        <div id="bb-update-body" style="font-size:0.9rem;color:#c2c6d0">Checking GitHub for updates...</div>
        <div class="bb-modal__footer">
          <button type="button" id="bb-update-recheck" class="bb-btn">Re-check Now</button>
          <a id="bb-update-link" class="bb-btn" href="https://github.com/morganlowe/BenchBear/releases" target="_blank" rel="noopener">Open GitHub Releases</a>
          <button type="button" class="bb-btn bb-btn--primary" onclick="document.getElementById('bb-update-dialog').close()">Close</button>
        </div>
      </dialog>

      <dialog id="bb-part-detail-dialog" class="bb-modal" style="max-width:840px">
        <h3 id="bb-detail-title">Part Details</h3>
        <div id="bb-detail-body" style="font-size:0.9rem;color:#c2c6d0">Loading...</div>
        <div class="bb-modal__footer" id="bb-detail-footer">
          <button type="button" class="bb-btn" onclick="document.getElementById('bb-part-detail-dialog').close()">Close</button>
        </div>
      </dialog>
    `;
    document.body.appendChild(wrap);
    document.getElementById("bb-update-recheck").addEventListener("click", () => openUpdateModal(true));

    document.getElementById("bb-pf-part-file").addEventListener("change", (e) => {
      const f = e.target.files && e.target.files[0];
      if (!f) return;
      bbImgState.pendingPartFile = f;
      bbImgState.pendingPartObjectUrl = URL.createObjectURL(f);
      bbImgState.selectedImgUrl = "";
      bbImgState.clearPartImg = false;
      renderBbImageSlots();
      renderBbGalleryStrip();
    });
    document.getElementById("bb-pf-pinout-file").addEventListener("change", (e) => {
      const f = e.target.files && e.target.files[0];
      if (!f) return;
      bbImgState.pendingPinoutFile = f;
      bbImgState.pendingPinoutObjectUrl = URL.createObjectURL(f);
      bbImgState.selectedPinoutUrl = "";
      bbImgState.clearPinoutImg = false;
      renderBbImageSlots();
      renderBbGalleryStrip();
    });
    document.getElementById("bb-pf-part-clear").addEventListener("click", () => {
      bbImgState.selectedImgUrl = "";
      bbImgState.pendingPartFile = null;
      bbImgState.pendingPartObjectUrl = "";
      bbImgState.clearPartImg = !!bbImgState.existingPartPath;
      document.getElementById("bb-pf-part-file").value = "";
      renderBbImageSlots();
      renderBbGalleryStrip();
    });
    document.getElementById("bb-pf-pinout-clear").addEventListener("click", () => {
      bbImgState.selectedPinoutUrl = "";
      bbImgState.pendingPinoutFile = null;
      bbImgState.pendingPinoutObjectUrl = "";
      bbImgState.clearPinoutImg = !!bbImgState.existingPinoutPath;
      document.getElementById("bb-pf-pinout-file").value = "";
      renderBbImageSlots();
      renderBbGalleryStrip();
    });

    document.getElementById("bb-pf-packs").addEventListener("input", () => {
      const packs = Math.max(1, parseInt(document.getElementById("bb-pf-packs").value || "1", 10));
      document.getElementById("bb-pf-final-qty").value = String(packs * bbPackCalc.basePackQty);
      if (bbPackCalc.basePackPrice > 0) {
        document.getElementById("bb-pf-final-cost").value = (packs * bbPackCalc.basePackPrice).toFixed(2);
      }
      recalcBbUnitCost();
    });
    document.getElementById("bb-pf-final-qty").addEventListener("input", recalcBbUnitCost);
    document.getElementById("bb-pf-final-cost").addEventListener("input", recalcBbUnitCost);

    document.getElementById("bb-pf-import-btn").addEventListener("click", async () => {
      const urlEl = document.getElementById("bb-pf-import-url");
      const btn = document.getElementById("bb-pf-import-btn");
      const st = document.getElementById("bb-pf-import-status");
      const url = urlEl.value.trim();
      if (!url) {
        st.textContent = "Paste a product URL first.";
        return;
      }
      btn.disabled = true;
      st.textContent = "Fetching product metadata...";
      try {
        const d = await invFetch("/api/parts/fetch-url", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ url }),
        });
        const skuEl = document.getElementById("bb-pf-sku");
        if (!skuEl.readOnly && !skuEl.value.trim() && d.suggested_sku) skuEl.value = d.suggested_sku;
        if (d.category) document.getElementById("bb-pf-cat").value = d.category;
        if (d.name) document.getElementById("bb-pf-name").value = d.name;
        if (d.spec) document.getElementById("bb-pf-spec").value = d.spec;
        if (d.package) document.getElementById("bb-pf-pkg").value = d.package;
        if (d.pack_qty > 1) document.getElementById("bb-pf-qty").value = d.pack_qty;
        if (d.unit_cost > 0) document.getElementById("bb-pf-cost").value = Number(d.unit_cost).toFixed(2);
        if (d.supplier_name) document.getElementById("bb-pf-sup").value = d.supplier_name;
        if (d.mpn) document.getElementById("bb-pf-mpn").value = d.mpn;
        if (d.supplier_sku) document.getElementById("bb-pf-ssku").value = d.supplier_sku;
        if (d.url) document.getElementById("bb-pf-url").value = d.url;
        if (d.description) document.getElementById("bb-pf-desc").value = d.description;
        const packQty = Math.max(1, parseInt(d.pack_qty || 1, 10));
        const totalPrice = Number(d.total_price || 0) > 0
          ? Number(d.total_price)
          : (Number(d.unit_cost || 0) > 0 ? Number(d.unit_cost) * packQty : 0);
        bbPackCalc.basePackQty = packQty;
        bbPackCalc.basePackPrice = totalPrice;
        document.getElementById("bb-pf-packs").value = "1";
        document.getElementById("bb-pf-final-qty").value = String(packQty);
        document.getElementById("bb-pf-qty").value = String(packQty);
        if (totalPrice > 0) {
          document.getElementById("bb-pf-final-cost").value = totalPrice.toFixed(2);
          document.getElementById("bb-pf-cost").value = (totalPrice / packQty).toFixed(2);
        } else {
          document.getElementById("bb-pf-final-cost").value = "";
        }
        updateBbCalcSummary();
        bbImgState.galleryImages = Array.isArray(d.images) ? d.images : [];
        if (!bbImgState.selectedImgUrl && !bbImgState.existingPartPath && !bbImgState.pendingPartFile && bbImgState.galleryImages.length) {
          const firstPhoto = bbImgState.galleryImages.find((x) => !x.likely_pinout) || bbImgState.galleryImages[0];
          if (firstPhoto) bbImgState.selectedImgUrl = firstPhoto.url;
        }
        if (!bbImgState.selectedPinoutUrl && !bbImgState.existingPinoutPath && !bbImgState.pendingPinoutFile) {
          const firstPinout = bbImgState.galleryImages.find((x) => x.likely_pinout);
          if (firstPinout) bbImgState.selectedPinoutUrl = firstPinout.url;
        }
        renderBbImageSlots();
        renderBbGalleryStrip();
        st.textContent = d.warning || `Imported from ${d.supplier_name || "store"}${d.unit_cost > 0 ? " ($" + Number(d.unit_cost).toFixed(2) + ")" : ""}.`;
        st.style.color = d.blocked ? "#fbbf24" : "#34d399";
      } catch (err) {
        st.textContent = `Fetch failed: ${err.message}`;
        st.style.color = "#f87171";
      } finally {
        btn.disabled = false;
      }
    });

    document.getElementById("bb-part-form").addEventListener("submit", async (e) => {
      e.preventDefault();
      const payload = {
        sku: document.getElementById("bb-pf-sku").value.trim(),
        category: document.getElementById("bb-pf-cat").value.trim() || "hardware",
        name: document.getElementById("bb-pf-name").value.trim(),
        spec: document.getElementById("bb-pf-spec").value.trim(),
        package: document.getElementById("bb-pf-pkg").value.trim(),
        qty: parseInt(document.getElementById("bb-pf-qty").value || "0", 10),
        min_qty: parseInt(document.getElementById("bb-pf-min").value || "1", 10),
        unit_cost: parseFloat(document.getElementById("bb-pf-cost").value || "0"),
        location_code: document.getElementById("bb-pf-loc").value.trim() || null,
        supplier_name: document.getElementById("bb-pf-sup").value.trim() || null,
        mpn: document.getElementById("bb-pf-mpn").value.trim(),
        supplier_sku: document.getElementById("bb-pf-ssku").value.trim(),
        product_url: document.getElementById("bb-pf-url").value.trim(),
        projects_tags: document.getElementById("bb-pf-proj").value.trim() || "-",
        description: document.getElementById("bb-pf-desc").value.trim(),
        notes: document.getElementById("bb-pf-notes").value.trim(),
        image_path: bbImgState.clearPartImg ? "__CLEAR__" : "",
        pinout_image_path: bbImgState.clearPinoutImg ? "__CLEAR__" : "",
        image_url: bbImgState.pendingPartFile ? "" : (bbImgState.selectedImgUrl || ""),
        pinout_image_url: bbImgState.pendingPinoutFile ? "" : (bbImgState.selectedPinoutUrl || ""),
      };
      try {
        await invFetch("/api/parts", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(payload),
        });
        for (const [kind, file] of [["part", bbImgState.pendingPartFile], ["pinout", bbImgState.pendingPinoutFile]]) {
          if (!file) continue;
          const fd = new FormData();
          fd.append("kind", kind);
          fd.append("file", file);
          const upRes = await fetch(`${invApi}/api/parts/${encodeURIComponent(payload.sku)}/image`, { method: "POST", body: fd });
          if (!upRes.ok) throw new Error((await upRes.text()) || `Upload failed (${kind})`);
        }
        document.getElementById("bb-part-dialog").close();
        showToast(`Saved ${payload.sku}`);
        if (window.location.pathname.includes("/inventory/")) {
          setTimeout(() => window.location.reload(), 650);
        } else {
          refreshExplorerStrip();
        }
      } catch (err) {
        showToast(err.message, true);
      }
    });

    document.getElementById("bb-loc-form").addEventListener("submit", async (e) => {
      e.preventDefault();
      const payload = {
        code: document.getElementById("bb-lf-code").value.trim(),
        name: document.getElementById("bb-lf-name").value.trim(),
        area: document.getElementById("bb-lf-area").value.trim(),
        rack: document.getElementById("bb-lf-rack").value.trim(),
        drawer: document.getElementById("bb-lf-drawer").value.trim(),
        barcode_value: document.getElementById("bb-lf-barcode").value.trim(),
        description: document.getElementById("bb-lf-desc").value.trim(),
      };
      try {
        await invFetch("/api/locations", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(payload),
        });
        document.getElementById("bb-loc-dialog").close();
        showToast(`Saved location ${payload.code}`);
        if (window.location.pathname.includes("/inventory/")) {
          setTimeout(() => window.location.reload(), 650);
        } else {
          refreshExplorerStrip();
        }
      } catch (err) {
        showToast(err.message, true);
      }
    });

    document.getElementById("bb-sup-form").addEventListener("submit", async (e) => {
      e.preventDefault();
      const id = document.getElementById("bb-sf-id").value;
      const payload = {
        name: document.getElementById("bb-sf-name").value.trim(),
        website: document.getElementById("bb-sf-web").value.trim(),
        currency: document.getElementById("bb-sf-curr").value.trim() || "USD",
        lead_time_days: parseInt(document.getElementById("bb-sf-lead").value || "3", 10),
        shipping_notes: document.getElementById("bb-sf-ship").value.trim(),
      };
      try {
        await invFetch(id ? `/api/suppliers/${id}` : "/api/suppliers", {
          method: id ? "PUT" : "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(payload),
        });
        document.getElementById("bb-sup-dialog").close();
        showToast(`Saved supplier ${payload.name}`);
        if (window.location.pathname.includes("/inventory/")) {
          setTimeout(() => window.location.reload(), 650);
        }
      } catch (err) {
        showToast(err.message, true);
      }
    });

    document.getElementById("bb-scan-form").addEventListener("submit", async (e) => {
      e.preventDefault();
      const raw = document.getElementById("bb-scan-input").value.trim();
      const mode = document.getElementById("bb-scan-mode").value;
      const out = document.getElementById("bb-scan-result");
      if (!raw) return;
      try {
        const res = await invFetch(`/api/barcodes/resolve?code=${encodeURIComponent(raw)}`);
        if (res.match_kind === "part" && res.part) {
          let part = res.part;
          if (mode === "checkin") {
            part = await invFetch(`/api/parts/${encodeURIComponent(part.sku)}/adjust`, {
              method: "POST",
              headers: { "Content-Type": "application/json" },
              body: JSON.stringify({ delta: res.parsed_qty || 1, reason: "scan_in" }),
            });
            showToast(`Checked in ${part.sku}: Qty ${part.qty}`);
          } else if (mode === "checkout") {
            part = await invFetch(`/api/parts/${encodeURIComponent(part.sku)}/adjust`, {
              method: "POST",
              headers: { "Content-Type": "application/json" },
              body: JSON.stringify({ delta: -1, reason: "scan_out" }),
            });
            showToast(`Checked out ${part.sku}: Qty ${part.qty}`);
          }
          out.innerHTML = `<strong>${esc(part.sku)}</strong> — ${esc(part.name)} | Qty: <b>${part.qty}</b> | Bin: <b>${esc(part.location_code || "-")}</b> | $${Number(part.unit_cost).toFixed(2)}`;
        } else if (res.match_kind === "location" && res.location) {
          out.innerHTML = `Location <strong>${esc(res.location.code)}</strong> (${esc(res.location.name)}): ${res.location.part_count} SKUs, $${Number(res.location.total_value).toFixed(2)}`;
        } else {
          out.textContent = `No matching part or bin found for '${raw}'.`;
        }
      } catch (err) {
        out.textContent = `Error: ${err.message}`;
      }
    });
  }

  async function refreshLookupLists() {
    try {
      const [locs, sups] = await Promise.all([invFetch("/api/locations"), invFetch("/api/suppliers")]);
      cachedLocations = locs;
      cachedSuppliers = sups;
      const locList = document.getElementById("bb-loc-list");
      const supList = document.getElementById("bb-sup-list");
      if (locList) locList.innerHTML = locs.map((l) => `<option value="${esc(l.code)}">`).join("");
      if (supList) supList.innerHTML = sups.map((s) => `<option value="${esc(s.name)}">`).join("");
    } catch (_) {}
  }

  const bbPackCalc = { basePackQty: 1, basePackPrice: 0 };

  function updateBbCalcSummary() {
    const el = document.getElementById("bb-pf-calc-summary");
    if (!el) return;
    const fq = parseInt(document.getElementById("bb-pf-final-qty").value || document.getElementById("bb-pf-qty").value || "0", 10);
    const fcRaw = document.getElementById("bb-pf-final-cost").value.trim();
    const fc = fcRaw === "" ? NaN : parseFloat(fcRaw);
    if (fq > 0 && !Number.isNaN(fc) && fc > 0) {
      const ea = (fc / fq).toFixed(2);
      const prefix = bbPackCalc.basePackQty > 1 ? `Pack of ${bbPackCalc.basePackQty} | ` : "";
      el.textContent = `${prefix}${fq} units -> $${ea}/ea`;
      el.style.color = "#34d399";
    } else {
      el.textContent = "Enter final qty & total paid to divide unit cost.";
      el.style.color = "#9aa0aa";
    }
  }

  function recalcBbUnitCost() {
    const rawQty = document.getElementById("bb-pf-final-qty").value.trim();
    const rawCost = document.getElementById("bb-pf-final-cost").value.trim();
    if (rawQty !== "") {
      const fq = parseInt(rawQty, 10);
      if (fq > 0) document.getElementById("bb-pf-qty").value = String(fq);
    }
    const qty = parseInt(rawQty || document.getElementById("bb-pf-qty").value || "0", 10);
    if (rawCost !== "") {
      const fc = parseFloat(rawCost);
      if (!Number.isNaN(fc) && fc >= 0 && qty > 0) {
        document.getElementById("bb-pf-cost").value = (fc / qty).toFixed(2);
      }
    }
    updateBbCalcSummary();
  }

  async function openPartModal(sku, defaultCat) {
    ensureModals();
    await refreshLookupLists();
    let p = null;
    if (sku) {
      try {
        const detail = await invFetch(`/api/parts/${encodeURIComponent(sku)}`);
        p = detail.part;
      } catch (err) {
        showToast(err.message, true);
        return;
      }
    }
    document.getElementById("bb-part-modal-title").textContent = p ? `Edit Part ${p.sku}` : "Add New Part";
    document.getElementById("bb-pf-sku").value = p ? p.sku : "";
    document.getElementById("bb-pf-sku").readOnly = !!p;
    document.getElementById("bb-pf-cat").value = p ? p.category : (defaultCat || "microcontrollers");
    document.getElementById("bb-pf-name").value = p ? p.name : "";
    document.getElementById("bb-pf-spec").value = p ? p.spec : "";
    document.getElementById("bb-pf-pkg").value = p ? p.package : "";
    document.getElementById("bb-pf-qty").value = p ? p.qty : 1;
    document.getElementById("bb-pf-min").value = p ? p.min_qty : 1;
    document.getElementById("bb-pf-cost").value = p ? Number(p.unit_cost).toFixed(2) : "0.00";
    document.getElementById("bb-pf-loc").value = p ? (p.location_code || "") : "";
    document.getElementById("bb-pf-sup").value = p ? (p.primary_supplier_name || "") : "";
    document.getElementById("bb-pf-mpn").value = p ? p.mpn : "";
    document.getElementById("bb-pf-ssku").value = p ? p.supplier_sku : "";
    document.getElementById("bb-pf-url").value = p ? p.product_url : "";
    document.getElementById("bb-pf-proj").value = p ? p.projects_tags : "-";
    document.getElementById("bb-pf-desc").value = p ? (p.description || "") : "";
    document.getElementById("bb-pf-notes").value = p ? p.notes : "";
    document.getElementById("bb-pf-import-url").value = p ? p.product_url : "";
    document.getElementById("bb-pf-import-status").textContent = "";
    bbImgState.selectedImgUrl = "";
    bbImgState.selectedPinoutUrl = "";
    bbImgState.clearPartImg = false;
    bbImgState.clearPinoutImg = false;
    bbImgState.pendingPartFile = null;
    bbImgState.pendingPartObjectUrl = "";
    bbImgState.pendingPinoutFile = null;
    bbImgState.pendingPinoutObjectUrl = "";
    bbImgState.existingPartPath = p ? (p.image_path || "") : "";
    bbImgState.existingPinoutPath = p ? (p.pinout_image_path || "") : "";
    bbImgState.galleryImages = [];
    bbPackCalc.basePackQty = 1;
    bbPackCalc.basePackPrice = 0;
    document.getElementById("bb-pf-packs").value = "1";
    document.getElementById("bb-pf-final-qty").value = "";
    document.getElementById("bb-pf-final-cost").value = "";
    updateBbCalcSummary();
    document.getElementById("bb-pf-part-file").value = "";
    document.getElementById("bb-pf-pinout-file").value = "";
    renderBbImageSlots();
    renderBbGalleryStrip();
    document.getElementById("bb-part-dialog").showModal();
  }

  function openLocModal(code) {
    ensureModals();
    const l = code ? cachedLocations.find((x) => x.code === code) : null;
    document.getElementById("bb-loc-modal-title").textContent = l ? `Edit Location ${l.code}` : "Add Location / Bin";
    document.getElementById("bb-lf-code").value = l ? l.code : "";
    document.getElementById("bb-lf-name").value = l ? l.name : "";
    document.getElementById("bb-lf-area").value = l ? l.area : "";
    document.getElementById("bb-lf-rack").value = l ? l.rack : "";
    document.getElementById("bb-lf-drawer").value = l ? l.drawer : "";
    document.getElementById("bb-lf-barcode").value = l ? l.barcode_value : "";
    document.getElementById("bb-lf-desc").value = l ? l.description : "";
    document.getElementById("bb-loc-dialog").showModal();
  }

  function openSupModal(name) {
    ensureModals();
    const s = name ? cachedSuppliers.find((x) => x.name.toLowerCase() === name.toLowerCase()) : null;
    document.getElementById("bb-sup-modal-title").textContent = s ? `Edit Supplier ${s.name}` : "Add Supplier";
    document.getElementById("bb-sf-id").value = s ? s.id : "";
    document.getElementById("bb-sf-name").value = s ? s.name : "";
    document.getElementById("bb-sf-web").value = s ? s.website : "";
    document.getElementById("bb-sf-curr").value = s ? s.currency : "USD";
    document.getElementById("bb-sf-lead").value = s ? s.lead_time_days : 3;
    document.getElementById("bb-sf-ship").value = s ? s.shipping_notes : "";
    document.getElementById("bb-sup-dialog").showModal();
  }

  function openScanModal() {
    ensureModals();
    document.getElementById("bb-scan-result").textContent = "";
    document.getElementById("bb-scan-input").value = "";
    document.getElementById("bb-scan-dialog").showModal();
    setTimeout(() => document.getElementById("bb-scan-input")?.focus(), 50);
  }

  async function openUpdateModal(force = true) {
    ensureModals();
    const dlg = document.getElementById("bb-update-dialog");
    const body = document.getElementById("bb-update-body");
    const link = document.getElementById("bb-update-link");
    body.innerHTML = "<p style='color:#9aa0aa'>Checking GitHub for updates...</p>";
    if (!dlg.open) dlg.showModal();
    try {
      const info = await invFetch(`/api/updates/check${force ? "?force=1" : ""}`);
      if (info.release_url) link.href = info.release_url;
      const latestTxt = info.latest_tag || (info.latest_version ? `v${info.latest_version}` : "Not published");
      const commitTxt = info.local_commit ? ` (${esc(info.local_commit)})` : "";
      let statusTxt = "Up to Date";
      let statusCol = "#34d399";
      if (info.error) {
        statusTxt = "Check Failed";
        statusCol = "#fbbf24";
      } else if (info.update_available) {
        statusTxt = "Update Available";
        statusCol = "#fbbf24";
      }
      const notesHtml = info.release_notes
        ? `<pre style="white-space:pre-wrap;background:#171a21;border:1px solid rgba(140,145,155,0.22);border-radius:6px;padding:0.65rem;font-size:0.8rem;max-height:180px;overflow-y:auto;margin-top:0.6rem">${esc(info.release_notes)}</pre>`
        : "";
      const errHtml = info.error ? `<p style="color:#fbbf24;margin:0.45rem 0 0">${esc(info.error)}</p>` : "";
      body.innerHTML = `
        <p style="margin:0 0 0.45rem"><b>Current:</b> v${esc(info.current_version)}${commitTxt} &nbsp;|&nbsp; <b>Latest (${esc(info.repo)}):</b> ${esc(latestTxt)} &nbsp;|&nbsp; <b style="color:${statusCol}">${statusTxt}</b></p>
        ${errHtml}
        ${notesHtml}
        <div style="background:#171a21;border:1px solid rgba(140,145,155,0.22);border-radius:6px;padding:0.55rem 0.7rem;margin-top:0.6rem">
          <div style="font-size:0.75rem;color:#9aa0aa;margin-bottom:0.2rem">Update &amp; Restart Command</div>
          <code>${esc(info.update_command || "./wiki.sh update")}</code>
        </div>
      `;
    } catch (err) {
      body.innerHTML = `<p style="color:#f87171">Failed to check updates: ${esc(err.message)}</p>`;
    }
  }

  function renderActionBar(container, kpisHtml, hubHref, defaultCat) {
    container.innerHTML = `
      <div class="bb-action-bar">
        <div class="bb-action-bar__kpis">${kpisHtml}</div>
        <div class="bb-action-bar__btns">
          <button type="button" class="bb-btn bb-btn--primary" data-bb-act="add-part">+ Add Part</button>
          <button type="button" class="bb-btn" data-bb-act="add-loc">+ Add Location</button>
          <button type="button" class="bb-btn" data-bb-act="add-sup">+ Add Supplier</button>
          <button type="button" class="bb-btn" data-bb-act="scan">Quick Scan</button>
          <button type="button" class="bb-btn" data-bb-act="updates">Updates</button>
          <a class="bb-btn" href="${esc(hubHref)}">Open Inventory Studio</a>
        </div>
      </div>
    `;
    container.querySelector('[data-bb-act="add-part"]').addEventListener("click", () => openPartModal(null, defaultCat));
    container.querySelector('[data-bb-act="add-loc"]').addEventListener("click", () => openLocModal(null));
    container.querySelector('[data-bb-act="add-sup"]').addEventListener("click", () => openSupModal(null));
    container.querySelector('[data-bb-act="scan"]').addEventListener("click", openScanModal);
    container.querySelector('[data-bb-act="updates"]').addEventListener("click", () => openUpdateModal(true));
  }

  function kpiPillsHtml(v) {
    if (!v) return `<span class="bb-kpi-pill">Inventory Hub offline — run <code>./wiki.sh start</code></span>`;
    return `
      <span class="bb-kpi-pill">SKUs: <b>${v.total_skus}</b></span>
      <span class="bb-kpi-pill">Units: <b>${v.total_units}</b></span>
      <span class="bb-kpi-pill">Value: <b>$${Number(v.total_value).toFixed(2)}</b></span>
      <span class="bb-kpi-pill ${v.low_stock_skus > 0 ? "bb-kpi-pill--warn" : ""}">Low Stock: <b>${v.low_stock_skus}</b></span>
    `;
  }

  async function refreshExplorerStrip() {
    const strip = document.getElementById("bb-explorer-inventory-strip");
    if (!strip) return;
    let val = null;
    try {
      val = await invFetch("/api/valuation");
    } catch (_) {}
    renderActionBar(strip, kpiPillsHtml(val), "inventory-hub/", "microcontrollers");
  }

  async function openPartDetailModal(sku, defaultCat) {
    ensureModals();
    const dlg = document.getElementById("bb-part-detail-dialog");
    const title = document.getElementById("bb-detail-title");
    const body = document.getElementById("bb-detail-body");
    const footer = document.getElementById("bb-detail-footer");
    title.textContent = sku;
    body.innerHTML = "<p style='color:#9aa0aa'>Loading part details...</p>";
    footer.innerHTML = `<button type="button" class="bb-btn" onclick="document.getElementById('bb-part-detail-dialog').close()">Close</button>`;
    if (!dlg.open) dlg.showModal();
    try {
      const detail = await invFetch(`/api/parts/${encodeURIComponent(sku)}`);
      const p = detail.part;
      title.textContent = `${p.sku} — ${p.name}`;
      const mediaCards = [];
      if (p.image_path) {
        const src = `${invApi}/api/images/${encodeURIComponent(p.image_path)}`;
        mediaCards.push(`<div class="bb-detail-card"><div class="bb-detail-card__k">Part Photo</div><img src="${esc(src)}" alt="${esc(p.name)}" data-zoom="${esc(src)}" data-zoom-title="${esc(p.sku + ' — ' + p.name)}" /></div>`);
      }
      if (p.pinout_image_path) {
        const src = `${invApi}/api/images/${encodeURIComponent(p.pinout_image_path)}`;
        mediaCards.push(`<div class="bb-detail-card"><div class="bb-detail-card__k">Pinout / Wiring Diagram</div><img src="${esc(src)}" alt="Pinout" data-zoom="${esc(src)}" data-zoom-title="${esc(p.sku + ' — Pinout / Wiring Diagram')}" /></div>`);
      }
      const mediaHtml = mediaCards.length ? `<div class="bb-detail-media">${mediaCards.join("")}</div>` : "";
      const supInfo = [p.primary_supplier_name, p.supplier_sku ? `SKU: ${p.supplier_sku}` : "", p.mpn ? `MPN: ${p.mpn}` : ""].filter(Boolean).join(" | ") || "-";
      const links = [
        p.product_url ? `<a class="bb-btn bb-btn--sm" href="${esc(p.product_url)}" target="_blank" rel="noopener">Product Page</a>` : "",
        p.datasheet_url ? `<a class="bb-btn bb-btn--sm" href="${esc(p.datasheet_url)}" target="_blank" rel="noopener">Datasheet</a>` : "",
      ].filter(Boolean).join(" ");
      const descHtml = p.description ? `<div style="font-size:0.75rem;color:#9aa0aa;margin-bottom:0.25rem;text-transform:uppercase">Description</div><div class="bb-detail-text">${esc(p.description)}</div>` : "";
      const notesHtml = p.notes ? `<div style="font-size:0.75rem;color:#9aa0aa;margin-bottom:0.25rem;text-transform:uppercase">Notes</div><div class="bb-detail-text">${esc(p.notes)}</div>` : "";
      body.innerHTML = `
        ${mediaHtml}
        <div class="bb-detail-grid">
          <div class="bb-detail-kv"><div class="bb-detail-kv__k">SKU / Category</div><div class="bb-detail-kv__v">${esc(p.sku)} (${esc(p.category)})</div></div>
          <div class="bb-detail-kv"><div class="bb-detail-kv__k">Spec</div><div class="bb-detail-kv__v">${esc(p.spec || "-")}</div></div>
          <div class="bb-detail-kv"><div class="bb-detail-kv__k">Package</div><div class="bb-detail-kv__v">${esc(p.package || "-")}</div></div>
          <div class="bb-detail-kv"><div class="bb-detail-kv__k">Stock / Min</div><div class="bb-detail-kv__v">${p.qty} / ${p.min_qty}</div></div>
          <div class="bb-detail-kv"><div class="bb-detail-kv__k">Unit Cost / Value</div><div class="bb-detail-kv__v">$${Number(p.unit_cost).toFixed(2)} / $${Number(p.extended_value).toFixed(2)}</div></div>
          <div class="bb-detail-kv"><div class="bb-detail-kv__k">Location</div><div class="bb-detail-kv__v">${esc(p.location_code || "-")}</div></div>
          <div class="bb-detail-kv"><div class="bb-detail-kv__k">Supplier / MPN</div><div class="bb-detail-kv__v">${esc(supInfo)}</div></div>
          <div class="bb-detail-kv"><div class="bb-detail-kv__k">Added / Modified</div><div class="bb-detail-kv__v">${esc((p.created_at || "-").slice(0, 10))} / ${esc((p.updated_at || "-").slice(0, 10))}</div></div>
        </div>
        ${links ? `<div style="display:flex;gap:0.4rem;flex-wrap:wrap;margin-bottom:0.65rem">${links}</div>` : ""}
        ${descHtml}
        ${notesHtml}
      `;
      body.querySelectorAll("img[data-zoom]").forEach((img) => {
        img.addEventListener("click", () => openBbLightbox(img.dataset.zoom, img.dataset.zoomTitle));
      });
      footer.innerHTML = `
        <a class="bb-btn" target="_blank" rel="noopener" href="${invApi}/api/labels/part/${encodeURIComponent(p.sku)}.pdf?barcode=combo&preset=2x1">Print Label</a>
        <button type="button" class="bb-btn bb-btn--primary" id="bb-detail-edit-btn">Edit Part</button>
        <button type="button" class="bb-btn" onclick="document.getElementById('bb-part-detail-dialog').close()">Close</button>
      `;
      document.getElementById("bb-detail-edit-btn").addEventListener("click", () => {
        dlg.close();
        openPartModal(p.sku, defaultCat || p.category);
      });
    } catch (err) {
      body.innerHTML = `<p style="color:#f87171">Failed to load part: ${esc(err.message)}</p>`;
    }
  }

  function enhanceInventoryTables(defaultCat, partsBySku = {}) {
    const article = document.querySelector(".md-content__inner");
    if (!article) return;

    article.querySelectorAll("table").forEach((tbl) => {
      if (tbl.dataset.bbEnhanced === "1") return;
      const headRow = tbl.querySelector("thead tr");
      if (!headRow) return;
      const headers = Array.from(headRow.querySelectorAll("th")).map((th) => th.textContent.trim().toLowerCase());
      const skuIdx = headers.indexOf("sku");
      const nameIdx = headers.indexOf("part") !== -1 ? headers.indexOf("part") : headers.indexOf("name");
      const qtyIdx = headers.indexOf("qty");
      const valIdx = headers.indexOf("value");
      const codeIdx = headers.indexOf("code");
      const supIdx = headers.indexOf("supplier");

      if (skuIdx === 0 && qtyIdx !== -1) {
        tbl.dataset.bbEnhanced = "1";
        const actTh = document.createElement("th");
        actTh.textContent = "Manage";
        headRow.appendChild(actTh);

        tbl.querySelectorAll("tbody tr").forEach((tr) => {
          const cells = tr.querySelectorAll("td");
          if (cells.length <= qtyIdx) return;
          const sku = cells[0].textContent.trim();
          const partMeta = partsBySku[sku];
          cells[0].innerHTML = `<span class="bb-part-link">${esc(sku)}</span>`;
          cells[0].querySelector(".bb-part-link")?.addEventListener("click", () => openPartDetailModal(sku, defaultCat));
          if (nameIdx !== -1 && cells[nameIdx]) {
            const existingText = cells[nameIdx].innerHTML;
            const thumb = partMeta && partMeta.image_path
              ? `<img class="bb-part-thumb" src="${esc(invApi + "/api/images/" + encodeURIComponent(partMeta.image_path))}" alt="${esc(partMeta.name || sku)}" />`
              : "";
            cells[nameIdx].innerHTML = `<span class="bb-part-name-wrap">${thumb}<span class="bb-part-link">${existingText}</span></span>`;
            cells[nameIdx].querySelector(".bb-part-thumb")?.addEventListener("click", (e) => {
              e.stopPropagation();
              openBbLightbox(`${invApi}/api/images/${encodeURIComponent(partMeta.image_path)}`, `${sku} — ${partMeta.name || "Part Photo"}`);
            });
            cells[nameIdx].querySelector(".bb-part-link")?.addEventListener("click", () => openPartDetailModal(sku, defaultCat));
          }
          const pinBtnHtml = partMeta && partMeta.pinout_image_path
            ? `<button type="button" class="bb-btn bb-btn--sm" data-op="pinout">Pinout</button>`
            : "";
          const td = document.createElement("td");
          td.innerHTML = `<span class="bb-inline-actions">
            <button type="button" class="bb-btn bb-btn--sm" data-op="dec">-1</button>
            <button type="button" class="bb-btn bb-btn--sm" data-op="inc">+1</button>
            ${pinBtnHtml}
            <button type="button" class="bb-btn bb-btn--sm" data-op="edit">Edit</button>
            <a class="bb-btn bb-btn--sm" target="_blank" rel="noopener" href="${invApi}/api/labels/part/${encodeURIComponent(sku)}.pdf?barcode=combo&preset=2x1">Label</a>
          </span>`;
          td.querySelector('[data-op="pinout"]')?.addEventListener("click", () => {
            openBbLightbox(`${invApi}/api/images/${encodeURIComponent(partMeta.pinout_image_path)}`, `${sku} — Pinout / Wiring Diagram`);
          });
          td.querySelector('[data-op="dec"]').addEventListener("click", async () => {
            try {
              const p = await invFetch(`/api/parts/${encodeURIComponent(sku)}/adjust`, {
                method: "POST",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify({ delta: -1, reason: "wiki_adjust" }),
              });
              cells[qtyIdx].textContent = String(p.qty);
              if (valIdx !== -1) cells[valIdx].textContent = `$${Number(p.extended_value).toFixed(2)}`;
              showToast(`${p.sku} -> Qty ${p.qty}`);
            } catch (err) {
              showToast(err.message, true);
            }
          });
          td.querySelector('[data-op="inc"]').addEventListener("click", async () => {
            try {
              const p = await invFetch(`/api/parts/${encodeURIComponent(sku)}/adjust`, {
                method: "POST",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify({ delta: 1, reason: "wiki_adjust" }),
              });
              cells[qtyIdx].textContent = String(p.qty);
              if (valIdx !== -1) cells[valIdx].textContent = `$${Number(p.extended_value).toFixed(2)}`;
              showToast(`${p.sku} -> Qty ${p.qty}`);
            } catch (err) {
              showToast(err.message, true);
            }
          });
          td.querySelector('[data-op="edit"]').addEventListener("click", () => openPartModal(sku, defaultCat));
          tr.appendChild(td);
        });
      } else if (codeIdx === 0 && headers.includes("barcode")) {
        tbl.dataset.bbEnhanced = "1";
        const actTh = document.createElement("th");
        actTh.textContent = "Manage";
        headRow.appendChild(actTh);
        tbl.querySelectorAll("tbody tr").forEach((tr) => {
          const cells = tr.querySelectorAll("td");
          if (!cells.length) return;
          const code = cells[0].textContent.trim();
          const td = document.createElement("td");
          td.innerHTML = `<span class="bb-inline-actions">
            <button type="button" class="bb-btn bb-btn--sm" data-op="edit-loc">Edit</button>
            <a class="bb-btn bb-btn--sm" target="_blank" rel="noopener" href="${invApi}/api/labels/bin/${encodeURIComponent(code)}.pdf?barcode=combo&preset=ql62x42">Bin PDF</a>
          </span>`;
          td.querySelector('[data-op="edit-loc"]').addEventListener("click", async () => {
            await refreshLookupLists();
            openLocModal(code);
          });
          tr.appendChild(td);
        });
      } else if (supIdx === 0 && headers.includes("lead time")) {
        tbl.dataset.bbEnhanced = "1";
        const actTh = document.createElement("th");
        actTh.textContent = "Manage";
        headRow.appendChild(actTh);
        tbl.querySelectorAll("tbody tr").forEach((tr) => {
          const cells = tr.querySelectorAll("td");
          if (!cells.length) return;
          const name = cells[0].textContent.trim();
          const td = document.createElement("td");
          td.innerHTML = `<button type="button" class="bb-btn bb-btn--sm" data-op="edit-sup">Edit</button>`;
          td.querySelector('[data-op="edit-sup"]').addEventListener("click", async () => {
            await refreshLookupLists();
            openSupModal(name);
          });
          tr.appendChild(td);
        });
      }
    });
  }

  async function initBenchBearInventory() {
    ensureModals();
    const path = window.location.pathname;
    if (document.getElementById("bb-explorer-inventory-strip")) {
      await refreshExplorerStrip();
      return;
    }

    const isInvPage = path.includes("/inventory/");
    const isProjPage = path.includes("/projects/");
    if (!isInvPage && !isProjPage) return;

    const article = document.querySelector(".md-content__inner");
    const h1 = article?.querySelector("h1");
    if (!article || !h1) return;

    const segs = path.split("/").filter(Boolean);
    const lastSeg = segs[segs.length - 1] || "microcontrollers";
    const defaultCat = ["valuation", "locations", "suppliers"].includes(lastSeg) ? "microcontrollers" : lastSeg;
    const hubHref = isInvPage ? "../../inventory-hub/" : "../../inventory-hub/";

    let barHost = document.getElementById("bb-page-action-bar");
    if (!barHost) {
      barHost = document.createElement("div");
      barHost.id = "bb-page-action-bar";
      h1.insertAdjacentElement("afterend", barHost);
    }

    let val = null;
    const partsBySku = {};
    try {
      const [v, parts] = await Promise.all([invFetch("/api/valuation"), invFetch("/api/parts")]);
      val = v;
      if (Array.isArray(parts)) {
        parts.forEach((p) => { partsBySku[p.sku] = p; });
      }
    } catch (_) {}
    renderActionBar(barHost, kpiPillsHtml(val), hubHref, defaultCat);
    enhanceInventoryTables(defaultCat, partsBySku);
  }

  if (typeof window.document$ !== "undefined" && typeof window.document$.subscribe === "function") {
    window.document$.subscribe(() => {
      initBenchBearInventory();
    });
  } else {
    document.addEventListener("DOMContentLoaded", initBenchBearInventory);
  }
})();
