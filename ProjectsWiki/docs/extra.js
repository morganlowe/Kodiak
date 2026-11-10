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
    const host = window.location.hostname || "wiki.local";
    if (host === "wiki.local") return;
    rootEl.querySelectorAll('a[href*="://wiki.local:8765/"], img[src*="://wiki.local:8765/"]').forEach((el) => {
      if (el.hasAttribute("href")) {
        el.setAttribute("href", el.getAttribute("href").replace("://wiki.local:8765/", `://${host}:8765/`));
      }
      if (el.hasAttribute("src")) {
        el.setAttribute("src", el.getAttribute("src").replace("://wiki.local:8765/", `://${host}:8765/`));
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
                      ⚡ Quick Peek
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
      modalBody.innerHTML = `<div class="modal-loading">⚡ Loading <strong>${escapeHtml(item.name)}</strong> via AJAX...</div>`;

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
            ? `✓ Synced ${catalog.length} projects (+${addedCount} new)`
            : `✓ Synced — all ${catalog.length} projects up to date`;
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
          ? "✓ Enabled automatic Start at Boot (systemd user service)"
          : "✓ Disabled automatic Start at Boot";
      } catch (_err) {
        resultsMetaEl.textContent = "⚠ Run 'wiki' in terminal to start the local control API first.";
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
          mode === "ai" ? "✨ Tuning description with AI..." : "💾 Saving changes...";
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

        if (tuneStatusMsg) tuneStatusMsg.textContent = "✓ Saved & updated!";
        setTimeout(() => {
          if (tuneDrawer) tuneDrawer.hidden = true;
          modalTuneBtn?.setAttribute("aria-expanded", "false");
        }, 700);
      } catch (_err) {
        if (tuneStatusMsg) tuneStatusMsg.textContent = "⚠ Could not reach local API on port 8765.";
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
