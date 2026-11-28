# Changelog

All notable changes to **Kodiak** will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.3.0] — 2026-11-27

### Added

- **BenchBear v1.0.0 Rust Inventory, Locations, Suppliers, Costs & Barcode Engine Integration ([github.com/morganlowe/BenchBear](https://github.com/morganlowe/BenchBear), `ProjectsWiki/`)**:
  - Integrated the standalone **BenchBear** (`benchbear v1.0.0`) Rust engine into `ProjectsWiki/` (`ProjectsWiki/Cargo.toml`, `ProjectsWiki/Cargo.lock`, `ProjectsWiki/src/`, `ProjectsWiki/docs/inventory-hub.md`, `ProjectsWiki/docs/extra.js`, `ProjectsWiki/docs/extra.css`, `ProjectsWiki/app/wiki_app.py`, `ProjectsWiki/mcp/server.py`, `ProjectsWiki/mcp/README.md`).
  - **Portable SQLite Inventory & Markdown Sync**: `inventory.db` configured in `DELETE` journal mode with `FULL` synchronous writes, atomic `snapshot.json` mirroring, and regenerated category/location/supplier/valuation Markdown tables (`inventory/` and `docs/inventory/`).
  - **Locations, Suppliers, Costs & BOMs**: Structured bins (`area`, `rack`, `drawer`, `BIN:` barcodes), seeded and custom vendors, multi-supplier quotes, inventory valuation rollups, supplier-grouped low-stock reorder sheets, URL product scraper with packs/final-cost calculator and photo/pinout gallery picker, clickable part detail modals, and atomic project BOM stock deduction.
  - **1D/2D Thermal Labels & Barcode Scanning**: Pure-Rust Code 128 and QR generation (`2x1`, `2.25x1.25`, Brother QL `ql62x29`/`ql62x42`, Phomemo sizes, and custom dimensions) in SVG, vector PDF, 203-DPI PNG, and driverless Phomemo ESC/POS `.phomemo` raster output, plus USB keyboard-wedge, camera, image upload, and DigiKey/Mouser/LCSC 2D bag barcode resolution.
  - **18-Tool Stdio MCP Server**: `benchbear mcp` and `ProjectsWiki/mcp/server.py` wrapper exposing inventory, bin, supplier, valuation, reorder, BOM, label, and scan tools.
- **BenchBear Branding**: Rebranded Kodiak’s companion navigation, setup wizard, Settings, embedded inventory hub, wiki site, and documentation as BenchBear, using the current BenchBear logo while preserving existing Kodiak companion package names and user data paths.
- **Kodiak Wiki Tab Inventory Hub Surface (`src-tauri/src/pipeline.rs`, `src/main.ts`)**:
  - Extended `WikiStatusDetails` and `get_detailed_wiki_status` to probe `http://127.0.0.1:8090/api/health`, track `inventory_reachable`, `inventory_url`, and live SKU/bin/supplier/BOM counts, and exclude `inventory-hub.md` from `doc_count`.
  - Added the live `Inventory :8090` badge, `Inventory Hub` iframe button (`#open-wiki-inventory-btn`), and direct `Hub :8090` external link to the Projects Wiki toolbar, plus `benchbear.exe` cleanup in Windows `stop_wiki_service`.

### Changed

- **Packaged & Read-Only Deployment Hardening (`ProjectsWiki/wiki_config.py`, `ProjectsWiki/wiki_server.py`, `ProjectsWiki/wiki.sh`, `ProjectsWiki/src/main.rs`)**:
  - Added writable-directory detection (`_default_data_dir()`) falling back to `~/.local/share/kodiak/projectswiki-data` (Linux/macOS) or `%LOCALAPPDATA%\kodiak\projectswiki-data` (Windows) when `ProjectsWiki` lives under a read-only system install.
  - Updated `wiki_server.py` to resolve `WIKI_DIR` next to `sys.executable` when PyInstaller-frozen, guard `cargo build --release` on writable directories, pass `CREATE_NO_WINDOW` to `benchbear` on Windows, and clean up `inv_proc` cleanly when frozen `mkdocs.commands.serve` exits.
  - Updated `wiki.sh` `run_daemon_foreground()` to avoid calling `systemctl --user stop projects-wiki.service` inside systemd `ExecStart`, and made Markdown export errors non-fatal during `benchbear serve` when `ProjectsWiki` is read-only.
- Updated package versions, UI labels, `README.md` (with links to `https://github.com/morganlowe/BenchBear`), and `CHANGELOG.md` to `v1.3.0`.

### Packaging & CI

- Updated `PKGBUILD`, `scripts/build_deb.sh`, `scripts/build_windows_wiki.py`, `Makefile`, `.gitignore`, and `.github/workflows/ci.yml` to compile, test, and package `benchbear` (`kodiak-projectswiki` switched to `x86_64` on Arch and `amd64` on Debian, and Windows `with-wiki` installer bundles both `kodiak-projectswiki.exe` and `benchbear.exe`).

## [1.2.1] — 2026-11-27

### Added

- **Optional GitHub Releases Update Checker & SHA-256 Verified Downloader (`src-tauri/src/updater.rs`, `src-tauri/src/config.rs`, `src-tauri/src/lib.rs`, `src/main.ts`)**:
  - Added opt-in `AppConfig.check_for_updates` (`false` by default) and `AppConfig.skipped_update_version` (`None` by default) so Kodiak makes zero unsolicited GitHub network calls unless enabled.
  - Added backend module `updater.rs` and 5 Tauri commands (`check_for_updates`, `download_kodiak_update`, `open_update_download_dir`, `launch_downloaded_installer`, `skip_update_version`) querying `api.github.com/repos/morganlowe/Kodiak/releases/latest`, detecting host variant (`arch`, `debian`, `appimage`, `windows-with-wiki`, `windows-without-wiki`), validating release download URLs/filenames, verifying SHA-256 checksums via `sha2` before finalizing downloads into `~/Downloads/kodiak-updates/v<ver>/`, and generating copyable install commands (`sudo pacman -U ...` / `sudo dpkg -i ...`) without executing elevated package-manager commands.
  - Added the **Software Updates (GitHub Releases)** panel in Settings, a dismissible Workbench **Update Available** banner, a **Check for Updates** button and badge in About, and optional companion `kodiak-projectswiki` toggle on Arch/Debian.
  - Added 6 Rust unit tests in `updater::tests` covering semver comparison, platform asset selection, SHA-256 digest extraction, URL/filename safety validation, install command generation, and opt-in config defaults (`91` passing tests total).

### Changed

- Updated package versions, UI labels, `README.md`, `config.example.toml`, and `docs/DECISIONS.md` to `v1.2.1` (`Kodiak_1.2.1_x64-with-wiki-setup.exe` and `Kodiak_1.2.1_x64-without-wiki-setup.exe`).

## [1.2.0] — 2026-11-27

### Added

- **Iterative Multi-Round & Conversational Pipeline (`src-tauri/src/history.rs`, `src-tauri/src/pipeline.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/config.rs`, `src-tauri/src/events.rs`)**:
  - Replaced the single-shot pipeline with multi-round archival (`TaskRoundSnapshot`, `TaskRecord::archive_current_round`) and Tauri commands (`rerun_task_from_stage`, `continue_task_round`, `retry_task_step`, `resume_paused_task`, `cancel_active_iteration`) while preserving Charter Section 1 metadata-only cloud isolation.
  - Added `PipelineEvent::RoundChanged`, `AppState.active_cancel`, Stage 2 `pause_on_request` pause/resume flow (`ExecuteStageOptions`, `run_execute_stage_advanced`, `run_single_step_with_events`), read-only two-phase discovery (`discovery_pass_enabled`), and opt-in bounded automatic repair (`auto_repair_enabled`, `max_auto_repair_rounds`).
  - Added `VerificationResult.response` and Stage 3 synthesis instructions in `VERIFY_SYSTEM_PROMPT` to deliver direct, grounded user answers alongside pass/concerns/fail verdicts.
- **Plan Approval Gate (`src-tauri/src/lib.rs`, `src/main.ts`)**:
  - Enabled `pause_after_plan` by default (`AppConfig.pause_after_plan = true`) with backend commands `approve_plan_and_execute`, `revise_pending_plan`, and `discard_pending_plan`.
  - Rendered an interactive Plan Review card with per-step selection checkboxes, revision input (`Revise Plan`), **Approve & Execute Plan**, and **Discard Plan** actions.
- **Unified Conversational Thread UI & Docked Stage Progress Bar (`src/main.ts`, `src/theme.css`)**:
  - Replaced flat output blocks with chronological `You` -> `Kodiak` conversation turns, bottom `Reply / Continue Conversation` composer (`Ctrl+Enter` shortcut), collapsible archived round details, inline Workbench toggles (`Pause for Plan Review`, `Auto-Repair`), collapsible **Live Event Ticker**, Settings controls, and Unified Inbox `Fix Test Errors` recovery action.
  - Docked the 4-segment stage progress bar (`Interpret | Execute | Verify | Done`) flush below the Health Strip on the Workbench tab so scrolled content never covers or peeks above it.
  - Added an explicit **Not yet written to disk** banner in active rounds when file/command Action Cards remain `pending_approval`.
- **Follow-Up Objective Promotion, Tool Normalization & Breadth-First Discovery (`src-tauri/src/pipeline.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/prompts.rs`)**:
  - Added `build_effective_round_prompt` so Round 2+ follow-up instructions take primary placement over initial Round 1 prompts, and `build_local_prior_round_context` to seed archived step summaries, shell outputs, and prior answers into local Stage 2 execution.
  - Added `normalize_plan_steps` to normalize common LLM tool aliases (such as `create_file`, `write_file`, `draft_doc`) onto `draft_code`, and updated `list_project_relative_files` to breadth-first traversal (all root files first, per-subdirectory cap of 8 files with `(+N more files)` indicator, limit raised to 80).
- **Unit Tests (`src-tauri/src/lib.rs`)**:
  - Added 15 Rust unit tests covering iterative config defaults, `TaskPlan` serde aliases, `TaskRecord` round archiving, `VerificationResult.response`, plan step filtering, Charter §1 context sanitization, Stage 2 `REQUEST` pause/resume, discovery detection, shell error detection, plan validation, tool normalization, follow-up prompt promotion, local prior-round context seeding, and BFS file discovery (85 tests passing, 0 failed, 3 ignored).

### Changed

- Updated package versions, UI labels, `README.md`, `config.example.toml`, and `docs/DECISIONS.md` to `v1.2.0` (`Kodiak_1.2.0_x64-with-wiki-setup.exe` and `Kodiak_1.2.0_x64-without-wiki-setup.exe`).
- Updated `VERIFY_SYSTEM_PROMPT` with an explicit `CRITICAL APPROVAL-STATUS RULE` so Stage 3 never claims `pending_approval` or `dry_run` files were already written to disk.

### Fixed

- Fixed cascade attempt classification (`classify_cascade_attempt_code`) so interpreter plan rejections log as `invalid_plan` rather than `429`.
- Fixed `TaskPlan` deserialization resilience for `files_included` / `files` / `target_files` aliases and missing `step.id` values, plus strict rejection of unsupported `route_tool` names in `validate_plan` (preventing silent fallthrough into `shell`).
- Replaced unsafe byte-slice string truncation in `build_iterative_interpret_context` and `build_verify_user_message` with UTF-8 character-safe `truncate_chars`.
- Fixed `validate_plan` to allow read-only `search_and_summarize` and `review_git_diff` steps on `.` or `""`.

## [1.1.9] — 2026-11-12

### Added

- **Light / Dark / Match System Theme (`index.html`, `src/theme.css`, `src/main.ts`)**:
  - Added an **Appearance & Theme** selector in Settings with **Match System (Currently Light/Dark)**, **Light**, and **Dark** modes.
  - Added synchronous FOUC-prevention in `index.html` and mapped `<meta name="color-scheme">` to `:root` `color-scheme` with full light-mode custom property overrides and live OS `prefers-color-scheme` / `localStorage` sync.
- **Integrated Lumo Bridge Authentication (`src-tauri/src/pipeline.rs`, `src-tauri/src/lib.rs`, `src/main.ts`)**:
  - Added Tauri commands (`check_lumo_auth`, `launch_lumo_login_browser`, `complete_lumo_auth`) and a **Lumo Bridge Authentication** panel in Settings to check `lumo-tamer` session status, launch Chrome/Chromium with CDP on port `9222` using a persistent `lumo-cdp` profile, extract tokens non-interactively, and restart `lumo-tamer`.
  - Added an actionable re-authentication hint to `HTTP 500` Lumo Bridge errors and a **Re-auth Lumo** button directly on `Cloud Lumo Bridge` stage failure banners.

### Changed

- Updated documentation and package versions to `v1.1.9` (`Kodiak_1.1.9_x64-with-wiki-setup.exe` and `Kodiak_1.1.9_x64-without-wiki-setup.exe`).

## [1.1.8] — 2026-11-11

### Fixed

- **Windows Release Staging Directory (`.github/workflows/release.yml`)**: Moved staged Windows NSIS installers out of `dist/windows-releases/` into `$env:RUNNER_TEMP\windows-releases` so Vite's `dist/` cleanup during the second Tauri build does not erase `Kodiak_<ver>_x64-without-wiki-setup.exe` before upload.

### Changed

- Updated documentation and package versions to `v1.1.8` (`Kodiak_1.1.8_x64-with-wiki-setup.exe` and `Kodiak_1.1.8_x64-without-wiki-setup.exe`).

## [1.1.7] — 2026-11-11

### Added

- **Dual Windows Release Installers (With & Without ProjectsWiki)**:
  - Added `src-tauri/tauri.nowiki.conf.json` (`bundle.resources: []`) to produce a standalone core Kodiak bundle without bundled `ProjectsWiki` resources.
  - Updated `.github/workflows/release.yml` to build and attach both `Kodiak_1.1.7_x64-with-wiki-setup.exe` (including `kodiak-projectswiki.exe`, templates, and docs) and `Kodiak_1.1.7_x64-without-wiki-setup.exe` (core orchestrator only) on Windows tags alongside Linux `.deb` and `.AppImage` bundles.

### Changed

- **Documentation & Version Bump**:
  - Updated `README.md` Windows installation instructions to document both `Kodiak_1.1.7_x64-with-wiki-setup.exe` and `Kodiak_1.1.7_x64-without-wiki-setup.exe` and bumped package versions to `v1.1.7`.

## [1.1.6] — 2026-11-11

### Added

- **In-App New Folder & Project Creation**:
  - Added backend services (`get_project_creation_context_service`, `create_project_or_folder_service`) and Tauri commands (`get_project_creation_context`, `create_project_or_folder`) in `src-tauri/src/pipeline.rs` and `src-tauri/src/lib.rs` with deny-list / system-path guards, relative subpath validation (`validate_relative_subpath`, `validate_safe_creation_target`), optional starter `README.md`, optional `git init`, optional whitelist registration (grant level `READ` / `CREATE` / `BUILD` / `AUTONOMOUS`), and active project switching.
  - Added a dedicated modal `<dialog>` (`#create-project-dialog` in `src/main.ts` and `src/theme.css`) with project vs folder modes, preset parent selector, custom parent input with live validation and resolved path preview, accessible labels/hints/validation feedback, and trigger actions from the Sidebar, Workbench, Settings, Dashboard, and project context menu (`New Subfolder Here...`).
  - Added Rust unit test `test_create_project_and_folder_lifecycle_and_safety` covering project/folder creation, subpath checks, deny-list protection, and whitelist enrollment.

### Changed

- **Author Copyright, Acknowledgements & Documentation**:
  - Enforced the repository-wide no-emojis policy across UI elements, documentation, and changelog entries.
  - Replaced the footer emblem with the bordered Morninglion Industries emblem (`src/main.ts`).
  - Updated copyright and systems architecture attribution to **Morgan Lowe** across `README.md`, `package.json`, `src-tauri/Cargo.toml`, `PKGBUILD`, `scripts/build_deb.sh`, and the desktop application's About panel (`src/main.ts`).
  - Added **Acknowledgements & Shout-Outs** (recognizing Morgan Lowe, AI model collaborators, open-source ecosystem, and Frank development hardware specifications) and improved `README.md` navigation and package install instructions.

### Fixed

- **Packaging Hygiene**: Strictly excluded non-template project markdown documentation files from packaged assets.
- **Settings Edit Provider Cancel Action**: Removed duplicate `cancel-edit-alt-provider-btn` element ID when editing an alternate provider (`#cancel-edit-alt-provider-bottom-btn`) and wired both Cancel buttons in `src/main.ts` to the same reset handler.

## [1.1.5] — 2026-11-10

### Added

- **Standalone Windows ProjectsWiki (`kodiak-projectswiki.exe`)**:
  - Created cross-platform unified runner `ProjectsWiki/wiki_server.py` supporting `serve`, `sync`, `build`, `rebuild`, and `stop` subcommands across Windows and Linux.
  - Automated Windows freeze build script (`scripts/build_windows_wiki.py`) using PyInstaller to package the complete Python runtime, MkDocs Material, PyYAML, and the ProjectsWiki Sync API into a self-contained executable.
  - Bundled ProjectsWiki assets, templates, and documentation directly into Kodiak's Windows installer (`src-tauri/resources/ProjectsWiki`) — zero Python or pip setup required for Windows users.
  - Cross-platform process execution in Tauri backend (`src-tauri/src/pipeline.rs`): runs `kodiak-projectswiki.exe` silently in the background with Windows `CREATE_NO_WINDOW` (no command prompt popups).
  - Cross-platform directory resolution in `src-tauri/src/config.rs` (`resolve_wiki_dir`) supporting installer resource directories, local development paths, and `%LOCALAPPDATA%`.
  - Platform-adaptive Wiki Setup Wizard in frontend (`src/main.ts`): provides Windows `hosts` path guidance (`C:\Windows\System32\drivers\etc\hosts`) and tailored background daemon management.

### Removed / Changed

- **Retired WiX / MSI Packaging in Favor of Single NSIS `.exe`**:
  - Dropped `msi` bundle target from `src-tauri/tauri.conf.json` and GitHub Actions release matrix.
  - Streamlined Windows distribution to a single, fast, self-contained NSIS installer (`Kodiak_1.1.5_x64-setup.exe`), cutting release build duration in CI.

## [1.1.4] — 2026-11-10

### Added

- **Automatic & Manual Project Directory Rescan**:
  - Automatically rescans the configured `projects_dir` (e.g. `~/Projects`) upon application launch via `rescan_projects_service` (`rescan_projects_dir`).
  - Detects new project folders and subfolders containing project markers (`.ino` Arduino sketches, `.git` repositories, `Cargo.toml`, `package.json`, `Makefile`, `CMakeLists.txt`, `platformio.ini`, `pyproject.toml`).
  - Automatically registers newly discovered projects into `cfg.whitelist` with `READ` grant level, persists `config.toml`, and updates documentation in the Projects Wiki catalog.
  - Added a dedicated manual rescan button (`↻`) on the sidebar **Projects** header with spin animation, tooltip, and duplicate-prevention state locking.
  - Added a companion **"↻ Rescan Directory"** button in **Settings** under *Project Whitelist & Grant Matrix*.
- **Directory Search on Main Side Window**:
  - Embedded real-time search input inside the sidebar's Projects section.
  - Instant case-insensitive filtering matching both project directory names and full paths.
  - Dynamic match counter badge (`matched / total`) displaying live results.
  - Keyboard navigation: press `Enter` to activate the first filtered project; press `Escape` or click 'Clear' to clear the filter.
  - Input focus and selection ranges are preserved during background UI re-renders.
- **Persistent UI Collapsed States**:
  - Persists all collapsible UI sections across application restarts and closes:
    - Entire sidebar toggle (`sidebarCollapsed`)
    - Projects list toggle (`sidebarProjectsCollapsed`)
    - Memories list toggle (`sidebarMemoriesCollapsed`)
    - Tasks list toggle (`sidebarTasksCollapsed`)
    - Settings Project Whitelist toggle (`settingsProjectsCollapsed`)
    - Local Decision Journal toggle (`journalCollapsed`)
  - Dual-layer storage ensures settings are saved in Web Storage (`localStorage`) and synced to `AppConfig.ui_collapsed` in `~/.config/kodiak/config.toml`.

## [1.1.3] — 2026-11-10

### Added

- **Workbench Active Context Card**:
  - Prominent context header above the Posture bar displaying the active whitelisted project directory path, whitelist grant badges (`[READ]`, `[BUILD]`, etc.), and live job status.
  - Interactive context badge clearly distinguishing `Live Workbench` from `Historical Job View`.
  - Quick action buttons: **`+ New Task`** (clears lingering card diagnostics, resets pipeline stages to idle, and focuses the prompt), **`[Dismiss Error]`** (clears failed/deferred error cards from workbench view without deleting records), and **`[Wiki]`** (one-click jump to project documentation).
  - Dynamic drafting indicator showing a live truncated prompt preview while entering instructions.
- **Configurable Alternate Provider Timeout**:
  - Added `timeout_secs` support to `AltProviderConfig` (persisted in `~/.config/kodiak/config.toml`).
  - Increased default alternate provider timeout from 60 seconds to 180 seconds (3 minutes), providing ample headroom for deep reasoning and thinking models (such as Gemini with extended thinking) and preventing premature client timeouts during provider capacity spikes.
  - Increased connection probe timeout from 10 seconds to 30 seconds.

### Fixed

- **Chained Read-Only Command Parsing**:
  - Updated `is_readonly_command` in `src-tauri/src/enforcer.rs` to evaluate chained command segments (`&&` and `;`). If all segments are read-only (`git status`, `ls`, `cat`, `pwd`, `rg`) and contain no redirects (`>`) or subshells (`$()`), they now execute cleanly under `READ` grants without triggering false-positive action class mismatch denials.
- **Action Class Alignment**:
  - If a cloud model planner marks a shell command as `read` when it requires `execute`, the pipeline aligns it to `execute` so the enforcer evaluates it against the project's whitelist grant level (prompting for approval if needed) rather than failing with an unrecoverable action-class mismatch error.
- **Project Switch State Isolation**:
  - Switching projects via the sidebar or dropdown now cleanly resets the live workbench (`state.currentTask = null`, stage indicators idle, draft cleared) so stale diagnostic cards from another project or past runs never linger or bleed over.
- **Gemini Model Optimization**:
  - Prioritized `gemini-flash-lite-latest` and `gemini-flash-latest` in the preset selector and model fetcher, ensuring sub-2-second response times and avoiding temporary Google 503 high-demand spikes on `gemini-3.8-flash`.

## [1.1.2] — 2026-11-10

### Added

- **Right-Click Project Exclusions**:
  - Custom right-click context menu on sidebar project items to easily exclude directories from view without deleting whitelist configuration.
  - Excluded directories persist across restarts in `~/.config/kodiak/config.toml` (`exclusion_list`).
  - Dedicated **Settings → Excluded Projects** panel to inspect and restore excluded directories with 1 click.
  - Suppressed default browser navigation context menu (Back/Forward/Reload) across the desktop app.
- **Runtime Custom CSS Engine**:
  - Live CSS style injection engine with dynamic `<style id="kodiak-custom-css">` mounting.
  - **Settings → Custom CSS (Main Window)** editor to write, test, and save custom styles directly in `config.toml`.
  - Instant live preview on saving settings without requiring app restarts.
- **Windows Packaging & Release Workflow**:
  - Windows platform support: generated multi-resolution icon bundle (`icon.ico`, `icon.icns`, and full PNG mipmaps) matching Linux desktop icons.
  - GitHub Actions multi-platform release matrix building on both `ubuntu-22.04` (deb, AppImage) and `windows-latest` (MSI and NSIS setup installer).
  - Documentation and prerequisites for Windows 10/11 installation and building from source.

### Fixed

- **AppConfig Struct Integrity**: Resolved duplicate field definitions and missing `whitelist` in default initialization.
- **Tauri IPC Command Registration**: Added missing `set_exclusion_list` and `set_custom_css` handlers to the Tauri command dispatcher.
- **Outdated Documentation**: Corrected version references in package installation examples for Arch Linux (`kodiak-1.1.2-1`) and Debian/Ubuntu (`kodiak_1.1.2-1_amd64.deb`).

## [1.1.1] — 2026-11-10

### Fixed

- **Alternate Provider TLS Root Certificates**: added `rustls-tls-webpki-roots` to `reqwest` in `Cargo.toml`. Previously, requests to external providers (such as Google Gemini at `https://generativelanguage.googleapis.com`) failed during TLS negotiation due to missing Mozilla root certificates in the default `rustls` configuration.
- **Proxy Loopback Bypassing**: configured `no_proxy()` to apply selectively to local test and Ollama endpoints (`127.0.0.1` and `localhost`), enabling external AI endpoints to route cleanly across host network setups.
- **Gemini OpenAI-Compatible Endpoint Headers**: added `x-goog-api-key` header to alternate provider requests to ensure Google Gemini OpenAI compatibility.
- **Detailed Transport Error Reporting**: enhanced reqwest error formatting (`format_reqwest_error`) to extract and report the underlying root-cause chain instead of a truncated top-level message.
- **Safe Makepkg Build Directory Isolation**: updated `Makefile` to pass `BUILDDIR=/tmp/kodiak-makepkg` to `makepkg`, isolating `$srcdir` from the repository root to avoid wiping local `./src`.

### Added

- **Task Stop / Cancel Control**:
  - Interactive **Stop Task** button in the Workbench action bar and Live Event Ticker header.
  - Runtime cancellation checks in `run_execute_stage_with_events` and `execute_queued_pipeline`, allowing immediate abort of active pipelines and background tasks.
  - Cancellation capability across active and waiting queue tasks in the Task Queue drawer.

## [1.1.0] — 2026-11-10

### Added

**Sequential Multi-Task Queue & VRAM-Aware Scheduling (§2.1 Slice 2)**

- Multi-task queue manager (`src-tauri/src/queue.rs`) enabling non-blocking prompt submission while tasks are executing in the background.
- **VRAM Serialization Mutex**: enforces strict serialization of local Ollama model inference across all tasks, preventing GPU Out-Of-Memory crashes on Frank's RTX 4050 6GB Laptop GPU.
- **Intra-Repository Dependency Chaining** (`wait_for_previous_approval`): automatically transitions subsequent tasks on the same project into `WaitingForDependency` until prior action cards are reviewed, preventing code edits against stale file trees.
- Inter-project concurrency: queued tasks targeting different whitelisted repositories can proceed without waiting on unrelated approvals.
- Full queue lifecycle state machine: `Queued`, `Interpreting`, `WaitingForDependency`, `LocalWorking`, `Verifying`, `WaitingApproval`, `Completed`, `Failed`, `Cancelled`.
- Append-safe queue persistence (`~/.local/share/kodiak/queue.json`) and real-time Tauri event notifications (`kodiak://queue-event`).
- Slide-over Task Queue Drawer with live item cards, project directory indicators, status pills, and direct 1-click jump to pending action cards.
- Non-blocking task prompt submission: typing a prompt and pressing Enter or clicking Run instantly enqueues the task, clears the input, and returns focus without freezing the workbench.

**Hardware-Aware Dynamic Profiling (§2.1 Slice 2)**

- Dynamic hardware profiling engine (`src-tauri/src/hardware.rs`) detecting host System RAM via `/proc/meminfo` and dedicated VRAM via `nvidia-smi` / `/sys/class/drm`.
- 4-Tier Hardware Envelope Classification (`Tier1Entry`, `Tier2Mid`, `Tier3Performance`, `Tier4Workstation`) automatically mapping hardware limits to safe, high-throughput Ollama models (`fast_model`, `coder_model`, `heavy_model`).
- Auto-seeding on fresh installations and interactive Settings UI card with one-click model adoption.

**Unified Queue Inbox, Diff Stack Review & Test Hooks (§2.2 Slice 2 Phase B)**

- **Unified Queue Inbox (`src-tauri/src/inbox.rs`, `src/main.ts`)**: aggregates pending action cards across all queued and historical tasks into a single filterable inbox view inside the slide-over Queue Drawer.
- **Diff Stack Overlap & Conflict Detection**:
  - Unified diff hunk header parser (`extract_diff_line_ranges`) extracting inclusive 1-based line ranges `[start, end]`.
  - Conflict detection engine (`detect_conflicts`) comparing proposed diff hunks across queued tasks targeting the same repository and same file path.
  - Automatic collision flagging (`has_conflict = true`) with descriptive collision notes when proposed changes intersect, preventing destructive overwrite of overlapping line edits before approval.
- **Batch Approval & Rejection IPC Commands**:
  - `batch_approve_cards`: sequentially applies approved action cards under Grant Matrix and Deny-List verification, updates audit journal and task records, and synchronizes queue item counts.
  - `batch_reject_cards`: bulk-rejects selected cards with user-specified rationale, records decision audit entries, and skips dependent downstream steps.
- **Post-Approval Local Test Hooks**:
  - Project verification command detector (`detect_project_test_command`) supporting `cargo check` (Rust), `npm test` (JavaScript/TypeScript), `pytest` (Python), and `make test` (C/C++/Makefiles).
  - Post-approval execution hook automatically running project test commands when directory grants satisfy `BUILD` or `AUTONOMOUS` posture.
  - **Local Compiler Log Digestion**: when test commands report compiler errors or warnings, automatically digests terminal logs via local `llama3.2:3b` under `vram_lock` serialization, returning a concise actionable summary.
- **Unified Inbox UI & Filter Controls**: sub-navigation drawer tabs, bulk selection controls (`Select Safe`, `All`, `Clear`), prominent diff collision callouts, and expandable test hook result panels.

**Multi-Project Health & Activity Dashboard (§2.3 Slice 2 Phase C)**

- **Multi-Project Aggregation View (`src-tauri/src/dashboard.rs`, `src/main.ts`)**: dedicated Dashboard view presenting all whitelisted repositories in a clean grid.
- **Git Working Tree Telemetry**: checks `.git` worktrees, displays active branches ('main'), and flags uncommitted files with real-time dirty status badges.
- **Granular Project Posture & Task History**: displays governing grant levels, total task executions, pending review cards, and most recent task verdicts (`PASS` / `FAIL`).
- **One-Click Workspace Actions**: instant switching of the workbench active project ('Set Active'), direct jump to Workbench (`Open Workbench →`), ProjectsWiki documentation access (`Wiki`), and pending card review.
- **Local-First Telemetry & Savings Calculator**: tracks local autonomy ratio (percentage of actions executed under Grant Matrix without cloud tokens), cloud tokens saved via zero-cost local Ollama delegation, and verified task pass rates.

**Projects Wiki Companion Refinements**

- Header typography & branding scale: expanded header height to `3.5rem` with `.md-header__topic` scaled to `2.4rem` to match bear logo height.
- Enhanced desktop text scale across `extra.css` for improved legibility on high-DPI displays.

## [1.0.0] — 2026-11-10

### Added

**Core Orchestrator**

- Native Linux desktop orchestrator built with Rust (Tauri v2), the Tokio async runtime, and TypeScript.
- **Four-Charter Security Model**: local-first isolation, human authority over every action, standby failover, and the Projects Wiki knowledge hub.
- **Granular Grant Matrix** — per-directory permission levels (`READ`, `CREATE`, `BUILD`, `AUTONOMOUS`) that never escalate without explicit session approval.
- **Hard deny-list** protecting SSH keys, cloud credentials, `.env` secrets, shell profiles, system directories, and `.git/` history — globally enforced, even under `AUTONOMOUS` grant.
- **Destructive Diff Interceptor** flagging any patch that modifies or deletes ≥ 40% of an existing file for mandatory review.
- **Proton Lumo integration** (via the local `lumo-tamer` bridge) as the primary zero-access cloud reasoning engine.
- **Standby Failover Cascade** across ordered OpenAI-compatible backup providers (Google Gemini, Groq, OpenRouter), gated behind explicit privacy consent (`cascade.consent`).
- **Interactive Choice Cards** for pivoting to local Ollama execution, retrying an alternate provider, or deferring a run when cascade is disabled or exhausted.
- **Live Model Discovery & Automatic Tier Resolution** — dynamic capability queries (Google-native `ModelService.ListModels`, OpenAI `/models`) mapped into `Low` / `Medium` / `High` execution tiers without hardcoded model lists.
- **Local Delegation pipeline** (`local-delegation_local-ollama`): `llama3.2:3b` for zero-cloud-token context summaries; `qwen2.5-coder:7b` and `qwen2.5-coder:14b` for raw file inspection, diff drafting, and heavy refactoring.
- **Memory Bank** with project-scoped context nodes, `@memory-tag` prompt references, and Memory Chip delivery receipts verifying injected context.
- **Append-only audit journal** (`~/.local/share/kodiak/journal.jsonl`) plus per-run diagnostic logs (`~/.local/share/kodiak/runs/<task-id>/run.log`) with retention controls (`run_log_max_mb`, `run_log_keep_runs`).
- **WebKitGTK desktop UI**: zero-jump dual-RAF scroll persistence, custom slim WebKit scrollbars, collapsible workspace panels, dark theme (`docs/THEME.md`), and full keyboard shortcuts (`Ctrl + Enter`, `Ctrl + Shift + A/D`, `Ctrl + 1–4`, `Ctrl + L`, `Ctrl + W`, `Ctrl + ,`).
- Atomic filesystem writes and sandboxed subprocess execution behind human approval.

**Projects Wiki Companion (`kodiak-projectswiki`)**

- Optional, fully local documentation subsystem: MkDocs Material portal (port 8000) with instant search and auto-categorized navigation (*Hardware & Electronics*, *Embedded & Firmware*, *Software & Utilities*, *Audio & Synthesizers*, *3D Printing & CAD*).
- Sync & Asset REST daemon (port 8765, `sync_api.py`) with Git remote parsing, language/file statistics, hardware asset indexing (datasheets, schematics, Gerbers, photos), and manual-edit preservation via `project_overrides.json`.
- Directory scanner (`wiki.sh sync` / `make_wiki.py`) with dynamic `mkdocs.yml` navigation generation.
- **AI Build Doc** generator (`build_wiki.py` / `wiki-ai`) producing standardized engineering docs (purpose, BOM, pinouts, build/flash instructions, known issues) via local Ollama — zero cloud cost.
- Onboarding wizard detection, port verification, optional whitelist enrollment (`READ` posture), and zero-sudo `wiki.local` hostname mapping with `127.0.0.1` fallback.
- Opt-in systemd user unit (`kodiak-projectswiki.service`) — never enabled behind the user's back.

**Packaging**

- Split Arch `PKGBUILD` producing `kodiak` and `kodiak-projectswiki` packages with dependency hooks (`python-mkdocs-material`, `python-requests`, `python-toml`).
- Debian/Ubuntu `.deb` binary packages with `apt` dependency resolution.

[Unreleased]: https://github.com/morganlowe/Kodiak/compare/v1.3.0...HEAD
[1.3.0]: https://github.com/morganlowe/Kodiak/compare/v1.2.1...v1.3.0
[1.2.1]: https://github.com/morganlowe/Kodiak/compare/v1.2.0...v1.2.1
[1.2.0]: https://github.com/morganlowe/Kodiak/compare/v1.1.9...v1.2.0
[1.1.9]: https://github.com/morganlowe/Kodiak/compare/v1.1.8...v1.1.9
[1.1.8]: https://github.com/morganlowe/Kodiak/compare/v1.1.7...v1.1.8
[1.1.7]: https://github.com/morganlowe/Kodiak/compare/v1.1.6...v1.1.7
[1.1.6]: https://github.com/morganlowe/Kodiak/compare/v1.1.5...v1.1.6
[1.1.5]: https://github.com/morganlowe/Kodiak/compare/v1.1.4...v1.1.5
[1.1.4]: https://github.com/morganlowe/Kodiak/compare/v1.1.3...v1.1.4
[1.1.3]: https://github.com/morganlowe/Kodiak/compare/v1.1.2...v1.1.3
[1.1.2]: https://github.com/morganlowe/Kodiak/compare/v1.1.1...v1.1.2
[1.1.1]: https://github.com/morganlowe/Kodiak/compare/v1.1.0...v1.1.1
[1.1.0]: https://github.com/morganlowe/Kodiak/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/morganlowe/Kodiak/releases/tag/v1.0.0
