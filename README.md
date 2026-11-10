<div align="center">
  <img src="src/assets/kodiak-icon.svg" alt="Kodiak Logo" width="96" height="96" />

  # Kodiak

  **Autonomous Desktop Orchestrator & Local-First AI Development Partner for Linux**

  [![Version: v1.0.0](https://img.shields.io/badge/Version-v1.0.0-success.svg)](#)
  [![License: Apache-2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)
  [![Platform: Linux](https://img.shields.io/badge/Platform-Linux_%7C_Arch_%7C_Debian%2FUbuntu-orange.svg)](#installation--packaging)
  [![Tech: Rust | Tauri v2 | TypeScript](https://img.shields.io/badge/Stack-Rust_%7C_Tauri_v2_%7C_TypeScript-red.svg)](#)
  [![AI: Ollama | Proton Lumo](https://img.shields.io/badge/AI-Ollama_%7C_Proton_Lumo-purple.svg)](#)
</div>

---

![Kodiak Workbench Interface](docs/screenshots/kodiak-ui.png)

---

## What is Kodiak?

**Kodiak** is a native Linux desktop orchestrator built with **Rust (Tauri v2)**, the **Tokio** async runtime, and **modern TypeScript**. It bridges high-reasoning cloud synthesis with zero-cost local GPU execution while enforcing strict, cryptographically verifiable filesystem boundaries.

Conventional cloud coding agents and fragile CLI wrappers require blanket filesystem access, execute unverified shell pipelines, and silently stream proprietary source code across third-party APIs. Kodiak eliminates this trade-off: cloud models plan and verify using sanitized metadata, while local models execute all raw file reads, code generation, and diff drafting directly on your hardware. Every action is governed by an explicit permission matrix and recorded to an immutable local audit trail.

---

## Core Principles & The Kodiak Charter

Kodiak enforces a strict **Four-Charter Security Model**:

### 1. Your Files Stay Home (Local-First Isolation)
Kodiak touches nothing outside explicitly whitelisted project directories. Cloud synthesis is strictly isolated to sanitized architectural plans and metadata. Local models (`local-delegation_local-ollama` running `llama3.2:3b` and `qwen2.5-coder:7b`/`14b`) handle raw file inspection, register maps, compiler output parsing, and unified diff reviews at zero cloud token cost.

### 2. Nothing Happens Without You (Human Authority)
File writes, command executions, and deletions require explicit human consent or must match strict, downward-only grant levels (`READ`, `CREATE`, `BUILD`, `AUTONOMOUS`). An unbypassable hard deny-list protects root system directories, SSH keys, cloud credentials, `.env` secrets, shell profiles, and `.git/` history—even when operating in `AUTONOMOUS` mode.

### 3. Standby Failover & Automated Cascade
Kodiak uses **Proton Lumo** (via the local `lumo-tamer` bridge) as its primary zero-access cloud reasoning engine. When cloud quotas are exhausted or rate limits (`429`/`503`) hit, Kodiak never crashes or loses state. It either executes an opt-in **Automated Failover Cascade** across configured standby OpenAI-compatible endpoints (Google Gemini 3.0-Flash / 2.5-Pro, Groq, OpenRouter) or presents interactive **Choice Cards** allowing you to pivot to local Ollama execution (`qwen2.5-coder:7b`, `qwen2.5-coder:14b`) or defer the task.

### 4. Integrated Projects Wiki & AI Knowledge Hub (`kodiak-projectswiki`)
An optional, loosely coupled companion subsystem that transforms scattered hardware, firmware, software, and 3D printing projects into a unified, searchable local knowledge portal. It indexes PDFs, schematics, and manuals, synthesizes standardized documentation via local Ollama models at zero cloud cost, and serves an interactive **MkDocs Material** portal at `http://wiki.local:8000` (with zero-sudo fallback to `http://127.0.0.1:8000`).

---

## System Architecture

Kodiak's backend pipelines decouple reasoning from execution across an asynchronous Tokio runtime, ensuring deterministic state verification at every phase:

```mermaid
flowchart TD
    Prompt["1. User Prompt & Scoped Memory"] --> Plan["2. Cloud Synthesis & Plan (Proton Lumo / Cascade)"]
    Plan --> Enforcer{"3. Enforcer (Grant Matrix & Hard Deny Check)"}
    
    Enforcer -->|Blocked by Deny-List| Reject["Hard Abort & Security Alert"]
    Enforcer -->|Permitted / Scoped| Delegation["4. Local Delegation (local-delegation_local-ollama)"]
    
    subgraph Local Execution Engine
        Delegation --> Summary["Zero-Token Context Summaries (llama3.2:3b)"]
        Delegation --> CodeGen["Raw File Inspection & Diff Drafting (qwen2.5-coder:7b / 14b)"]
        Delegation --> Failover["Standby Cascade / Interactive Choice Card"]
    end
    
    Summary & CodeGen & Failover --> Approval{"5. Action Audit & Human Approval"}
    Approval -->|Approved / Covered by Grant| Apply["Atomic Filesystem Write / Subprocess Exec"]
    Approval -->|Declined| Discard["Discard Diff & Log Rejection"]
    
    Apply & Discard & Reject --> Journal[("6. Append-Only Audit Journal\n(~/.local/share/kodiak/journal.jsonl)")]
```

---

## Feature Highlights

### Granular Grant Matrix & Destructive Diff Interceptor
Permissions are configured per whitelisted directory and never escalate without explicit session approval:
- **`READ`**: Inspect files and propose unified diffs only; every modification queues for manual human approval.
- **`CREATE`**: Includes `READ` plus permission to create new, pre-planned files (never overwrites existing files).
- **`BUILD`**: Includes `CREATE` plus directory creation (`mkdir`) and execution of sandboxed build/test toolchains.
- **`AUTONOMOUS`**: Full read/write/execute capabilities within the target project tree, strictly bounded by the global hard deny-list and flagged by the **Destructive Diff Interceptor** whenever a patch modifies or deletes $\ge 40\%$ of an existing file.

### Standby Failover Cascade & Interactive Choice Cards
Maintain uninterrupted momentum during API outages or rate-limiting events:
- **Automated Cascade**: Configure an ordered list of standby providers. With explicit privacy consent (`cascade.consent = true`), Kodiak automatically routes planning metadata to secondary endpoints upon primary failure.
- **Interactive Choice Cards**: When cascade is disabled or exhausted, Kodiak pauses the pipeline and surfaces an in-app Choice Card to switch to local Ollama inference (`qwen2.5-coder:7b` or `14b`), retry an alternate provider, or defer the run.

### Live Model Discovery & Automatic Tier Resolution
Kodiak dynamically queries provider capabilities without hardcoded model lists:
- **Native Discovery**: Uses `ModelService.ListModels` for Google AI Studio (Gemini) and standard `/models` endpoints for OpenAI-compatible providers (Groq, OpenRouter, Lumo, Ollama).
- **Automatic Tiering**: Live-filters discovered models and maps them into `Low` (fast/economical), `Medium` (balanced), and `High` (deep architectural synthesis) execution tiers.

### Memory Bank & Context Scoping
Persist architectural decisions, hardware pinouts, and coding conventions inside a project-scoped **Memory Bank** (referenced directly in prompts via `@memory-tag`). Delivery receipts (Memory Chips) verify exactly which context nodes were injected into a run, while the append-only decision journal (`~/.local/share/kodiak/journal.jsonl`) and per-run logs (`~/.local/share/kodiak/runs/<task-id>/run.log`) guarantee complete observability.

### Ergonomic WebKitGTK Desktop UI
Engineered specifically for Linux desktop environments:
- **Zero-Jump Scroll Persistence**: Dual-`requestAnimationFrame` (dual-RAF) viewport locking prevents scroll position resets during live streaming updates and background runs.
- **Custom Slim Scrollbars**: Clean dark theme (`docs/THEME.md`) utilizing custom WebKit scrollbars that eliminate bulky GTK stepper collisions.
- **Collapsible Workspace Panels**: Quickly toggle Project Whitelists, Alternate Providers, Memory Bank, and the Local Decision Journal.

---

## Projects Wiki Companion (`kodiak-projectswiki`)

![Kodiak Projects Wiki Interface](docs/screenshots/wiki.png)

For makers, embedded systems engineers, and polyglot developers, project catalogs quickly fragment across disconnected Git repositories, datasheets, CAD files, and loose BOM spreadsheets. Projects often span physical hardware (PCBs, soldering), embedded firmware (ESP32, RP2040, Arduino), 3D printing/CAD, and host software (Rust, Python, C++).

**`kodiak-projectswiki`** solves this as an optional, 100% local companion subsystem. It monitors your project directories, indexes manuals and schematics, and leverages local Ollama models to author and maintain standardized documentation at zero cloud token cost and with zero data exfiltration.

### Dual-Port Architecture & Four-Layer Pipeline

```text
┌────────────────────────────────────────────────────────────────────────┐
│                           KODIAK DESKTOP APP                           │
│  [Workbench]   [Projects Wiki Tab]   [Settings]                        │
└───────────────────────┬────────────────────────────────────────────────┘
                        │ Embeds via iframe or launches in system browser
                        ▼
┌────────────────────────────────────────────────────────────────────────┐
│                   WIKI WEB SERVER (Port 8000)                          │
│  MkDocs Material • Instant Search Index • High-Density Theme           │
│  Accessible at [http://wiki.local:8000](http://wiki.local:8000)  (or [http://127.0.0.1:8000](http://127.0.0.1:8000))      │
└───────────────────────▲────────────────────────────────────────────────┘
                        │ Serves compiled static HTML & indexed assets
                        │
┌───────────────────────┴────────────────────────────────────────────────┐
│                   SYNC & ASSET API (Port 8765)                         │
│  Lightweight local Python daemon (wiki.sh / sync_api.py)               │
│  • Monitors projects_dir configured in ~/.config/kodiak/wiki.toml      │
│  • Manages project_overrides.json (custom titles, pinned tags, icons)  │
│  • Indexes datasheets, schematics, manuals, photos, and Gerbers        │
│  • Serves raw asset previews & status endpoints to Kodiak UI           │
└───────────────────────▲────────────────────────────────────────────────┘
                        │
┌───────────────────────┴────────────────────────────────────────────────┐
│                   LOCAL AI DOCUMENTATION GENERATOR                     │
│  Local Ollama Engine (qwen2.5-coder:7b / llama3.2:3b)                  │
│  • Reads codebases, Git logs, pin definitions, and wiring notes        │
│  • Synthesizes structured Markdown: Overview, BOM, Pinouts, Setup      │
│  • Zero cloud cost, zero third-party data exfiltration                 │
└────────────────────────────────────────────────────────────────────────┘
```

1. **MkDocs Material Documentation Engine (Port `8000`)**: Compiles structured Markdown into a fast static web application featuring instant client-side search, category filtering, a dark slate and burnt orange palette, and high-density typography (`html { font-size: 80%; }`).
2. **Sync & Asset Daemon (Port `8765`)**: A local Python REST service (`sync_api.py`) that discovers new directories, parses Git remotes, calculates language/file statistics, tracks hardware assets, and preserves manual edits and custom tags inside `~/.config/kodiak/project_overrides.json`.
3. **Automated Directory Scanner (`wiki.sh sync` / `make_wiki.py`)**: Recursively traverses your projects root, automatically categorizes repositories (*Hardware & Electronics*, *Embedded & Firmware*, *Software & Utilities*, *Audio & Synthesizers*, *3D Printing & CAD*), and builds the dynamic `mkdocs.yml` navigation tree.
4. **Local AI Documentation Generator (`build_wiki.py` / `wiki-ai`)**: Triggered from the Kodiak UI (**AI Build Doc**) or CLI, it feeds directory structures, code comments, and README excerpts to local Ollama (`qwen2.5-coder:7b`) to generate standardized engineering documentation covering:
   - Project Purpose & Problem Statement
   - Bill of Materials (BOM) & Hardware Specifications
   - Pinout Tables & Wiring Diagrams
   - Build, Flashing, and Execution Instructions
   - Known Issues & Architectural Notes

### Integration, Security Boundaries & Zero-Sudo Fallback
- **Strictly Optional**: Users who only want the core coding orchestrator can install `kodiak` standalone without `kodiak-projectswiki`. When absent or disabled, Kodiak hides the Projects Wiki header tab and status indicators cleanly.
- **Wizard Step 3 Onboarding**: During initial setup, Kodiak detects if `kodiak-projectswiki` is installed, prompts for your projects root, verifies port availability (`8000` and `8765`), and optionally adds discovered repositories into Kodiak's security whitelist under `READ` posture.
- **Zero-Sudo Hostname (`wiki.local`)**: The onboarding wizard offers to map `127.0.0.1 wiki.local` in `/etc/hosts` only with explicit user consent. If skipped or run without root privileges, Kodiak seamlessly falls back to `http://127.0.0.1:8000`.
- **Read-Only Source Boundary**: The wiki subsystem is strictly read-only with respect to your repositories. Generated wiki output lives in `~/.local/share/kodiak/wiki/`, completely decoupled from your source trees.

### Projects Wiki GUI & CLI Operations

| Operation | Kodiak Wiki Tab Button | CLI Command |
| :--- | :--- | :--- |
| **Start Wiki Services** | `Start Projects Wiki Server` | `wiki.sh start` |
| **Rescan & Update Navigation** | `Sync Catalog` | `wiki.sh sync` |
| **Index PDFs & Schematics** | `Rebuild Assets` | `wiki.sh assets` |
| **Generate Documentation via AI** | `AI Build Doc` | `python build_wiki.py --project <name>` |
| **Stop Wiki Services** | `Stop` | `wiki.sh stop` |
| **Open in System Browser** | `External` | `xdg-open http://wiki.local:8000` |

---

## Installation & Packaging

### Arch Linux (and derivatives like CachyOS, EndeavourOS, Manjaro)
Kodiak provides a split `PKGBUILD` producing both the desktop orchestrator (`/usr/bin/kodiak`) and the optional documentation hub (`kodiak-projectswiki`, installed to `/usr/share/kodiak/projectswiki/` with hooks for `python-mkdocs-material`, `python-requests`, and `python-toml`).

Install both packages via `pacman`:

```bash
sudo pacman -U kodiak-1.0.0-1-x86_64.pkg.tar.zst kodiak-projectswiki-1.0.0-1-any.pkg.tar.zst
```

*(To install only the core desktop orchestrator without the wiki, omit `kodiak-projectswiki-1.0.0-1-any.pkg.tar.zst`.)*

Or build and install from source via `makepkg`:

```bash
git clone [https://github.com/bearbuilt/kodiak.git](https://github.com/bearbuilt/kodiak.git)
cd kodiak
makepkg -sfci
```

### Debian / Ubuntu
Install the native `.deb` binary packages using `dpkg` and resolve dependencies via `apt`:

```bash
sudo dpkg -i dist/deb/kodiak_1.0.0-1_amd64.deb dist/deb/kodiak-projectswiki_1.0.0-1_all.deb
sudo apt-get install -f
```

### Systemd User Integration
`kodiak-projectswiki` installs an opt-in systemd user unit at `/usr/lib/systemd/user/kodiak-projectswiki.service`. Packaging scripts never enable it behind your back; start it on demand via the Kodiak UI or enable it with `systemctl --user`:

```bash
systemctl --user daemon-reload
systemctl --user start kodiak-projectswiki.service
```

To enable automatic startup on login:

```bash
systemctl --user enable --now kodiak-projectswiki.service
```

### Building from Source

#### 1. Install Prerequisites
Ensure you have **Rust** (`1.78+`), **Node.js** (`v20+`), **Ollama**, and the **Tauri v2 WebKitGTK (`webkit2gtk-4.1`)** development headers installed.

**Arch Linux:**
```bash
sudo pacman -S --needed base-devel rust nodejs npm webkit2gtk-4.1 openssl curl wget libappindicator-gtk3 librsvg python python-mkdocs-material python-requests python-toml
```

**Debian / Ubuntu:**
```bash
sudo apt update
sudo apt install -y build-essential curl wget libssl-dev libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev nodejs npm python3 python3-requests python3-toml mkdocs
```

#### 2. Pull Required Local Ollama Models
```bash
ollama pull llama3.2:3b
ollama pull qwen2.5-coder:7b
ollama pull qwen2.5-coder:14b
```

#### 3. Compile & Run
```bash
git clone [https://github.com/bearbuilt/kodiak.git](https://github.com/bearbuilt/kodiak.git)
cd kodiak
npm install

# Launch in development mode with hot-reload
npm run tsc && cargo tauri dev

# Build optimized production binaries
npm run build && cargo tauri build
```

The compiled binary is produced at `src-tauri/target/release/kodiak`.

---

## Configuration Reference

### 1. Core Orchestrator Configuration (`~/.config/kodiak/config.toml`)

Kodiak generates a default configuration on first launch. Below is a complete, annotated production reference:

```toml
# ~/.config/kodiak/config.toml
# Kodiak v1.0.0 Core Orchestrator Configuration

[general]
theme = "kodiak-dark"
journal_path = "~/.local/share/kodiak/journal.jsonl"
runs_dir = "~/.local/share/kodiak/runs"
run_log_level = "info"          # Options: "debug" | "info" | "warn" | "error"
run_log_max_mb = 50             # Maximum disk usage for per-run diagnostic logs
run_log_keep_runs = 20          # Number of recent run logs to retain (never prunes journal.jsonl)
destructive_diff_threshold = 0.40

[primary_provider]
name = "lumo"
protocol = "openai-compat"
base_url = "[http://127.0.0.1:3003/v1](http://127.0.0.1:3003/v1)"
tier_low = "lumo-lite"
tier_medium = "lumo"
tier_high = "lumo-max"
timeout_secs = 120

[local_delegation]
pipeline_id = "local-delegation_local-ollama"
ollama_base_url = "[http://127.0.0.1:11434](http://127.0.0.1:11434)"
summary_model = "llama3.2:3b"           # Zero-cloud-token file & log summarizer
worker_model_fast = "qwen2.5-coder:7b"  # Standard local diff & code generation
worker_model_deep = "qwen2.5-coder:14b" # Heavy local refactoring & Choice Card fallback
max_context_tokens = 32768

[cascade]
enabled = true
consent = true                          # Explicit consent to route prompts to standby providers on failure
order = ["gemini", "groq", "openrouter"]
allow_choice_card_on_exhaustion = true

[[alternate_providers]]
id = "gemini"
label = "Google AI Studio (Gemini)"
discovery_mode = "google-native"        # Uses native ModelService.ListModels
base_url = "[https://generativelanguage.googleapis.com/v1beta/openai/](https://generativelanguage.googleapis.com/v1beta/openai/)"
api_key_env = "GEMINI_API_KEY"
tier_low = "gemini-3.0-flash"
tier_medium = "gemini-3.0-flash"
tier_high = "gemini-2.5-pro"

[[alternate_providers]]
id = "groq"
label = "Groq Cloud"
discovery_mode = "openai-models"        # Queries standard /v1/models endpoint
base_url = "[https://api.groq.com/openai/v1](https://api.groq.com/openai/v1)"
api_key_env = "GROQ_API_KEY"
tier_low = "llama-3.1-8b-instant"
tier_medium = "llama-3.3-70b-versatile"
tier_high = "qwen-2.5-coder-32b"

[[alternate_providers]]
id = "openrouter"
label = "OpenRouter"
discovery_mode = "openai-models"
base_url = "[https://openrouter.ai/api/v1](https://openrouter.ai/api/v1)"
api_key_env = "OPENROUTER_API_KEY"
tier_low = "google/gemini-3.0-flash"
tier_medium = "anthropic/claude-3.5-sonnet"
tier_high = "deepseek/deepseek-r1"

# Granular Project Whitelist Matrix
# Valid grant levels: "READ" | "CREATE" | "BUILD" | "AUTONOMOUS"
[[whitelist]]
path = "~/Nextcloud/Projects/kodiak"
grant = "BUILD"
memory_scope = "kodiak-core"

[[whitelist]]
path = "~/Nextcloud/Projects/embedded-firmware"
grant = "CREATE"
memory_scope = "firmware-hardware"

[[whitelist]]
path = "~/Nextcloud/Projects/datasheets"
grant = "READ"
memory_scope = "hardware-docs"

[security]
# Hard deny-list: globally enforced even under AUTONOMOUS grant
hard_deny_patterns = [
  "**/.git/**",
  "**/.env*",
  "**/credentials.json",
  "**/*.pem",
  "**/*.key",
  "~/.ssh/**",
  "~/.aws/**",
  "~/.gcp/**",
  "~/.config/gcloud/**",
  "~/.bashrc",
  "~/.zshrc",
  "/etc/**",
  "/root/**"
]
```

### 2. Projects Wiki Configuration (`~/.config/kodiak/wiki.toml`)

The `kodiak-projectswiki` subsystem reads `~/.config/kodiak/wiki.toml` (alongside custom project metadata in `~/.config/kodiak/project_overrides.json` and compiled output in `~/.local/share/kodiak/wiki/`):

```toml
# ~/.config/kodiak/wiki.toml
# Kodiak Projects Wiki & AI Knowledge Hub Configuration

projects_dir = "~/Nextcloud/Projects"
site_domain = "wiki.local"              # Zero-sudo fallback to 127.0.0.1:8000 if unmapped in /etc/hosts
port_web = 8000                         # MkDocs Material documentation server port
port_api = 8765                         # Python Sync & Asset REST API daemon port
ollama_model = "qwen2.5-coder:7b"       # Primary local model for AI Build Doc synthesis

[paths]
overrides_file = "~/.config/kodiak/project_overrides.json"
output_dir = "~/.local/share/kodiak/wiki"

[scanner]
ollama_base_url = "[http://127.0.0.1:11434](http://127.0.0.1:11434)"
summary_model = "llama3.2:3b"
include_extensions = ["md", "rs", "ts", "py", "c", "cpp", "h", "ino", "toml", "pdf", "svd", "scad", "gbr"]
exclude_dirs = ["target", "node_modules", ".git", "dist", "build", "__pycache__"]
```

---

## Keyboard Shortcuts & Daily Workflow

### Keyboard Shortcuts

| Shortcut | Action |
| :--- | :--- |
| `Ctrl + Enter` | Submit current task prompt to the active planning pipeline |
| `Ctrl + Shift + A` | Approve & Grant the focused Action Card in the queue |
| `Ctrl + Shift + D` | Decline & Discard the focused Action Card diff |
| `Ctrl + 1` | Toggle **Project Whitelists** panel |
| `Ctrl + 2` | Toggle **Alternate Providers & Live Discovery** panel |
| `Ctrl + 3` | Toggle **Memory Bank** panel |
| `Ctrl + 4` | Toggle **Local Decision Journal** panel |
| `Ctrl + L` | Open the **Per-Run Diagnostic Log** viewer (`run.log`) |
| `Ctrl + ,` | Open Global Settings & Cascade Configuration modal |
| `Ctrl + W` | Switch to **Projects Wiki** tab or launch `http://wiki.local:8000` |
| `Esc` | Dismiss active modal or abort an unapproved in-flight prompt |

### Standard Daily Workflow

1. **Scope Your Workspace**: Open the **Projects** sidebar (`Ctrl + 1`), select your target whitelisted directory, and verify its active grant posture (`READ`, `CREATE`, `BUILD`, or `AUTONOMOUS`).
2. **Submit a Task**: Enter your plain-language task in the **Task Delegation Prompt** (referencing relevant hardware pinouts or project specs via `@memory-tag`) and click **Run Pipeline** (`Ctrl + Enter`).
3. **Zero-Token Local Inspection**: Kodiak dispatches `local-delegation_local-ollama` (`llama3.2:3b`) to summarize relevant local files and sends only sanitized structural metadata to Proton Lumo (or your active cascade tier) to synthesize a step-by-step plan.
4. **Local Code Drafting**: Local `qwen2.5-coder:7b` / `14b` workers execute the plan steps, inspect register maps or source trees, and generate unified diffs.
5. **Human-in-the-Loop Review**: Inspect proposed modifications in the **Action Audit Queue**. Review any **Destructive Diff** warnings, then press `Ctrl + Shift + A` to apply atomically. Every grant, rejection, and failover event is permanently recorded in `~/.local/share/kodiak/journal.jsonl`.
6. **Update Engineering Documentation**: Switch to the **Projects Wiki** tab (`Ctrl + W`), click **Sync Catalog** to index new schematics or code changes, and trigger **AI Build Doc** (`qwen2.5-coder:7b`) to keep your local `wiki.local:8000` knowledge base current.

---

## License & Acknowledgements

### License
Copyright © 2026 Bear Built.

Licensed under the **Apache License, Version 2.0** (the "License"); you may not use this software except in compliance with the License. You may obtain a copy of the License at:

[http://www.apache.org/licenses/LICENSE-2.0](http://www.apache.org/licenses/LICENSE-2.0)

Unless required by applicable law or agreed to in writing, software distributed under the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.

### Acknowledgements
- **Bear Built** — Systems architecture, security model design, and Linux packaging.
- **DeepMind Antigravity** — Agentic orchestration research, multi-tier delegation design patterns, and verification workflows.
- **MkDocs Material** — Powering the local `kodiak-projectswiki` engineering documentation portal.
- **Tauri, Ollama & Proton Lumo Communities** — Providing the foundational local-first runtime and privacy-preserving inference bridges.

<div align="center">
  <br />
  <img src="src/assets/bear-built-logo.png" alt="Bear Built Logo" width="140" />
  <p><strong>Engineered by Bear Built</strong></p>
</div>
