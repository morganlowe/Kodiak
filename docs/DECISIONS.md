# Architectural & Implementation Decisions (`docs/DECISIONS.md`)

---

## 1. Local & Cloud Model Integration Strategy (§2)

### Decision: Direct HTTP Calls from Rust with Exact `ollama_delegate_mcp.py` Prompts & Contracts

After inspecting `ollama_delegate_mcp.py` and `local_ai.py` in the local-delegation plugin:

- **Request shaping is straightforward and self-contained**:
  - **Ollama (`/api/generate`, `/api/chat`, `/api/tags`)**:
    - Default options: `{"num_ctx": 16384, "temperature": 0.1}`, `"stream": false`.
    - Model resolution: queries `GET /api/tags` (5s timeout); if the requested model is installed, uses it; otherwise falls back in order: `qwen2.5-coder:7b` → `qwen2.5-coder:14b` → `llama3.2:3b`.
    - Numbered file slicing: reads 1-indexed lines (`start_line..=end_line`, default `1..=1500`) formatted as `L{idx}: {line}`.
    - Tool semantics & prompt templates:
      - `summarize_file`: uses `llama3.2:3b` (`DEFAULT_FAST_MODEL`) with numbered lines to produce constants/globals, 1-indexed function/class line ranges (`L10-L45`), and focus-specific notes.
      - `search_and_summarize`: runs `rg -n --no-heading --color=never -C 1 <pattern> <dir>` (falling back to `grep -rnI -C 1`), caps output to `max_matches` lines (default 250), and summarizes via `llama3.2:3b` preserving `file:line` references.
      - `draft_code`: uses `qwen2.5-coder:7b` (`DEFAULT_CODER_MODEL`) with specification + optional numbered context slice.
      - `digest_log_or_errors`: uses `llama3.2:3b` on the last `tail_lines` (default 400) of log/stderr output.
      - `review_git_diff`: uses `qwen2.5-coder:7b` on `git -C <repo> diff` (truncated at 36,000 chars).
  - **Lumo Bridge (`http://127.0.0.1:3003/v1`)**:
    - Health check: `GET /v1/models` with `Authorization: Bearer <api_key>`.
    - Chat completion: `POST /v1/chat/completions` with `Authorization: Bearer <api_key>`, `{"model": "lumo" | "lumo-lite" | "lumo-max", "messages": [...], "stream": false}`.
    - Crucially, `ollama_delegate_mcp.py`'s `ensure_lumo_server()` directly spawns `node dist/tamer.js server` if offline, which **conflicts with §2.1 & §4.1** of the Foreman brief (*"Do NOT spawn `tamer server` directly as an app subprocess; the unit owns the bridge lifecycle. Foreman interacts with it ONLY via `systemctl --user start lumo-tamer` and the HTTP health check"*).

- **Conclusion**:
  Calling the Ollama and Lumo Bridge HTTP endpoints directly from Rust (`reqwest`) using a dedicated `prompts.rs` module containing the exact prompt constants and routing contracts is cleaner, faster, avoids Python subprocess overhead, and strictly obeys the systemd lifecycle rule in §2.1/§4.1.

---

## 2. Tauri 2 Scaffolding Method (§3.1)

### Decision: Template-Based Layout (`src-tauri/`, `tauri.conf.json`, `src/`)

- System inspection showed `webkit2gtk-4.1`, `gtk3`, `libayatana-appindicator`, `librsvg`, `openssl`, `pkgconf`, `base-devel`, `gcc`, `nodejs`, and `npm` are already installed, whereas `cargo` / `rustc` (`rust` package) and `cargo-tauri` are not yet installed.
- Per §3.1, Foreman uses the clean Tauri 2 minimal template directory structure (`src-tauri/Cargo.toml`, `src-tauri/build.rs`, `src-tauri/tauri.conf.json`, `src-tauri/src/main.rs`, and `src/` with TypeScript + `theme.css`) and prompts the user before installing missing `pacman` packages.

---

## 3. Wiki Tab Mechanics (§2.1 & §6) — Finalized: Live Site Embed (`<iframe>`)

### Decision: Probe Local Wiki Server (`127.0.0.1:80` / `:8000`) + Start Button + `<iframe>` Embed

1. `~/Nextcloud/ProjectsWiki` is an **MkDocs Material** project (`mkdocs.yml`, `wiki.sh`, `make_wiki.py`, `sync_api.py`, `docs/index.md`, `docs/extra.css`, `docs/extra.js`, `docs/projects.json`).
2. `wiki.sh` starts two local HTTP servers:
   - `mkdocs serve` on `127.0.0.1:80` (`http://wiki.local`) or `127.0.0.1:8000` (depending on `net.ipv4.ip_unprivileged_port_start`)
   - `sync_api.py` on `127.0.0.1:8765` (`/api/sync`, `/api/service-status`, `/api/tune`, `/files/...`)
3. Rather than duplicating the Explorer UI and creating maintenance drift:
   - Kodiak probes the live wiki server (`http://127.0.0.1:80` and `http://127.0.0.1:8000`).
   - If offline, the Wiki tab shows a clean "Wiki server offline" state with a **Start Wiki** button that invokes `systemctl --user start projects-wiki.service` (if the user unit exists) or `<wiki_dir>/wiki.sh start`, then polls until reachable.
   - Once reachable, Kodiak embeds the live wiki URL in a full-bleed `<iframe>` so search, Quick Peek modals, and the port-8765 Sync/Tune API work identically to the standalone site.
4. Kodiak's own UI consumes the exact `--wiki-*` tokens extracted from `~/Nextcloud/ProjectsWiki/docs/extra.css` (`#ff6e40` deep-orange accent, `#1e2129` / `#16181d` dark slate surfaces, `12px` / `8px` radii, `#22c55e` OK green) in a single `src/theme.css`.

---

## 4. CachyOS / Arch Packaging & Version Bump Discipline

