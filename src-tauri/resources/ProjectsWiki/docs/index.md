---
hide:
  - navigation
  - toc
---

# Projects & Hardware Archive

<p class="hero-subtitle">Interactive index of hardware builds, synthesizers, microcontrollers, cyberdecks, mechanical restorations, and software tools.</p>

<div id="wiki-explorer-app" class="explorer-wrapper">
  <div class="explorer-toolbar" role="search" aria-label="Filter and search projects">
    <div class="search-input-wrap">
      <span class="search-icon" aria-hidden="true">⌕</span>
      <input
        type="search"
        id="project-search"
        class="explorer-search-input"
        placeholder="Filter projects by name, chip (ESP32, RP2040), or keyword... (Press /)"
        autocomplete="off"
        aria-label="Filter projects"
      />
      <kbd class="search-kbd" id="search-kbd-hint" aria-hidden="true">/</kbd>
    </div>

    <div class="toolbar-controls">
      <select id="project-sort" class="explorer-select" aria-label="Sort projects">
        <option value="name-asc">Sort: A → Z</option>
        <option value="name-desc">Sort: Z → A</option>
        <option value="modified-desc">Sort: Modified (Newest)</option>
        <option value="modified-asc">Sort: Modified (Oldest)</option>
        <option value="created-desc">Sort: Created (Newest)</option>
        <option value="created-asc">Sort: Created (Oldest)</option>
        <option value="category">Sort: Category</option>
      </select>

      <div class="view-toggle-group" role="group" aria-label="View layout">
        <button type="button" id="view-grid-btn" class="view-toggle-btn is-active" aria-pressed="true" title="Grid View">
          ⊞ Grid
        </button>
        <button type="button" id="view-list-btn" class="view-toggle-btn" aria-pressed="false" title="Compact List View">
          ☰ List
        </button>
      </div>

      <button
        type="button"
        id="boot-toggle-btn"
        class="explorer-btn explorer-btn--ghost"
        aria-pressed="false"
        title="Toggle starting ProjectsWiki automatically at boot (systemd user service)"
      >
        <span class="boot-dot" aria-hidden="true"></span>
        <span class="boot-label">Boot: OFF</span>
      </button>

      <button type="button" id="sync-projects-btn" class="explorer-btn explorer-btn--ghost" title="Scan projects directory for new repos and refresh catalog">
        <span class="sync-icon" aria-hidden="true">↻</span>
        <span class="sync-label">Sync</span>
      </button>

      <button type="button" id="random-peek-btn" class="explorer-btn explorer-btn--accent" title="Preview a random project via AJAX">
        🎲 Surprise Me
      </button>
    </div>
  </div>

  <div id="category-pills" class="category-pills" role="toolbar" aria-label="Project categories"></div>

  <div class="explorer-status-bar">
    <span id="results-meta" class="results-meta" aria-live="polite">Loading project catalog...</span>
    <button type="button" id="reset-filters-btn" class="reset-filters-btn" hidden>
      Reset filters ×
    </button>
  </div>

  <div id="projects-grid" class="projects-grid" tabindex="-1"></div>
</div>

<dialog id="project-preview-dialog" class="wiki-modal" aria-labelledby="modal-project-title">
  <div class="wiki-modal__header">
    <div class="wiki-modal__title-group">
      <span id="modal-project-category" class="card-category-badge">Project</span>
      <h2 id="modal-project-title" class="wiki-modal__title" tabindex="-1">Project Preview</h2>
    </div>
    <div class="wiki-modal__actions">
      <button type="button" id="modal-tune-btn" class="explorer-btn explorer-btn--ghost" aria-expanded="false" title="Fix or tune this project's description, title, or category">
        ✏️ Tune / Edit
      </button>
      <button type="button" id="modal-random-btn" class="explorer-btn explorer-btn--ghost" title="Load another random project">
        🎲 Next Random
      </button>
      <a id="modal-fullpage-link" href="#" class="explorer-btn explorer-btn--primary">
        Open Full Page →
      </a>
      <button
        type="button"
        commandfor="project-preview-dialog"
        command="close"
        id="modal-close-btn"
        class="modal-close-btn"
        aria-label="Close preview"
        title="Close (Esc)"
      >
        ✕
      </button>
    </div>
  </div>

  <div id="modal-tune-drawer" class="tune-drawer" hidden>
    <div class="tune-drawer__section tune-drawer__ai">
      <label for="tune-prompt-input" class="tune-label">
        ✨ <strong>Quick Correction / AI Tune</strong> — Tell the wiki what this project actually is:
      </label>
      <div class="tune-ai-row">
        <input
          type="text"
          id="tune-prompt-input"
          class="tune-input"
          placeholder="e.g., It's not a camshaft, it's a custom Raspberry Pi Wi-Fi security camera I made ages ago..."
        />
        <button type="button" id="tune-ai-btn" class="explorer-btn explorer-btn--accent">
          ✨ Smart Tune
        </button>
      </div>
    </div>

    <div class="tune-drawer__grid">
      <div class="tune-field">
        <label for="tune-title-input" class="tune-label">Title</label>
        <input type="text" id="tune-title-input" class="tune-input" />
      </div>
      <div class="tune-field">
        <label for="tune-category-select" class="tune-label">Category</label>
        <select id="tune-category-select" class="tune-select">
          <option value="Audio & Synths">Audio & Synths</option>
          <option value="ESP32 & Embedded">ESP32 & Embedded</option>
          <option value="Displays & E-Ink">Displays & E-Ink</option>
          <option value="Cyberdecks & PCs">Cyberdecks & PCs</option>
          <option value="Vehicles & Mechanical">Vehicles & Mechanical</option>
          <option value="Software & Tools">Software & Tools</option>
          <option value="Hardware & Mods">Hardware & Mods</option>
        </select>
      </div>
    </div>

    <div class="tune-field">
      <label for="tune-overview-input" class="tune-label">Overview & Notes (Markdown — Directory files & photo gallery below are preserved automatically)</label>
      <textarea id="tune-overview-input" class="tune-textarea" rows="4"></textarea>
    </div>

    <div class="tune-drawer__footer">
      <span id="tune-status-msg" class="tune-status-msg" aria-live="polite"></span>
      <button type="button" id="tune-cancel-btn" class="explorer-btn explorer-btn--ghost">
        Cancel
      </button>
      <button type="button" id="tune-save-btn" class="explorer-btn explorer-btn--primary">
        💾 Save Changes
      </button>
    </div>
  </div>

  <div id="modal-project-body" class="wiki-modal__body">
    <div class="modal-loading">Loading project documentation...</div>
  </div>
</dialog>
