# Changelog

All notable changes to **Kodiak** will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

_Nothing yet — planned work will be listed here as it lands._

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
- **Git Working Tree Telemetry**: checks `.git` worktrees, displays active branches (`🌿 main`), and flags uncommitted files with real-time dirty status badges.
- **Granular Project Posture & Task History**: displays governing grant levels, total task executions, pending review cards, and most recent task verdicts (`PASS` / `FAIL`).
- **One-Click Workspace Actions**: instant switching of the workbench active project (`⚡ Set Active`), direct jump to Workbench (`Open Workbench →`), ProjectsWiki documentation access (`📖 Wiki`), and pending card review.
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

[Unreleased]: https://github.com/morganlowe/Kodiak/compare/v1.1.0...HEAD
[1.1.0]: https://github.com/morganlowe/Kodiak/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/morganlowe/Kodiak/releases/tag/v1.0.0