- **Root `PKGBUILD` & `Makefile`**:
   - Added [PKGBUILD](file://PKGBUILD) and [Makefile](file://Makefile) at the repository root.
   - **Version Single Source of Truth**: `PKGBUILD` dynamically reads `pkgver` from `src-tauri/tauri.conf.json` (both at top-level initialization and in `pkgver()`) so package versions never drift from the app version.
   - **Standard Build Output**: `make build` runs `tauri build` followed by `make pkg` (`makepkg -sf --noconfirm`), placing `kodiak-<version>-1-x86_64.pkg.tar.zst` in both the repo root and `src-tauri/target/release/bundle/arch/` alongside `.deb` and `.rpm`.
   - **One-Command CachyOS Install**: `makepkg -sfci --noconfirm` (or `make install`) compiles, packages, and installs Kodiak on any CachyOS/Arch machine in a single command.

---

## 9. Verify-500 investigation (Slice 2 §1)

### 9.1 Symptom & Captured Failing Task
- **Observed Symptom**: During task `f003f438-0333-448d-9391-17bb2fcfce2b` (`"Create a list of potential tasks to do on a saturday on a rural property. Keep it short."` targeting `~/Nextcloud/Projects/Ai-test/saturday_tasks.md` at `2026-11-07T11:44:05Z`), Stage 1 (`interpret`) succeeded via Lumo Bridge (`http://127.0.0.1:3003/v1/chat/completions`), Stage 2 (`execute`) drafted `saturday_tasks.md` locally on `qwen2.5-coder:7b` (~45 s), and Stage 3 (`verify`) failed at `11:44:52Z` with:
  ```json
  {"error":{"message":"The server encountered an error while processing your request.","type":"server_error","param":null,"code":null}}
  ```
- **Upstream Bridge Journal (`journalctl --user -u lumo-tamer`)**:
  ```text
  Nov 07 05:44:52 Frank tamer[4157290]: [11:44:52] ERROR: Chat completion error
  Nov 07 05:44:52 Frank tamer[4157290]:     error: "TypeError: fetch failed"
  ```
  In `lumo-tamer/src/api/routes/chat-completions/index.ts:236-243`, any exception thrown inside `deps.lumoClient.chatWithHistory()` is caught and mapped via `sendServerError(res)` (`src/api/error-handler.ts:33-45`) to `HTTP 500 Internal Server Error` with `"type": "server_error"`.

### 9.2 Request-Shape Diff: `interpret` vs `verify`
Both stages invoke `call_lumo_chat` (`POST http://127.0.0.1:3003/v1/chat/completions`) with identical JSON envelope structure. Below is the exact request-shape comparison captured for the `saturday_tasks.md` task:

| Dimension | Stage 1: `interpret` | Stage 3: `verify` | Diff / Impact |
| :--- | :--- | :--- | :--- |
| **Endpoint & Method** | `POST /v1/chat/completions` | `POST /v1/chat/completions` | Identical |
| **Model** | `"lumo"` | `"lumo"` | Identical |
| **Message Structure** | `[{"role":"system","content":...}, {"role":"user","content":...}]` | `[{"role":"system","content":...}, {"role":"user","content":...}]` | Identical 2-message array |
| **System Prompt Size** | `1,622 B` (`INTERPRET_SYSTEM_PROMPT`) | `489 B` (`VERIFY_SYSTEM_PROMPT`) | `verify` system prompt is 3.3× smaller |
| **User Prompt Size** | `215 B` (prompt + relative file names) | `1,281 B` (prompt + `TaskPlan` JSON + step summary + 13-line unified diff) | `verify` user prompt is larger (~1.3 KB vs 0.2 KB), well within Lumo's limit |
| **Total Payload Bytes** | `~2,010 B` | `1,986 B` | Virtually identical total byte size (~2 KB) |
| **Temperature / `max_tokens`** | Omitted (bridge default) | Omitted (bridge default) | Identical |
| **Streaming Flag** | `"stream": false` | `"stream": false` | Identical |
| **Timing Relative to Prior Cloud Call** | First call in session (or fresh connection) | Arrives **15–47 seconds after `interpret`** (after local Ollama finishes Stage 2) | **Critical difference**: connection sits idle in Node `undici` pool during Stage 2 |

### 9.3 Root Cause Classification
- **Classification**: **Upstream / Bridge transient connection drop (`TypeError: fetch failed` on stale keep-alive socket)**, not a malformed client payload.
- **Mechanism**: While Stage 2 (`execute`) runs locally on Ollama (`qwen2.5-coder:7b` / `llama3.2:3b`) for 15–47 seconds, `lumo-tamer`'s Node.js `undici` HTTP client holds an idle keep-alive TLS socket to Proton's upstream API (`https://lumo.proton.me/api/ai/v1/chat/completions`). When Proton's edge closes the idle socket near the 30–45s mark, the subsequent `verify` request reuses the half-closed socket, triggering `TypeError: fetch failed` inside `lumo-tamer` and returning `HTTP 500 Internal Server Error` to Foreman.

### 9.4 Fix Implemented
1. **Debug-Level Cloud Request/Response Logger**: Added `CloudRequestDebugEntry` and `append_cloud_debug_log()` in [src-tauri/src/pipeline.rs](file://src-tauri/src/pipeline.rs), logging every `interpret` and `verify` attempt (stage, attempt number, system/user/total payload bytes, status code, elapsed ms, and outcome) to `~/.local/share/foreman/cloud_debug.jsonl` and stderr.
2. **Automatic Single Retry on 5xx / Transient Error (2 s Backoff)**: In `call_lumo_chat()`, if attempt 1 receives any HTTP `5xx` status or transport error, Foreman logs `"http_5xx_retrying_after_2s"` (or `"transport_error_retrying"`), sleeps `2 seconds` (`tokio::time::sleep(Duration::from_secs(2)).await`), and automatically performs a second attempt before surfacing any error to the UI.
3. **Manual Stage-Only Retry Preserved**: The `"Retry Stage 'verify' Only"` button in the error banner remains intact if both attempts fail.

---

## 10. Slice 2 — Feedback, Continuity (v0.2.0)

- **Events**: one `PipelineEvent` enum in `src-tauri/src/events.rs`, emitted on channel `foreman://pipeline-event` (stage changes, ticker lines incl. per-step routing + approval events, draft stream chunks/done).
- **Progress bar / ticker**: updated via targeted DOM patches (no full re-render) so prompt text and scroll survive. Stage-only retries re-enter at the retried stage (later segments reset). Ticker auto-scrolls, pauses on hover, and has a lock toggle.
- **Streaming drafts**: `draft_code` uses Ollama `stream:true`; falls back to non-streaming if the endpoint rejects it. Files are only written after the stream completes, so an Ollama kill mid-stream surfaces an error, offers stage retry, and leaves nothing on disk.
- **History schema v2**: `history.json` is `{schema_version:2, tasks:[...]}` with `title` (from plan goal) and `starred`. Legacy v1 arrays migrate automatically on first load (covered by `test_v1_array_migrates_to_v2_without_data_loss`).
- **Sidebar** replaces the History tab: Projects (grant badges, click to target), Tasks (Favorites, then Today/Yesterday/date), read-only open, "Re-run in same project", delete-with-confirm (history record only; logged as `history_deleted` in `journal.jsonl`). The decision journal moved under Settings.
- **Packaging**: version 0.2.0 → `foreman-0.2.0-1-x86_64.pkg.tar.zst`.
- **Verify-500 acceptance**: 10/10 consecutive live verify calls passed; 5xx auto-retry covered by a mock-server unit test.

---

## 11. Slice 2.1 — Memory Bank & Clipboard Export (v0.3.0)

### 11.1 Memory Bank Architecture & Storage
- **Location**: `~/.local/share/foreman/memory/*.md`
- **Format**: Plain Markdown files with YAML frontmatter:
  ```yaml
  ---
  title: Frank base specs
  tags: [hardware, frank, specs]
  scope: global        # or: project:<path>
  cloud_share: no      # or: yes
  updated: 2026-11-07T13:00:00Z
  ---
  ```
- **External Resilience**: Files can be created, edited, and git-tracked outside Foreman. If frontmatter is missing or corrupt, Foreman falls back safely (`scope: global`, `cloud_share: no`, `title = filename`) without crashing or blocking the UI.

### 11.2 Privacy & Injection Safety Core (§1 Hard Rule Preservation)
- **Local Stages (`draft_code`, `summarize_file`, `digest_log_or_errors`, `review_git_diff`)**: All matched memories' full text is injected locally into `accumulated_local_context` at zero cloud token cost. Never leaves the machine.
- **Cloud Stages (`interpret`, `verify`)**: Default is **catalog-only** (`title`, `tags`, ≤200-char first paragraph excerpt, capped at ~500 tokens total).
- **Explicit `@memory` Expansion**: Full text is sent to cloud stages *only* if explicitly referenced in the user prompt (e.g. `@frank-base-specs`) **AND** `cloud_share: yes` is explicitly declared. Cloud expansion is logged to `journal.jsonl`.
- **Knowledge vs Authority**: Memory context is additive knowledge only. It cannot grant permissions, relax directory whitelists, or override the §4 Grant Matrix / Deny List. Explicit system prompt directives enforce this in both `INTERPRET_SYSTEM_PROMPT` and `VERIFY_SYSTEM_PROMPT`.

### 11.3 Memory UI & Workbench Integration
- **Sidebar Memories Section**: Positioned below Projects; displays entry titles, tag chips, cloud share indicator, and scope badges. Project-scoped memories automatically filter to the active Workbench target.
- **Modals**: Read-only View card and full Edit/Create modal with frontmatter persistence.
- **Quick-Capture**: `+ New Memory` button in Workbench prompt header.
- **Prompt Mention Autocomplete**: Typing `@` in the prompt textarea triggers a floating mention picker listing available memories. Selecting an entry inserts `@slug` and tags the task plan with `memories_used`.

### 11.4 Unified Clipboard Export
- **One-Click Markdown Copy**:
  - Header **📋 Copy Task**: Generates a complete, structured Markdown transcript (Goal, Risk, Target, Plan Steps with Status, Diff Cards, Execution Logs, Cloud Verdict with Rationale, and Journal references).
  - Hover Corner Copy Buttons: On Cloud Plan, Steps, Execution Logs, Action/Diff Cards (preserving exact unified diff formatting `--- a/... +++ b/...`), and Cloud Verdict.
  - Ticker: Dedicated "Copy all lines" button.
- **Styling**: Strictly themed via existing `src/theme.css` tokens (zero new hardcoded hex).

### 11.5 Housekeeping & Packaging Fixes
- **Eliminated `$srcdir` Warning in `makepkg`**:
  - *Root Cause*: `tauri` without `custom-protocol` feature compiled in dev mode, causing `tauri-codegen` to emit `context.with_config_parent(#config_parent)` which baked the absolute host path into the binary.
  - *Fix*: Enabled `custom-protocol = ["tauri/custom-protocol"]` and `default = ["custom-protocol"]` in `src-tauri/Cargo.toml`. The warning is completely eliminated (0 references).
- **Ollama Stream Interruption**:
  - Added unit test `test_ollama_stream_interruption_zero_writes` verifying that premature stream truncation aborts cleanly, surfaces an error, and leaves zero partial files on disk.
- **Arch Packaging**:
  - Package `foreman-0.3.0-1-x86_64.pkg.tar.zst` cleanly generated with no packaging warnings.

---

## 12. Slice 2.2 — Local Timeout Repair & Source Epistemics (v0.3.1)

### 12.1 Diagnosis: Incident 1 (Local Ollama Timeout Mechanics)

#### 1. Exact Location of the 90-Second Limit
In `src-tauri/src/pipeline.rs`:
- Line 522 (`call_ollama_generate`):
  ```rust
  .timeout(Duration::from_secs(cfg.command_timeout_secs.max(90)))
  ```
- Line 585 (`call_ollama_generate_streaming`):
  ```rust
  .timeout(Duration::from_secs(cfg.command_timeout_secs.max(120)))
  ```
In `src-tauri/src/config.rs`, `AppConfig::command_timeout_secs` defaults to `60`.
Consequently, `cfg.command_timeout_secs.max(90)` evaluates to **exactly 90 seconds**.
In `reqwest`, `ClientBuilder::timeout(...)` configures an **overall total duration timeout** covering the entire HTTP transaction from socket connection to reading the final body byte.

#### 2. Scope of Application: All Local Calls vs Streamed Calls
- **Non-streamed local calls** (`summarize_file`, `search_and_summarize`, `digest_log_or_errors`, `review_git_diff`, and non-streaming fallback `draft_code`): use `call_ollama_generate`, triggering a hard abort at **90 seconds** total duration.
- **Streamed local calls** (`draft_code` streaming): use `call_ollama_generate_streaming`, bound by a hard client timeout of **120 seconds** total duration.
Thus, **all** local calls were governed by total duration timeouts that terminated healthy, active generation (even when running at ~71 tokens/sec) once the prompt/response size exceeded the 90s/120s threshold.

#### 3. Interaction with Slice 2.1 Stream-Abort Logic
In Slice 2.1, Foreman added zero-orphaned-writes safety: when `resp.chunk().await` encounters an error, it triggers `DraftStreamDone { success: false }` and aborts the stage without writing partial files to disk.
When reqwest's 90s/120s total timeout fired on an active generation, reqwest abruptly dropped the TCP connection. The stream loop received `Err("error sending request for url: operation timed out")`, leading Foreman to report that Ollama had crashed or failed, even though the Ollama journal proved the server was healthy and generating at ~71 t/s.

#### 4. Remediation Plan
1. **Idle / Inactivity Timeout for Streaming**: Replace the client-level total timeout with a per-chunk idle watchdog (`tokio::time::timeout(Duration::from_secs(local_idle_timeout_secs), resp.chunk())`). The timer resets on every received chunk (default 60s silence threshold).
2. **Generous Total Timeout for Non-Streaming**: Use `local_total_timeout_secs` (default 600s, configurable).
3. **Configuration Keys in `config.toml`**: Add `local_idle_timeout_secs` (default 60) and `local_total_timeout_secs` (default 600).
4. **Honest Error Classification**: Distinguish `connection_refused`, `server_error`, `client_timeout_idle`, and `client_timeout_total` with actionable messages.

### 12.2 Diagnosis & Architecture: Incident 2 (Source Epistemics)

#### 1. Symptom & Root Cause
- **Incident**: In task N, a local model drafted a comparison spec sheet (`frank_base_specs_2026_comparison.md`) which failed cloud verification ("discard this draft — the premise was never verified"). The unverified file remained in the project directory. In task N+1, the Interpret stage treated this file as authoritative project grounding and generated steps to summarize and search it.
- **Root Cause**: The pipeline lacked an epistemic model of its own artifacts. Any file on disk was treated as authoritative ground truth, allowing unverified hallucinations from past tasks to poison future task planning.

#### 2. Architecture & Guardrails
- **Journal Provenance Tracking**: `get_pipeline_generated_files` scans `~/.local/share/foreman/journal.jsonl` for events (`accepted`, `grant_auto_executed`) that wrote files into the target project directory. Files that have received a `user_authored_override` event are excluded.
- **Interpret Prompt Constraints**: `INTERPRET_SYSTEM_PROMPT` explicitly forbids using pipeline-generated files as authoritative domain facts or specifications, unless the user prompt explicitly requests inspection, editing, or auditing of that specific artifact. The list of known pipeline-generated files is passed to the Interpreter.
- **Enforcer Warning Chips**: During stage execution (`run_execute_stage_with_events`), any step reading or summarizing a pipeline-generated file is annotated with `warning_chip: Some("SOURCE: PIPELINE-GENERATED (UNVERIFIED)")`.
- **Cloud Verifier Grounding Scrutiny**: The verify stage receives a prominent "Grounding & Epistemic Warnings" section whenever pipeline-generated files were consulted, prompting the Verifier to scrutinize the output for recycled hallucinations.
- **User-Authored Override**:
  - The UI presents a warning chip on affected steps and diff cards with a `"Mark as user-authored"` button.
  - Clicking the button prompts the user with a modal confirmation explaining that marking the file as human-vetted will allow future tasks to treat it as authoritative grounding.
  - Once confirmed, Tauri command `mark_file_user_authored` records a `user_authored_override` entry in `journal.jsonl`, clearing the restriction for all subsequent pipeline runs.

### 12.3 Verification & Automated Tests
- `test_slow_local_stream_survives`: Validates that a streaming local response with chunks delayed over 1.5 seconds succeeds without premature timeout aborts.
- `test_silent_stream_trips_idle_timeout`: Validates that an inactive socket exceeding the idle timeout threshold is cleanly aborted with a classified `client_timeout_idle` error.
- `test_ollama_connection_refused_classification`: Validates that connection refusal produces a classified error distinguishing unreachable servers from timeouts.
- `test_plan_avoids_pipeline_generated_sources`: Simulates task execution and verifies that files recorded in `journal.jsonl` are correctly identified as pipeline-generated and flagged with warning chips on read/summarize steps.

### 12.4 Packaging & Version Bump
- Bumps version from `0.3.0` to `0.3.1` across `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, and `package.json`.
- Arch Linux package `foreman-0.3.1-1-x86_64.pkg.tar.zst` cleanly generated with zero `$srcdir` leakage or build warnings.

---

## 13. Slice 2.3 — Memory Injection Repair, REQUEST Class & Destructive-Diff Guard (v0.4.0)

### 13.1 Audit of Local-Tool Call Sites for Memory Delivery (Incident A Diagnosis)

Prior to Slice 2.3, memory retrieval in `run_execute_stage_with_events` (`pipeline.rs`) operated globally at the stage level:
```rust
let (matched_memories, _) = match_memories(&all_memories, &plan.goal, project_root);
let local_mem_context = build_local_memory_context(&matched_memories);
if !local_mem_context.is_empty() {
    accumulated_local_context.push_str(&format!("\n{local_mem_context}\n"));
}
```

Audit of all local-tool execution sites:
1. `summarize_file`:
   - *Current Payload*: `{SUMMARIZE_FILE_PROMPT_PREFIX}\n{focus_line}\n{SUMMARIZE_FILE_PROMPT_SUFFIX}\n\n{numbered}`
   - *Memory Received*: **ZERO**. If the step cited `@memory-name` or `memories_used` and targeted an empty/dot path (e.g., "Pull Frank's base specs from memory"), `resolve_canonical_target` resolved to the project directory, `read_numbered_lines` returned empty/directory error, and the 3b model responded with "please share the file".
2. `search_and_summarize`:
   - *Current Payload*: Code search matches from `rg` on `target_path`.
   - *Memory Received*: **ZERO**.
3. `digest_log_or_errors`:
   - *Current Payload*: Log tail from `target_path`.
   - *Memory Received*: **ZERO**.
4. `review_git_diff`:
   - *Current Payload*: Output of `git diff`.
   - *Memory Received*: **ZERO**.
5. `draft_code`:
   - *Current Payload*: `{DRAFT_CODE_PROMPT_PREFIX}\n\nSpecification:\n{instruction}{numbered_ctx}{extra_ctx}` where `extra_ctx` contained `accumulated_local_context`.
   - *Memory Received*: **Partial / Indirect only**. Received memory text *only* if the memory matched `&plan.goal` keywords, prepended generically as "Prior step summaries", without step-level binding or guarantees.
6. `chat_completion` (internal/MCP):
   - *Memory Received*: **ZERO** unless explicitly bundled into the caller prompt.

#### Root Causes Identified:
1. Step-level references (`step.memories_used`, `@name` citations in `step.title` or `step.instruction`) were never resolved or attached to individual steps.
2. The Cloud Interpreter mapped "fetch memory" actions to `summarize_file` with an empty/dot path (`target_path: ""`), causing `read_numbered_lines` on a directory to fail.
3. No enforcement existed at plan-parse time to reject empty-path `summarize_file` calls.

#### Remediation Plan (§1.2 & §1.3):
1. **Implicit Step Memory Delivery**: Any step referencing memories (via `memories_used`, `@name`, or planner citation chips) resolves the full text from local memory storage and prepends it directly to the step's prompt context.
2. **Dedicated Payload Budget**: Configurable `memory_step_budget_chars` (default 8000). If memory exceeds this budget, most relevant sections are included with a truncation notice.
3. **Implicit Memory Retrieval / `memory_fetch` Routing**: If a plan step is dedicated to fetching/summarizing memory, it delivers memory text directly without failing on an empty filesystem path.
4. **Plan Target Validation at Parse Time**: Validate step targets during plan parsing; reject plans where a `READ`/`summarize_file` step has an empty target path and no memory reference delivery.

### 13.2 REQUEST Action Class Architecture & Safety Primacy

Foreman historically lacked a structured mechanism to ASK. When missing access to an external reference or needing clarifying user input, plans attempted unwhitelisted reads or fabricated guesses.

1. **Request Kinds**:
   - `capability`: Requests permission outside the existing project whitelist (e.g., reading an external reference directory or non-whitelisted path).
     - When approved by the user, the grant is appended to `record.task_scoped_grants` on the active `TaskRecord`.
     - `~/.config/foreman/config.toml` is **never modified**; grants expire as soon as the task completes.
   - `information`: Prompts the user with a question or input field.
     - User input is captured and stored locally in `card.user_provided_info` and `record.task_provided_info`.
     - In adherence to §1 privacy rules, user-provided inputs remain local; only a concise excerpt is forwarded to cloud verification.
2. **Strict §4 Deny-List Primacy**:
   - Deny-list rules (root files, `~/.ssh`, `sudo`, destructive commands, dotfiles) take precedence over all requests.
   - Any capability request attempting to access deny-listed paths or execute deny-listed commands is **auto-declined** before reaching the user queue, with an explicit `AUTO-DECLINED (DENY LIST)` chip.
   - Approving an auto-declined card is strictly disallowed by the enforcer.
3. **Skip & Dependency Propagation**:
   - Users can choose to skip an information request. Skipping immediately marks the card as `skipped` and automatically marks subsequent dependent steps as `skipped`.

### 13.3 Destructive-Diff Guard & Inconsistency Note (Incident B)

During code generation tasks, local models occasionally replace large source files (e.g., deleting 265 lines out of 288 and inserting 23 lines) while claiming the edit is a "targeted fix with surrounding context".

1. **Destructive Diff Computation**:
   - Evaluates unified diff line removals relative to total file length.
   - If removal ratio $\ge 40\%$ (`destructive_diff_threshold = 0.40`), Foreman attaches a `CAUTION: LARGE-SCALE DELETION (<n>% of file)` chip unless the plan step explicitly declared `intent: "rewrite"`.
2. **Claim-Consistency Note**:
   - If a step claims to perform a "targeted edit", "fix typo", "corrected code section", or "patch" but exceeds the 40% deletion threshold, an `inconsistency_note` is generated.
   - The inconsistency note is rendered directly on the action card and included in the Cloud Verify prompt so that the verifier can reject deceptive diffs before human approval.

### 13.4 Verification & Zero-Regression Validation

All acceptance and unit tests pass with zero regressions:
- `test_memory_delivered_to_referenced_steps`: Verifies that cited memory text is resolved and delivered to local steps.
- `test_empty_path_plan_rejected`: Rejects empty target paths without memory references at plan-parse time.
- `test_request_capability_task_scoped`: Verifies task-scoped capability grants leave `config.toml` unchanged.
- `test_request_info_provide_and_skip`: Validates provide and skip flows with dependent step skipping.
- `test_request_cannot_override_deny_list`: Asserts deny-list capability requests are auto-declined.
- `test_destructive_diff_flagged`: Validates caution chips on $\ge 40\%$ deletions and suppression on `intent: rewrite`.
- `test_edit_claim_consistency_note`: Validates inconsistency warning generation when targeted claims conflict with large deletions.

---

## 14. Slice 2.4: Memory Delivery Truth, REQUEST Mandate & History Navigation

### 14.1 Memory Consumer Audit (Excerpt vs. Full Body)

An end-to-end audit of every memory consumer across `memory.rs`, `pipeline.rs`, `lib.rs`, and the cloud pipeline was conducted to resolve why local drafting generated fabricated embedded jargon ("deterministic allocation", "ISR contexts") while Cloud Verify reported `@frank-base-specs` as empty:

| Consumer / Call Site | Location | Expected Type | Actual Payload in Slice 2.3 | Defect / Vulnerability |
| :--- | :--- | :--- | :--- | :--- |
| `MemoryEntry::excerpt()` | `memory.rs:39` | Catalog Excerpt ($\le 200$ chars) | Stopped at first non-empty line (38 chars) | Returned `"Host machine specifications for Frank:"` with zero specs. |
| `build_cloud_memory_catalog()` | `memory.rs:307` | Catalog Excerpt | Formatted string of excerpts | Received 1-line truncated excerpt from above. |
| Cloud Interpret Stage | `pipeline.rs:1043` | Catalog Excerpt + Expanded Explicit | Catalog excerpt + full body if explicit `@` in prompt | If user didn't type `@` in prompt, explicit expansion was empty. |
| Cloud Verify Stage | `pipeline.rs:2198` | Catalog Excerpt | Catalog excerpt only | Only saw the 38-char single-line excerpt; failed grounding. |
| `build_local_memory_context()` | `memory.rs:337` | Full Body (`MemoryBody`) | Full body of memories matching `plan.goal` | If `plan.goal` lacked exact keywords, returned empty string. |
| `run_execute_stage_with_events()` (pre-loop) | `pipeline.rs:1113` | Full Body (`MemoryBody`) | `accumulated_local_context` | Only matched on `plan.goal`, not prompt or plan memories. |
| `resolve_step_memories()` | `memory.rs:354` | Full Body (`MemoryBody`) | Bounded full body string | Failed to match when step instruction omitted exact multi-word slug. |
| `summarize_file` (memory fetch) | `pipeline.rs:1456` | Full Body (`MemoryBody`) | `step_mem_payload` | Delivered full body if resolved; otherwise empty. |
| `summarize_file` (file read) | `pipeline.rs:1475` | Full Body (`MemoryBody`) | `numbered` lines + `step_mem_payload` | Allowed directories (`target_path: "."`) causing OS error 21. |
| `search_and_summarize` | `pipeline.rs:1540` | Full Body (`MemoryBody`) | search matches + `step_mem_payload` | Delivered full body if resolved. |
| `digest_log_or_errors` | `pipeline.rs:1585` | Full Body (`MemoryBody`) | log tail + `step_mem_payload` | Delivered full body if resolved. |
| `review_git_diff` | `pipeline.rs:1623` | Full Body (`MemoryBody`) | git diff + `step_mem_payload` | Delivered full body if resolved. |
| `draft_code` | `pipeline.rs:1658` | Full Body (`MemoryBody`) | `instruction` + `step_mem_payload` + `extra_ctx` | When `step_mem_payload` resolved to 0 bytes, system prompt injected ISR jargon. |
| `get_memories` (IPC) | `lib.rs:1030` | Full Body (`MemoryBody`) | JSON `Vec<MemoryEntry>` | Correct for UI Memories tab. |

#### Root Cause Analysis:
1. **Excerpt Single-Line Truncation**: `MemoryEntry::excerpt()` iterated lines and returned on the very first non-empty, non-`#` line. In `frank-base-specs.md`, line 9 was `Host machine specifications for Frank:`, so the excerpt returned only that single header line without any specs.
2. **Silent Zero-Byte Resolution on Local Drafting**: `resolve_step_memories` failed to resolve memories for steps whose instructions didn't contain the exact multi-word string or `@` citation, delivering 0 chars without emitting a warning.
3. **Prompt Contamination**: `DRAFT_CODE_PROMPT_PREFIX` instructed the model to enforce embedded C++ ISR rules even when drafting markdown documents (`.md`). In the absence of memory specs, the local model defaulted to echoing the prompt's embedded jargon.
4. **Lack of Type Distinctions**: Free-form `String` allowed excerpts and bodies to be conflated with no compiler guarantees.
5. **Zero Delivery Observability**: There was no record of delivered memory characters on step execution logs or cards.

### 14.2 Type-Safe Memory Boundaries: `MemoryExcerpt` & `MemoryBody`

To ensure compiler-enforced isolation between cloud and local stages:
- `MemoryExcerpt(pub String)`: Strictly represents cloud-facing condensed catalog excerpts ($\le 200$ chars). Implements `Deref<Target = str>`, `Display`, `Serialize`, `Deserialize`. Excerpts can never be silently passed to local tool payloads.
- `MemoryBody(pub String)`: Strictly represents full memory content for local workers. Implements `Deref<Target = str>`, `Display`, `Serialize`, `Deserialize`, `len()`, `is_empty()`.
- `MemoryEntry::excerpt()` was rewritten to accumulate non-empty, non-`#` lines across paragraphs up to 200 characters, rather than aborting after line 1.

### 14.3 Epistemic Prompt Disambiguation & Memory Delivery Chips

1. **Prompt Disambiguation (`DRAFT_DOC_PROMPT_PREFIX`)**:
   - For `.md`, `.markdown`, and `.txt` files, `draft_code` switches to `DRAFT_DOC_PROMPT_PREFIX`, completely eliminating embedded C++ ISR rules, deterministic allocation, and hardware jargon from general documentation drafting.
2. **Observability via `memory_delivery_chip`**:
   - `StepExecutionLog` and `ActionCard` records now capture `memory_delivery_chip: Option<String>` formatted as `memory: <slugs> (<n> chars delivered)`.
   - Rendered as a distinct visual chip (`.chip-memory`) in the UI using theme tokens (`--wiki-accent`, `--wiki-accent-soft`).
   - If a step references a memory that resolves to empty payload, a live ticker warning is immediately emitted.
   - Cloud Verify stage explicitly receives memory delivery chips in both condensed summaries and grounding warnings, allowing verification to scrutinize delivery veracity.

### 14.4 REQUEST Mandate: Unsourceable External Facts

- **System Prompt Rule 9**: The Cloud Interpreter is strictly mandated to emit an explicit `REQUEST` (information) step for unknowable, external facts (such as competitor specs like Framework 13, external credentials, or arbitrary API keys) before generating drafting steps.
- **Parse-Time Plan Validation**: `validate_plan(&plan, Some(project_root))` checks if any drafting step attempts to write or compare external specifications without a preceding `REQUEST` (information) step or matching memory reference, rejecting ungrounded plans upfront.

### 14.5 Plan-Target Validation (Directory vs. Regular File)

- **Parse-Time Directory Check**: `validate_plan` checks whether file-reading tools (`summarize_file`, `digest_log_or_errors`) target an existing directory path (e.g. `.` or subdirectories). Such plans are rejected immediately with a classified validation error.
- **Execute-Time Directory Guard**: `read_numbered_lines()` verifies `!path.is_dir()` and returns a clean, descriptive error (`Target '...' is a directory, not a file. File-reading tools require a regular file.`) rather than crashing with an unhandled OS error 21.

### 14.6 History Navigation & Workbench Restoration

1. **"Load into Workbench" (`#load-to-workbench-btn`)**:
   - Prefills the prompt input and active project from the historical task record.
   - Clears `readOnlyHistorical` and sets `currentTask = null`.
   - Resets pipeline stages to `idle`.
   - Leaves the workbench prompt editable with the Run button enabled without auto-executing.
2. **Navigation Trap Removal**:
   - Adds persistent `[← Return to Live Workbench]` button in the header and posture bar.
   - Clicking the Workbench navigation tab while in historical mode automatically exits read-only mode and restores the live workbench without requiring a page reload.

### 14.7 Slice 2.4 Regression Test Suite

All 22 unit and integration tests pass with zero regressions:
- `test_body_not_excerpt_reaches_local_steps`: Asserts full memory body reaches local steps and excerpt-only form is rejected.
- `test_unsourceable_facts_force_request_step`: Rejects plans drafting external facts without a prior REQUEST step.
- `test_directory_target_rejected_at_parse`: Asserts file-reading steps targeting directories are rejected at plan validation time.
- `test_deferred_target_validated_at_execute`: Asserts deferred directory targets fail cleanly with directory error instead of OS error 21.

---

## 15. Memory Hygiene: Automatic Write-Time Sanitation (§5 Slice 2.4 Addendum)

### 15.1 Context & Motivation

Memory files created from terminal outputs (such as `neofetch`, `fastfetch`, or raw command logs) frequently contain ANSI color codes, decorative ASCII art, or multi-column layouts where terminal graphics occupy the left column and system specifications occupy the right column. When fed into local or cloud models:
1. Pure ASCII art consumes context tokens while providing zero semantic value.
2. Two-column layouts cause tokenizers to interleave terminal graphics with system specifications.
3. ANSI escape codes pollute prompts and can confuse model instruction parsing.

However, using an LLM to rewrite or clean memory files introduces unacceptable fabrication risk — models hallucinate hardware specs, drop subtle values, or alter technical figures.

### 15.2 Deterministic Heuristic-Only Architecture

To guarantee absolute factual fidelity, Foreman implements a **100% deterministic, local heuristic pass** in `src-tauri/src/memory.rs`:
- **Zero LLM Calls**: Sanitation never invokes cloud or local models; no prompt generation or model rewriting occurs.
- **Trigger & Default**: Runs automatically on every memory create/edit in `save_memory_entry` and `save_memory_entry_with_options`. Default is ON, controlled by `memory_auto_sanitize` in `~/.config/foreman/config.toml` (default `true`) and exposed in the Settings UI.
- **Rolling `.orig` Backups**: Before any rewrite, original bytes are preserved as `<name>.md.orig` beside the file on disk (one rolling backup, updated on subsequent edits).
- **Audit Logging**: Each cleanup emits a one-line note to `journal.jsonl`: `memory_cleaned: <name>, <old> → <new> chars, backup kept`.
- **Zero-Touch Rule**: If nothing is removed, no rewrite occurs, no journal note is emitted, and the file's `mtime` is preserved untouched.

### 15.3 Sanitation Rules & Guarantees

1. **ASCII Art & Box-Drawing Removal**:
   - Consecutive lines where >60% of characters are non-alphanumeric punctuation/symbols (`!c.is_alphanumeric() && !c.is_whitespace()`) are stripped.
   - For two-column system outputs (e.g., neofetch ASCII logo on left, key-value specs on right separated by 4+ spaces), the right-hand text is preserved while stripping the art logo.
2. **ANSI Escape Stripping**:
   - Removes CSI sequences (`\x1b[...m` and related control codes).
3. **Whitespace Normalization**:
   - Collapses 3+ consecutive blank lines down to a single blank line.
   - Trims trailing whitespace on all lines.
4. **Hard Invariant Guarantees**:
   - **Front-Matter Verbatim**: YAML front-matter (`title`, `tags`, `scope`, `cloud_share`, `updated`) is never modified or re-parsed; it remains 100% byte-for-byte identical.
   - **Spec Preservation**: Alphanumeric lines (e.g., `- CPU: AMD Ryzen...`, `OS: CachyOS...`) are never deleted or reordered.
   - **Excessive Removal Safety Guard**: If cleaning would remove >50% of the non-whitespace characters in the memory body, the sanitizer aborts (`SkippedExcessiveRemoval`), leaves the file completely untouched, and logs `memory_clean_skipped: <name>, removal would exceed 50% (<old> → <new> chars)`.

### 15.4 Verification & Live Incident Validation

- **Regression Unit Tests**:
  - `test_sanitizer_strips_art_keeps_specs`: Verifies ASCII box art and ANSI codes are stripped while 100% of specification lines survive.
  - `test_sanitizer_never_touches_frontmatter`: Verifies YAML front-matter remains bit-for-bit identical before and after sanitation.
  - `test_sanitizer_backup_and_skip_behavior`: Validates `.orig` backup creation, disk persistence, and skip behavior on >50% art blocks.
- **Live Incident Validation**:
  - Executed against the live incident file `~/.local/share/foreman/memory/frank-base-specs.md` (2987 bytes).
  - Sanitizer cleanly stripped the CachyOS terminal art logo, preserved all hardware/OS/kernel/desktop lines, created `frank-base-specs.md.orig` with original bytes, and recorded the decision in `journal.jsonl`.

---

## 16. Quota-Aware Degradation & The Cloud-Failure Choice Card (Slice 2.5)

### 16.1 Live Incident Write-Up: The Arduino Nano Disco-Light 500s

During an interactive task (*Arduino Nano disco light*), Stage 1 (`interpret`) failed immediately with:
```json
HTTP 500 Internal Server Error: {"error":{"message":"The server encountered an error while processing your request.","type":"server_error",...}}
```
Foreman's UI treated the failure as a transient error and presented a prominent "Retry" button. Inspection of the bridge service journal (`journalctl --user -u lumo-tamer`) revealed the true upstream failure:
```text
Error: You have reached your weekly quota
```
This revealed three architectural flaws:
1. **Misclassification**: A weekly quota wall was misclassified as a generic 500 server hiccup and offered a retry button. Retrying a hard quota wall is futile, confuses the user, and wastes subsequent requests.
2. **Upstream Masking**: `lumo-tamer` caught all upstream errors inside completion handlers and re-wrapped them unconditionally as `HTTP 500` with generic `server_error` text, hiding rate limit and quota states from consumers.
3. **Absence of Recovery Paths**: Foreman left the user stranded with a binary retry-or-die option instead of presenting proactive fallback choices.

### 16.2 Bridge Patch (`lumo-tamer` Quota Passthrough)

- **Location**: `~/.gemini/config/plugins/local-delegation/vendor/lumo-tamer/src/api/error-handler.ts` and routes under `src/api/routes/chat-completions/` and `src/api/routes/responses/`.
- **Classification**: When an upstream error message or status matches quota or rate-limit semantics (`quota`, `rate limit`, `usage limit`, upstream 429, or 403-with-quota-text), `lumo-tamer` responds with **HTTP 429** and:
  ```json
  {"error":{"message":"<upstream text>","type":"quota_exhausted","param":null,"code":"quota_exhausted"}}
  ```
  All other upstream errors retain standard 500 wrapping.
- **Logging Preservation**: Raw upstream errors continue to be written verbatim to the systemd user journal (`journalctl --user -u lumo-tamer`).
- **Test Method Documentation**:
  Because the user was actively over quota on the free-tier Lumo bridge during Slice 2.5 implementation, verification was conducted live without consuming artificial quota:
  ```bash
  curl -i -X POST http://127.0.0.1:3003/v1/chat/completions \
    -H "Authorization: Bearer $(cat ~/.config/foreman/config.toml | grep lumo_api_key | cut -d'\"' -f2)" \
    -H "Content-Type: application/json" \
    -d '{"model":"lumo","messages":[{"role":"user","content":"ping"}]}'
  ```
  The bridge returned `HTTP/1.1 429 Too Many Requests` with payload `{"error":{"message":"You have reached your weekly quota","type":"quota_exhausted","param":null,"code":"quota_exhausted"}}`. Journal inspection confirmed raw message logging intact.

### 16.3 Foreman Quota Classification & Choice Card Architecture

1. **Dual-Detection Classification**:
   - Primary: HTTP 429 or `type/code == "quota_exhausted"` from patched `lumo-tamer`.
   - Defensive fallback: HTTP 500 bodies containing quota/rate-limit substrings (guards against unpatched bridge installations).
2. **Retry Suppression**: When `stage_error_kind == "quota_exhausted"`, the Retry button is suppressed. The live ticker displays: *"Cloud quota exhausted — waiting for your decision"*.
3. **Three-Path Choice Card**:
   - **[Defer task]**: Moves the task into the persistent `deferred` queue with reason `"Waiting for cloud availability"`.
   - **[Continue with local interpret / verify]**: Executes the failed cloud stage locally using Ollama's heavy model (`qwen2.5-coder:14b`). Attaches a persistent warning badge to the resulting plan or verdict: `LOCAL-GRADE PLANNING (cloud unavailable)` or `LOCAL-GRADE VERIFICATION (cloud unavailable)`. Preserves §1 privacy guarantees (zero bytes sent to external cloud).
   - **[Use alternate provider]**:
     - Configured via `[[alt_provider]]` in `config.toml` (Settings tab).
     - Hidden when unconfigured, replaced with a helpful hint to configure one in Settings.
     - **Privacy Consent Gate**: Foreman's §1 guarantee ("only Proton sees your data") does not apply to non-Proton services. Using an alternate provider sends user prompts and plan context across the internet to an external provider. To uphold safety and trust, Foreman enforces an explicit one-time session consent checkbox before any alternate provider invocation will execute.
     - Audit logging: All provider transitions and usage are recorded in `journal.jsonl` with provider names (`stage_provider: stage=interpret, provider=Gemini-OpenAI`).

### 16.4 Defer Queue & Availability Probing Architecture

1. **Partial Progress Preservation**:
   - A task that fails at `interpret` defers as a whole.
   - A task that completes `execute` and subsequently fails at `verify` defers **only** the verify stage (`deferred_stage: "verify"`). Resuming re-runs verify only; paid-for execution and edits are never re-run.
2. **Workbench Sidebar Integration**:
   - Prominent `Deferred (N)` section above the main task history with stage indicator and direct "Resume" button.
3. **Non-Blocking Availability Probe**:
   - On application startup and each cloud-stage completion, Foreman probes cloud health (`probe_cloud_status`).
   - Throttled in Rust to at most once per 5 minutes to avoid polling overhead.
   - If available and deferred tasks exist, displays a non-blocking toast: *"Cloud is back — N deferred task(s) ready. [Review] [Later]"*.
   - **Zero Auto-Run Invariant**: Cloud probe success surfaces notifications only; resumption strictly requires an explicit user click.

### 16.5 Verification Suite

All 8 named regression tests pass with zero regressions:
- `test_quota_error_classified_not_server_error`: Both detection paths (429 and 500-with-quota-text) classify as `quota_exhausted`.
- `test_no_retry_button_on_quota`: Quota classification suppresses retry action.
- `test_choice_card_local_interpret_routes_to_ollama`: 14b model is selected and local-grade badge is attached to plan.
- `test_alt_provider_requires_config_and_consent`: Prevents invocation without config, blocks execution without consent, and journals provider name upon consent.
- `test_defer_parks_partial_progress`: Verify stage failure defers verify only; execute work is preserved.
- `test_deferred_tasks_persist_restart`: Deferred state survives roundtrip history serialization.
- `test_resume_requires_manual_click`: Cloud probe success notifies without auto-running.
- `test_cloud_probe_throttled`: Probes within 5 minutes return throttled status and do not duplicate network pings.



---

## 17. Kodiak v0.6.0: Rename, Onboarding, REQUEST Clarity & GitHub Readiness

### 17.1 Rename Rationale: Foreman → Kodiak

1. **Package Name Collision**:
   The package name `foreman` collides with Red Hat's long-standing infrastructure management tool (The Foreman Project). Users installing on Arch Linux or querying AUR/system package managers encountered confusion and phantom package updates.
2. **Identity**:
   Renamed to **Kodiak** — the bear who runs the job site.
3. **Packaging Clean Swap**:
   `PKGBUILD` specifies `pkgname=kodiak`, `conflicts=('foreman')`, and `replaces=('foreman')`. Pacman cleanly swaps existing installations, installs binary as `/usr/bin/kodiak`, and registers desktop launcher `/usr/share/applications/kodiak.desktop`.
4. **Data Contract Invariant**:
   To ensure complete backwards compatibility and flawless audit replay, internal data-format contracts (`history.json` schema v2, `journal.jsonl` action types like `cloud_probe`, `memory_cleaned`, `request_granted`) remain strictly untouched. Only human-facing strings and branding were renamed.

### 17.2 First-Launch Atomic Zero-Data-Loss Migration

1. **Atomic Filesystem Rename**:
   On startup, if `~/.local/share/kodiak/` does not exist but `~/.local/share/foreman/` does, Kodiak performs an atomic rename (`std::fs::rename`) to the new location. The same is done for `~/.config/foreman` → `~/.config/kodiak`.
2. **Never Merge, Never Overwrite**:
   If both old and new directories exist simultaneously, Kodiak logs a warning and does not touch or overwrite existing directories.
3. **Clean Abort on Failure**:
   If any rename operation fails (e.g. permission restriction), Kodiak immediately aborts, rolls back already-moved directories to their previous state, and terminates startup with a clear error naming the exact path and OS reason, ensuring zero partial-state migration.
4. **Audit Journaling**:
   A `migration_from_foreman` action is recorded in the new `journal.jsonl` logging all migrated paths.
5. **Regression Verification**:
   - `test_migration_moves_dirs_atomically`
   - `test_migration_never_merges_existing_dirs`
   - `test_migration_aborts_cleanly_on_failure`

### 17.3 REQUEST Card Phrasing & Strict Validation

1. **Incident Background**:
   Live testing during Slice 2.5 surfaced a confusing card: *"Request project requirements and hardware details / Target: ."*. The user could not parse what information was needed or why a file target of `.` was shown.
2. **Validation Rules in `validate_plan`**:
   - REQUEST steps must not target files; `target_path == "."` or `./` is strictly rejected at plan parse.
   - Every REQUEST (information) step must contain:
     - Exactly ONE concrete question ending with `?` (e.g. *"Which MCU board and pins should the sketch target?"*).
     - An `answer_format` string indicating expected format (e.g. *"comma-separated pin list"*).
     - An optional `suggested_default` when the planner can infer reasonable defaults.
3. **UI Enhancements**:
   - Empty/non-file targets are hidden on REQUEST cards.
   - The question and expected format are rendered prominently.
   - When `suggested_default` is present, a one-click `[Use suggestion]` button instantly pre-fills the answer textarea.
4. **Regression Verification**:
   - `test_request_card_has_question_and_format`
   - `test_request_empty_target_rejected`

### 17.4 Provider Onboarding & Connection Testing

1. **Settings Setup Guides**:
   Added collapsible setup guides in Settings detailing:
   - Lumo via `lumo-tamer` on `http://127.0.0.1:3003/v1` (primary zero-access cloud).
   - Any OpenAI-compatible alternate endpoint (Ollama LAN, Groq, OpenRouter).
   - Privacy boundary disclaimer explaining that §1 privacy rule applies to Proton primary, while alternate providers receive task prompt/plan context.
2. **Live Connection Test Tool**:
   - Backend endpoint `test_alt_provider_connection` pings the provider with a 1-token test query (`"ping"` / `max_tokens: 1`).
   - Reports exact HTTP status code, round-trip latency in milliseconds, and detailed error messages.
   - `test_provider_test_connection_reports_http_code` verifies connection diagnostics.

### 17.5 GitHub Readiness

1. **Licensing**: Added Apache License 2.0 (`LICENSE`).
2. **Git Hygiene**: Comprehensive `.gitignore` covering Node, Rust/Tauri, Arch packaging, and sensitive runtime logs.
3. **Configuration Template**: `config.example.toml` with commented examples and default settings.
4. **Documentation**: Overhauled `README.md` with system architecture, §1 privacy rule, Arch packaging instructions, backend configuration guides, and screenshot reference.

---

## 18. Slice 2.7: Model Tiers, Provider Cascade, Run Diagnostics, About Page & The Charter (v0.7.0)

### 18.1 Provider Tiers & First-Class Gemini Preset
1. **Model Tiers**:
   - Added an optional `tier` field (`low | medium | high`) to `AltProviderConfig` and `AppConfig` (`tier` / `cloud_tier`).
   - The primary cloud model selector presents Lumo models as tiers (Standard/Medium = `lumo`, Lite/Low = `lumo-lite`, Max/High = `lumo-max`).
   - UI features a two-level picker (Provider → Tier) with a free-text model field pre-filled from the tier's default. Kodiak never guesses API model IDs; the user always sees and can override the exact model string that will be sent.
2. **Add Provider Presets**:
   - Presets pre-fill endpoint details for Lumo (`http://127.0.0.1:3003/v1`), Gemini OpenAI-compat (`https://generativelanguage.googleapis.com/v1beta/openai/`), Groq, OpenRouter, and Self-hosted Ollama.
   - Gemini is treated as a first-class preset with tiered model options (`gemini-3.0-flash` for low, `gemini-1.5-pro` for high).
3. **Regression Tests**:
   - `test_tier_field_roundtrips_config` verifies TOML roundtripping of provider tiers.

### 18.2 Privacy-Governed Provider Cascade (Ordered Failover)
1. **Architectural Rationale & Consent Requirement**:
   - Automatic failover sends task prompts to standby providers without per-request confirmation. Because third-party providers do not inherit Proton Lumo's zero-access privacy guarantee (Charter Tenet 1), automatic failover is **disabled by default** and requires explicit user consent (`cascade.consent = true`).
2. **Cascade Scope**:
   - Cascade only triggers for retryable cloud errors or quota exhaustion (HTTP 429 / 503).
   - Each cascade attempt is journaled to `journal.jsonl` as `cascade_attempt: stage=<stage> provider=<name> error=<code>`.
   - **Local-interpret and Defer are strictly excluded from automated cascade**: degrading to local models or pausing work remains an explicit human choice on the Choice Card upon cascade exhaustion.
   - Tasks executed via cascade providers receive full cloud grades (no `LOCAL-GRADE` badge).
3. **Regression Tests**:
   - `test_cascade_disabled_preserves_current_behavior`
   - `test_cascade_order_followed_and_journaled`
   - `test_cascade_exhaustion_shows_choice_card`
   - `test_cascade_requires_consent`

### 18.3 Per-Run Diagnostic Logs & Retention
1. **Concurrent Logging**:
   - Every task run streams structured logs to `~/.local/share/kodiak/runs/<task-id>/run.log` concurrently with the Live Event Ticker (capturing timestamps, stage transitions, model routing, memory chips, error classifications, and cascade attempts) along with sidecar `meta.json`.
2. **Retention & Safety**:
   - Configurable retention: `run_log_level` (`debug|info|warn|error`), `run_log_max_mb` (default 50), and `run_log_keep_runs` (default 20).
   - Pruning removes oldest run logs only; `history.json` and task records are immutable and untouched during pruning.
3. **In-App Viewer**:
   - Read-only monospace themed modal with "Copy All" and visual notices when logs were pruned.
4. **Regression Tests**:
   - `test_run_log_captures_ticker_events`
   - `test_run_log_level_filters_debug`
   - `test_run_log_rotation_prunes_oldest` (verifies `history.json` remains untouched).

### 18.4 Verifier Prompt Strengthening & The Keyboard-Backlight Incident
1. **The Exemplar Incident**:
   - During live hardware testing on an Arduino sketch, a queued-review diff hallucinated and injected an unrelated "keyboard backlight" feature containing two nonexistent functions (`initBacklight()`, `setBacklightBrightness()`). The previous verifier evaluated syntax and overall goal but failed to catch that unrequested functionality was introduced.
2. **Hardening `VERIFY_SYSTEM_PROMPT`**:
   - Added `CRITICAL IRRELEVANT-CONTENT RULE`: The verifier MUST fail any diff containing code or functionality not requested by the task goal (even if syntactically correct). The verifier must cite the irrelevant element explicitly.
3. **Regression Coverage**:
   - `test_verifier_flags_unrelated_content` (instruction-shape test). Note honestly: full behavioral validation of this prompt instruction across arbitrary LLMs will continue during live runs.

### 18.5 The Charter & Cross-Linking Architecture
1. **The Charter as Single Source of Truth**:
   - Kodiak's operating principles are formalized in `src/charter.ts` as six plain-language tenets with concrete mechanical enforcement mechanisms:
     - Tenet 1: Your files stay home (privacy)
     - Tenet 2: Nothing happens without you (human authority)
     - Tenet 3: Some things are always no (hard limits)
     - Tenet 4: Honest limits (epistemics)
     - Tenet 5: Local first (design bias)
     - Tenet 6: A paper trail for everything (auditability)
2. **Replacing Bare Jargon**:
   - Replaced cryptic section references ("§1", "§4.2", etc.) across the UI with clear human phrases and interactive `renderCharterChip` pills that link directly to the Charter in the About tab and scroll smoothly to the corresponding tenet.
3. **About Tab**:
   - Added an About tab displaying the Bear Built logo, app name/version ("Kodiak v0.7.0"), live system status (Lumo bridge, Ollama models, active project, uptime), links/licenses (Apache-2.0 and Bear Built mark attribution), and The Charter.

---

## 19. ProjectsWiki Strip-Down & Optional Component Architecture (Rev 2)

### 19.1 Git History Finding & Clean Publication Path
- **Audit Findings**:
  - `ProjectsWiki/` within `Aureus-AI-Foreman/` was completely untracked in git history (`git log --all --full-history -- ProjectsWiki` returned 0 commits).
  - The repository has only 2 existing commits (`85b6a95` Initial commit, `6f606b5` README screenshot update), neither of which contains any personal project files, photos, PDFs, or generated docs from `ProjectsWiki`.
  - **Decision**: No history filtering (`git filter-repo` / BFG) is required. The stripped component can be committed directly to git.

### 19.2 Component Split & Packaging Layout
- **Decision: Split Packaging (`kodiak` + `kodiak-projectswiki`)**:
  - The stripped wiki is decoupled into an optional sub-component installed to `/usr/share/kodiak/projectswiki/`.
  - In Arch Linux (`PKGBUILD`), `kodiak` defines `optdepends=('kodiak-projectswiki: Local Projects Wiki and AI documentation hub')`.
  - In Debian/Ubuntu, a separate binary package `kodiak-projectswiki` is generated with `Recommends: kodiak-projectswiki` on the main `kodiak` package.
  - When the component is omitted or disabled, Kodiak removes the Projects Wiki button from the navigation bar with no broken dependencies or orphan background services.

### 19.3 Configuration Schema & Externalized Overrides
- **Externalized Configuration (`~/.config/kodiak/wiki.toml`)**:
  - `projects_dir`: User-configurable directory for project repositories (defaults to `~/Projects`, prompted during setup). Eliminates hardcoded `~/Nextcloud/Projects` references.
  - `site_domain`: Defaults to `wiki.local`, with zero-sudo fallback to `http://127.0.0.1:8000` when `/etc/hosts` is unconfigured.
  - `ollama_url` / `ollama_model`: Connects to local Ollama (default `http://127.0.0.1:11434`, `qwen2.5-coder:7b`).
  - `port_web` / `port_api`: Defaults to 8000 and 8765.
  - `overrides_file`: Externalized to `~/.config/kodiak/project_overrides.json` so manual title, category, and overview edits survive package upgrades, catalog rescans, and documentation rebuilds.
- **Taxonomy Defaults**:
  - Neutral categories shipped by default: `Hardware & Electronics`, `Embedded & Firmware`, `Software & Tools`, `Robotics & Mechanical`, `Audio & Media`, and `General & Prototypes`. Fully customizable via `wiki.toml`.

### 19.4 Strict Read-Only Boundary & Pipeline Isolation
- **Boundary Tenet**:
  - The ProjectsWiki microservice (`sync_api.py`) and MkDocs viewer are strictly read-only with respect to Kodiak's workbench pipeline and task execution engine.
  - The wiki cannot inject tasks, modify the security whitelist, or alter active execution state.
  - The live production wiki at `~/Nextcloud/ProjectsWiki` remains untouched (baseline checksum verified before and after).

### 19.5 Zero-Sudo Port & Host Hygiene
- **Decision: Removal of Automated Port-80 Sysctl**:
  - Automated modification of `net.ipv4.ip_unprivileged_port_start=80` is removed from all default execution flows.
  - `/etc/hosts` and sysctl port 80 configuration are strictly opt-in behind interactive confirmation during `wiki setup`.
  - If unconfigured, the wiki binds to `127.0.0.1:8000` without requiring root permissions.

---

## 20. Alternate Provider Model Dropdowns, Editing & UX Continuity (v1.0.1 / Post-v1.0 Polish)

### 20.1 Dynamic Alternate Model Dropdown & Gemini 3.0 Defaults
- **Context & Issue**:
  - Google AI Studio deprecated `models/gemini-2.5-flash` with HTTP 404, recommending `gemini-3.0-flash`.
  - The previous model field was a plain text input that defaulted to the obsolete string and required typing exact API identifiers.
- **Decision**:
  - Introduced dynamic model dropdowns (`#new-alt-model-select`) alongside the model text input (`#new-alt-model`) for all presets (Gemini, Lumo, Groq, OpenRouter, Self-hosted Ollama).
  - Pre-wired recommended, verified models per preset:
    - **Gemini**: `gemini-3.0-flash` (Recommended Low/Standard), `gemini-1.5-pro` (High/Deep Reasoning), `gemini-2.0-flash`, `gemini-1.5-flash`, `gemini-2.0-flash-lite`.
    - **Lumo**: `lumo`, `lumo-lite`, `lumo-max`.
    - **Groq**: `llama-3.3-70b-versatile`, `llama-3.1-8b-instant`, `mixtral-8x7b-32768`.
    - **OpenRouter**: `anthropic/claude-3.5-sonnet`, `openai/gpt-4o`, `google/gemini-2.0-flash-001`, `meta-llama/llama-3.3-70b-instruct`.
    - **Ollama**: `qwen2.5-coder:7b`, `llama3.2:3b`, `qwen2.5-coder:14b`, `deepseek-r1:7b`.
    - **Custom option**: Allows entering arbitrary model identifiers for zero vendor lock-in.
  - Two-way binding: selecting a dropdown option synchronizes the text input; typing a custom model switches the dropdown to "Custom Model ID...".
  - Synchronized tier selection (`Low | Medium | High`) to automatically pick corresponding models (`gemini-3.0-flash` for low/medium, `gemini-1.5-pro` for high).

### 20.2 In-Place Alternate Provider Editing
- **Context & Issue**:
  - Users could previously only add or delete alternate providers. Modifying an endpoint, API key, model, or tier required deleting and re-entering from scratch.
- **Decision**:
  - Added an "Edit" button (`.edit-alt-provider-btn`) to every alternate provider card.
  - Clicking "Edit" populates the provider form with existing values, adjusts the header to "Edit Alternate Provider (Name)", switches the primary button to "Save Provider", and provides a "Cancel Edit" button.
  - Saving updates `config.alt_provider[idx]` in-place and preserves/updates cascade order references if renamed.

### 20.3 Zero-Scroll-Reset Across DOM Re-Renders
- **Context & Issue**:
  - In Kodiak, `renderApp()` sets `appEl.innerHTML = ...`. Replacing the DOM destroyed `.main-content` and `.sidebar__scroll`, resetting their scroll positions (`scrollTop = 0`). Any incoming message, status update, banner error, or settings interaction caused the viewport to jump to the top.
- **Decision**:
  - Prior to `appEl.innerHTML = ...`, `renderApp()` records `prevMainScroll`, `prevSidebarScroll`, and a map of `id -> scrollTop` for all scrolled containers.
  - Immediately following innerHTML insertion, scroll positions for `.main-content`, `.sidebar__scroll`, and all matching IDs are synchronously restored.

### 20.5 Live Model Discovery & Collapsed Project Retention
- **Context & Issue**:
  - Google AI Studio deprecated `models/gemini-2.5-flash` with HTTP 404, pointing developers to `models/gemini-3.0-flash`. Furthermore, `gemini-3.0-pro` is non-existent in Google v1beta API, causing 404 errors when selected.
  - In the sidebar, collapsing the project list previously hid all projects completely, leaving no indication of which project is currently active.
- **Decision**:
  - Implemented backend Tauri command `list_provider_models` (`pipeline::fetch_provider_models`) which queries live model endpoints:
    - Google Generative Language `ModelService.ListModels` (`https://generativelanguage.googleapis.com/v1beta/models?key={api_key}`), filtering for `generateContent` methods and excluding non-text embeddings/imagen, sorting `gemini-3.0-flash` and `gemini-2.5-pro` at the top and filtering out deprecated `2.5-flash`.
    - Standard OpenAI-compatible `/models` endpoint (Groq, OpenRouter, Ollama, Lumo) with Bearer token authentication.
  - Added frontend `↻ Fetch Live Models` button with automatic background querying on API key blur.
  - In `renderSidebar()`, when `sidebarProjectsCollapsed` is true, the active project is preserved on the left with its label, accent highlight, and grant badge (`BUILD`/`READ`).

### 20.7 Alternate Provider Form Draft Persistence & Responsive Layout
- **Context & Issue**:
  - In Kodiak Settings, the "Add Alternate Provider" form used a single-row auto-fit grid (`minmax(180px, 1fr)`) that stretched 8 controls into one narrow, squashed horizontal line on ultrawide displays.
  - Furthermore, when querying live models via `fetchLiveModelsForCurrentForm()`, the handler called `renderApp()`, wiping uncommitted DOM input values (Name, URL, API Key) and resetting them to empty placeholders, preventing the user from adding or saving providers.
  - An automatic fetch event on API key blur caused unexpected form wiping during entry.
- **Decision**:
  - **In-Place Live Model Fetch**: Removed all `renderApp()` calls from `fetchLiveModelsForCurrentForm()`. Status badges, button states, and model dropdown options are updated directly in the DOM.
  - **Form Draft State**: Added `state.altProviderFormDraft` which synchronizes with user inputs on every `input` and `change` event, ensuring form data survives any background re-renders.
  - **Structured Card Layout**: Replaced the 1-row squashed strip with `.alt-provider-form` comprising:
    - Row 1: Preset selector, Name, Tier, and Max Context Chars.
    - Row 2: Base URL and API Key (50/50 split).
    - Row 3: Model ID selector with clean `[↻ Fetch Live Models]` button placement and status badge.
    - Row 4: Dedicated right-aligned `[+ Add Provider]` / `[Save Changes]` action buttons.
  - **Explicit Fetching**: Removed blur-triggered auto-fetch so live discovery runs only when the user explicitly clicks `↻ Fetch Live Models`.

### 20.8 Universal Collapsible Lists & ProjectsWiki Scale Reduction (~20%)
- **Context & Issue**:
  - As projects, memories, and audit logs accumulate, long lists (such as the Local Decision Journal with dozens or hundreds of lines, sidebar memories, and tasks) pushed panels down and cluttered the interface.
  - MkDocs Material on wide screens automatically inflated `html { font-size: 125%; }`, causing typography and UI components on the Projects Wiki to appear approximately 20% too large.
- **Decision**:
  - **Collapsible Decision Journal**: Added `journalCollapsed: boolean` (default `true`) to `AppState`. When collapsed, a compact single-line card shows the latest audit action, count, and click-to-expand toggle.
  - **Collapsible Sidebar Sections**: Added click-to-collapse headers (`sidebarMemoriesCollapsed` and `sidebarTasksCollapsed`) with item counts and directional indicators (`▼` / `▶`).
  - **Wiki Scale Reduction**: Added root base scaling rules in `ProjectsWiki/docs/extra.css`:
    - `html { font-size: 80% !important; }` across all viewports (`82%` on ultrawide).
    - Tightened `rem`-based typography and padding across headings, search bar, navigation links, and explorer controls by ~20%.

### 20.9 MkDocs Live-Reload Loop Resolution & Process Coordination
- **Context & Issue**:
  - In testing the Projects Wiki, browsers experienced continuous page flashing and refreshing every 1–2 seconds.
  - Root cause investigation revealed two intertwined issues:
    1. A systemd user unit (`projects-wiki.service`) had been enabled with `Restart=on-failure` and `RestartSec=3`. Concurrently, an existing background instance was already running and listening on port 80/8000.
    2. When systemd executed `wiki.sh daemon`, it ran `sync_catalog` -> `make_wiki.py`, which unconditionally rewrote `mkdocs.yml` and `projects.json` even when content was identical.
    3. The updated mtime on `mkdocs.yml` triggered the running `mkdocs serve` watcher to rebuild and send reload commands over WebSocket.
    4. Meanwhile, the systemd process crashed on port binding (`OSError: [Errno 98] Address already in use`) and restarted 3 seconds later, creating an infinite reload cascade (over 1,300 restarts).
- **Decision**:
  - **Idempotent File Writes**: Added `_write_file_if_changed()` to `ProjectsWiki/make_wiki.py` for `mkdocs.yml`, `projects.json`, and markdown stubs. Files are never overwritten or touched if content is identical, preserving mtimes and preventing spurious MkDocs live-reload events.
  - **Process Coordination**: In `ProjectsWiki/wiki.sh`:
    - `start_background()` checks if `projects-wiki.service` is active before launching to prevent conflicting instances.
    - `stop_all` is called prior to `sync_catalog` and URL resolution.
    - `run_daemon_foreground()` cleans up any stale non-systemd background processes and lingering sockets before attempting to bind ports.
    - `restart` and `status` cleanly route to `systemctl --user` when the systemd service is active.
    - In `ProjectsWiki/sync_api.py`, increased `RestartSec` from 3 to 5 and added idempotent unit file generation.

## 21. Release Integrity & Repository Hardening (v1.0.0 Gate)

### 21.1 Foreman Residue Classification & Storage Path Retention Strategy
- **Context & Audit**:
  - A comprehensive case-insensitive sweep across `src-tauri/`, `src/`, `scripts/`, `Makefile`, `PKGBUILD`, and `config.example.toml` identified 35 occurrences of `foreman`.
  - The runtime storage path logic in `src-tauri/src/config.rs` already defaults to `~/.local/share/kodiak` and `~/.config/kodiak`. `src-tauri/src/migration.rs` provides automated, atomic first-launch migration from legacy `foreman` paths to `kodiak`.
- **Decision & Strategy**:
  - **Storage Paths & Migration**: Retained all migration routines in `src-tauri/src/migration.rs` and the `FOREMAN_DATA_DIR` environment fallback in `src-tauri/src/config.rs`. Silent removal or path breakage would orphan existing user history and journals.
  - **Packaging Directives**: Retained `conflicts=('foreman') replaces=('foreman')` in `PKGBUILD` and `Conflicts: foreman / Replaces: foreman` in `scripts/build_deb.sh` to allow system package managers (pacman, dpkg) to replace the legacy packages cleanly.
  - **Prose & Doc Comment Updates**: Surgically updated stale doc comments in `src-tauri/src/enforcer.rs` (lines 8, 146), `src-tauri/src/history.rs` (line 1), `src-tauri/src/memory.rs` (line 1), and a journal format string in `src-tauri/src/lib.rs` (line 1333) so log strings and internal module documentation reflect active `~/.local/share/kodiak/` paths.
  - **Test Fixtures**: Retained legacy fixture names in unit tests where migration from `foreman` is explicitly validated.

### 21.2 Slice 2 Roadmap Restoration & Path Alignment
- **Context**:
  - `SLICE2_PROPOSAL.md` was displaced during the v1.0.0 release sweep.
- **Decision**:
  - Restored and repaired the proposal under `docs/internal/SLICE2_PROPOSAL.md` (and `docs/proposals/`).
  - Replaced all "Foreman" product references with "Kodiak".
  - Renumbered legacy charter references: mapped the hard deny-list to Charter §2 and capability grants to the Granular Grant Matrix in Feature Highlights.
  - Reconciled storage path to `~/.local/share/kodiak/history.json` with an explicit migration note.
  - **STOP GATE Outcome**: Per Aureus's explicit directive, all proposal files and folders (`docs/internal/`, `docs/proposals/`) remain strictly private and local-only; ignored globally in `.gitignore` and omitted from GitHub tracking.

### 21.3 Formalization of Security Policy & Minimal CI
- **Context**:
  - The repository lacked a formal `SECURITY.md` and had zero automated CI workflows.
- **Decision**:
  - **Security Policy (`SECURITY.md`)**: Created root security policy under 40 lines specifying private disclosure via GitHub Security Advisories, establishing v1.0.0 support, and defining the four high-priority review surfaces (grant matrix, hard deny-list, filesystem boundaries, and atomic write path).
  - **Minimal CI (`.github/workflows/ci.yml`)**: Added GitHub Actions workflow running on `ubuntu-latest` (Node 20 + Rust stable) executing `npm ci && npm run tsc` and `cargo check --manifest-path src-tauri/Cargo.toml`. Added `"tsc": "tsc --noEmit"` to `package.json` to mirror the CI typecheck command cleanly.




