# Slice 2 Proposal — Multi-Task Queue & Multi-Project Dashboards

> **Scope Notice**: This is a one-page architectural proposal for Slice 2 following completion of the Slice 1 MVP. No Slice 2 features are built or stubbed in Slice 1.

---

## 1. Objective

Extend Foreman from a single-task delegation workbench into a **sequential multi-task queue and multi-project operations dashboard**, while preserving Slice 1's core invariants: zero raw file dumps to cloud models, strict canonical whitelist boundaries, the §4 deny list, and human-in-the-loop control via the §4.2 Grant Matrix and Cruise Control.

---

## 2. Proposed Capabilities

### 2.1 Sequential Task Queue with VRAM-Aware Scheduling
- **Queue Drawer**: Allow queuing multiple natural-language tasks across one or more whitelisted projects without blocking the prompt input while a task is executing.
- **VRAM Serialization for RTX 4050 (6 GB)**: Because `qwen2.5-coder:7b` and `llama3.2:3b` share local GPU memory on Frank's RTX 4050, local execution steps remain strictly serialized on a single worker queue while Cloud Interpret and Cloud Verify calls can overlap asynchronously with local model warm-up.
- **Dependency Chaining**: Optional "Wait for previous task approval" toggle so Task $N+1$ in the same repository does not draft against stale file state before Task $N$'s diff cards are approved or rejected.

### 2.2 Batch Approval & Diff Stack Review
- **Unified Queue Inbox**: Aggregate pending approval cards across queued tasks into a single filterable inbox grouped by project directory and action class (`create`, `overwrite`, `mkdir`, `execute`).
- **Conflict Detection**: Flag overlapping line ranges if two queued tasks propose edits to the same file before either card is applied.
- **Post-Approval Local Test Hook**: For directories at `BUILD` or `AUTONOMOUS`, optionally trigger a single local verification command (e.g., `cargo check` or `pytest`) after applying a batch of approved diff cards, routing any compiler output through `digest_log_or_errors` (`llama3.2:3b`).

### 2.3 Multi-Project Health & Activity Dashboard
- **Whitelisted Project Cards**: A dashboard view displaying each whitelisted directory, its current §4.2 grant level (`READ` / `CREATE` / `BUILD` / `AUTONOMOUS`), dirty git status count, last task verdict, and recent `journal.jsonl` statistics.
- **ProjectsWiki Cross-Linking**: Direct one-click jump from a whitelisted project in Foreman to its corresponding documentation modal in the embedded ProjectsWiki tab.
- **Local Token & Latency Telemetry (Local-Only)**: Track local Ollama token throughput (tok/s) vs. cloud prompt/completion token counts per task in `~/.local/share/foreman/history.json` to quantify cloud token savings over time.
