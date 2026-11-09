import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import iconUrl from "./assets/kodiak-icon.svg";
import bearBuiltLogoUrl from "./assets/bear-built-logo.png";
import { CHARTER_TENETS, renderCharterChip } from "./charter";

type GrantLevel = "READ" | "CREATE" | "BUILD" | "AUTONOMOUS";
type ActionClass = "read" | "create" | "overwrite" | "mkdir" | "execute" | "request";
type ActiveTab = "workbench" | "wiki" | "settings" | "about";
type StageName = "interpret" | "execute" | "verify" | "done";
type StageStatus = "idle" | "active" | "completed" | "failed";

interface TickerLine {
  level: string;
  message: string;
  timestamp: string;
}

interface LiveDraft {
  step_id: number;
  title: string;
  action_class: string;
  target_path: string;
  model: string;
  text: string;
  error: string | null;
}

type PipelineEvent =
  | { kind: "stage_changed"; task_id: string; stage: string; status: string; detail: string | null; timestamp: string }
  | { kind: "ticker_log"; task_id: string; stage: string; level: string; message: string; timestamp: string }
  | { kind: "draft_stream_chunk"; task_id: string; step_id: number; title: string; action_class: string; target_path: string; model: string; chunk: string; accumulated: string }
  | { kind: "draft_stream_done"; task_id: string; step_id: number; target_path: string; success: boolean; error: string | null };

interface AltProviderConfig {
  name: string;
  base_url: string;
  api_key: string;
  model: string;
  max_context_chars?: number;
  tier?: string;
}

interface CascadeConfig {
  enabled: boolean;
  consent: boolean;
  order: string[];
}

interface WhitelistEntry {
  path: string;
  level: GrantLevel;
}

interface AppConfig {
  lumo_url: string;
  lumo_api_key: string;
  cloud_model: string;
  tier?: string;
  ollama_url: string;
  fast_model: string;
  coder_model: string;
  heavy_model: string;
  command_timeout_secs: number;
  local_idle_timeout_secs?: number;
  local_total_timeout_secs?: number;
  memory_step_budget_chars?: number;
  memory_auto_sanitize?: boolean;
  destructive_diff_threshold?: number;
  dry_run: boolean;
  wiki_dir: string;
  active_project: string | null;
  whitelist: WhitelistEntry[];
  alt_provider?: AltProviderConfig[];
  cascade?: CascadeConfig;
  run_log_level?: string;
  run_log_max_mb?: number;
  run_log_keep_runs?: number;
}

interface TaskRunLogResponse {
  task_id: string;
  content: string;
  exists: boolean;
  pruned: boolean;
  meta?: any;
}

interface HealthStatus {
  bridge_reachable: boolean;
  bridge_models: string[];
  bridge_error: string | null;
  ollama_reachable: boolean;
  ollama_models: string[];
  ollama_error: string | null;
  active_project: string | null;
  active_project_grant: string | null;
  granted_dirs_count: number;
  wiki_reachable: boolean;
  wiki_url: string | null;
}

interface PlanStep {
  id: number;
  title: string;
  action_class: ActionClass;
  route_tool: string;
  target_path: string;
  instruction: string;
  search_pattern?: string | null;
  start_line?: number | null;
  end_line?: number | null;
  memories_used?: string[];
  request_kind?: string | null;
  justification?: string | null;
  consequences_of_decline?: string | null;
  intent?: string | null;
  question?: string | null;
  answer_format?: string | null;
  suggested_default?: string | null;
}

interface TaskPlan {
  goal: string;
  estimated_risk: string;
  files_involved: string[];
  memories_used?: string[];
  steps: PlanStep[];
}

interface MemoryEntry {
  id: string;
  title: string;
  tags: string[];
  scope: string;
  cloud_share: boolean;
  updated: string;
  body: string;
  warning?: string | null;
}

interface StepExecutionLog {
  step_id: number;
  title: string;
  tier: string;
  model: string;
  route_tool: string;
  action_class: ActionClass;
  target_path: string;
  summary: string;
  status: string;
  warning_chip?: string | null;
  memory_delivery_chip?: string | null;
}

interface ActionCard {
  card_id: string;
  step_id: number;
  title: string;
  action_class: ActionClass;
  target_path: string;
  directory: string;
  grant_level: string;
  diff_or_command: string;
  proposed_content?: string | null;
  shell_command?: string | null;
  status: string;
  decision_reason: string;
  execution_output?: string | null;
  warning_chip?: string | null;
  request_kind?: string | null;
  justification?: string | null;
  consequences_of_decline?: string | null;
  user_provided_info?: string | null;
  removal_ratio?: number | null;
  inconsistency_note?: string | null;
  memory_delivery_chip?: string | null;
  question?: string | null;
  answer_format?: string | null;
  suggested_default?: string | null;
}

interface VerificationResult {
  verdict: string;
  rationale: string;
}

interface ChoiceCard {
  card_id: string;
  stage: string;
  status: string;
  chosen_option?: string | null;
  chosen_at?: string | null;
  chosen_provider?: string | null;
}

interface TaskRecord {
  task_id: string;
  timestamp: string;
  title?: string;
  starred?: boolean;
  prompt: string;
  project_dir: string;
  cloud_model: string;
  cruise_control: boolean;
  dry_run: boolean;
  posture_summary: string;
  auto_executed_count: number;
  queued_count: number;
  denied_count: number;
  plan: TaskPlan | null;
  step_logs: StepExecutionLog[];
  action_cards: ActionCard[];
  verification: VerificationResult | null;
  failed_stage: string | null;
  stage_error: string | null;
  journal_path: string;
  task_scoped_grants?: Array<{ path: string; level: string }>;
  task_provided_info?: string[];
  stage_error_kind?: string | null;
  choice_cards?: ChoiceCard[];
  deferred?: boolean;
  deferred_stage?: string | null;
  defer_reason?: string | null;
  local_grade_plan?: boolean;
  local_grade_verify?: boolean;
  provider_interpret?: string | null;
  provider_verify?: string | null;
}

interface JournalEntry {
  timestamp: string;
  task_id: string;
  step_id: number;
  action_class: ActionClass;
  target_path: string;
  command_or_summary: string;
  directory: string;
  grant_level: string;
  cruise_control: boolean;
  dry_run: boolean;
  decision: string;
  reason: string;
}

// Application UI state
const state: {
  tab: ActiveTab;
  config: AppConfig | null;
  health: HealthStatus | null;
  cruiseControl: boolean;
  promptText: string;
  runningPipeline: boolean;
  startingBridge: boolean;
  startingWiki: boolean;
  currentTask: TaskRecord | null;
  readOnlyHistorical: boolean;
  history: TaskRecord[];
  journal: JournalEntry[];
  uiBannerError: string | null;
  uiBannerInfo: string | null;
  stages: Record<StageName, StageStatus>;
  ticker: TickerLine[];
  tickerLocked: boolean;
  liveDraft: LiveDraft | null;
  sidebarCollapsed: boolean;
  pendingDeleteId: string | null;
  memories: MemoryEntry[];
  activeMemoryModal: MemoryEntry | null;
  isEditingMemory: boolean;
  pendingDeleteMemoryId: string | null;
  mentionQuery: string | null;
  cloudToast: { show: boolean; count: number } | null;
  sessionAltConsent: boolean;
  appSessionStartTime: number;
  activeRunLog: TaskRunLogResponse | null;
} = {
  tab: "workbench",
  config: null,
  health: null,
  cruiseControl: false,
  promptText: "",
  runningPipeline: false,
  startingBridge: false,
  startingWiki: false,
  currentTask: null,
  readOnlyHistorical: false,
  history: [],
  journal: [],
  uiBannerError: null,
  uiBannerInfo: null,
  stages: { interpret: "idle", execute: "idle", verify: "idle", done: "idle" },
  ticker: [],
  tickerLocked: false,
  liveDraft: null,
  sidebarCollapsed: false,
  pendingDeleteId: null,
  memories: [],
  activeMemoryModal: null,
  isEditingMemory: false,
  pendingDeleteMemoryId: null,
  mentionQuery: null,
  cloudToast: null,
  sessionAltConsent: false,
  appSessionStartTime: Date.now(),
  activeRunLog: null,
};

let tickerHover = false;

const STAGE_ORDER: StageName[] = ["interpret", "execute", "verify", "done"];
const STAGE_LABEL: Record<StageName, string> = {
  interpret: "Interpret",
  execute: "Execute",
  verify: "Verify",
  done: "Done",
};

/** Derives stage statuses from a persisted task (used for historical / post-run views). */
function stagesFromTask(task: TaskRecord | null): Record<StageName, StageStatus> {
  const s: Record<StageName, StageStatus> = { interpret: "idle", execute: "idle", verify: "idle", done: "idle" };
  if (!task) return s;
  if (task.plan) s.interpret = "completed";
  if (task.step_logs.length > 0) s.execute = "completed";
  if (task.verification) {
    s.verify = "completed";
    s.done = "completed";
  }
  if (task.failed_stage && task.failed_stage in s) {
    s[task.failed_stage as StageName] = "failed";
  }
  return s;
}

function renderStageProgress(): string {
  const stages = state.readOnlyHistorical
    ? stagesFromTask(state.currentTask)
    : state.runningPipeline || STAGE_ORDER.some((s) => state.stages[s] !== "idle")
    ? state.stages
    : stagesFromTask(state.currentTask);
  return STAGE_ORDER.map(
    (st) => `
      <div class="stage-seg is-${stages[st]}">
        <div class="stage-seg__bar"></div>
        <span>${STAGE_LABEL[st]}</span>
      </div>`
  ).join("");
}

function tickerLineHtml(l: TickerLine): string {
  return `<div class="ticker-line is-${escapeHtml(l.level)}"><span class="ticker-line__time">${escapeHtml(
    l.timestamp.slice(11, 19)
  )}</span><span>${escapeHtml(l.message)}</span></div>`;
}

function liveDraftBody(d: LiveDraft): string {
  if (d.action_class === "create") {
    const pseudo = d.text
      .split("\n")
      .map((l) => `+${l}`)
      .join("\n");
    return renderDiffLines(pseudo);
  }
  return escapeHtml(d.text);
}

function liveDraftHtml(): string {
  const d = state.liveDraft;
  if (!d) return "";
  return `
    <article class="action-card" id="live-draft-card">
      <div class="row-between">
        <div class="row-inline">
          <span class="badge badge--warn">${d.error ? "STREAM FAILED" : "STREAMING"}</span>
          <strong>${escapeHtml(d.title)}</strong>
          <span class="badge badge--neutral">${escapeHtml(d.action_class)}</span>
          <span class="mono">${escapeHtml(d.target_path)}</span>
        </div>
        <span class="badge badge--neutral">${escapeHtml(d.model)}</span>
      </div>
      ${d.error ? `<p class="muted" style="margin:0;">${escapeHtml(d.error)} — nothing was written to disk. Use the stage retry button.</p>` : ""}
      <div class="diff-view" id="live-draft-body">${liveDraftBody(d)}</div>
    </article>`;
}

/** Targeted DOM updates for live events (avoids full re-render so the prompt/scroll state survive). */
function updateLiveDom(): void {
  const bar = document.getElementById("stage-progress");
  if (bar) bar.innerHTML = renderStageProgress();
  const tk = document.getElementById("ticker-body");
  if (tk) {
    tk.innerHTML = state.ticker.map(tickerLineHtml).join("");
    if (!state.tickerLocked && !tickerHover) tk.scrollTop = tk.scrollHeight;
  }
  const draftHost = document.getElementById("live-draft-host");
  if (draftHost) {
    const body = document.getElementById("live-draft-body");
    if (body && state.liveDraft && !state.liveDraft.error) {
      body.innerHTML = liveDraftBody(state.liveDraft);
      body.scrollTop = body.scrollHeight;
    } else {
      draftHost.innerHTML = liveDraftHtml();
    }
  }
}

function handlePipelineEvent(ev: PipelineEvent): void {
  switch (ev.kind) {
    case "stage_changed": {
      const st = ev.stage as StageName;
      if (!(st in state.stages)) return;
      if (ev.status === "active") {
        // Re-enter at this stage: it and everything after resets (supports stage-only retries)
        const idx = STAGE_ORDER.indexOf(st);
        STAGE_ORDER.forEach((s, i) => {
          if (i > idx) state.stages[s] = "idle";
        });
        state.stages[st] = "active";
      } else {
        state.stages[st] = ev.status as StageStatus;
        if (ev.status === "completed" && (ev.stage === "interpret" || ev.stage === "verify")) {
          checkCloudStatusToast();
        }
      }
      break;
    }
    case "ticker_log":
      state.ticker.push({ level: ev.level, message: ev.message, timestamp: ev.timestamp });
      if (state.ticker.length > 500) state.ticker.shift();
      break;
    case "draft_stream_chunk":
      state.liveDraft = {
        step_id: ev.step_id,
        title: ev.title,
        action_class: ev.action_class,
        target_path: ev.target_path,
        model: ev.model,
        text: ev.accumulated,
        error: null,
      };
      if (!document.getElementById("live-draft-body")) {
        // Card not in DOM yet: render once, then patch incrementally
        updateLiveDom();
        return;
      }
      break;
    case "draft_stream_done":
      if (!ev.success && state.liveDraft) {
        state.liveDraft.error = ev.error ?? "Local model stream failed";
      } else if (!ev.success) {
        state.liveDraft = {
          step_id: ev.step_id, title: "Draft", action_class: "create",
          target_path: ev.target_path, model: "", text: "", error: ev.error ?? "Local model stream failed",
        };
      }
      break;
  }
  updateLiveDom();
}

function resetLiveRun(keepTicker: boolean): void {
  state.liveDraft = null;
  if (!keepTicker) state.ticker = [];
}

function escapeHtml(raw: string | null | undefined): string {
  const div = document.createElement("div");
  div.textContent = raw ?? "";
  return div.innerHTML;
}

function renderDiffLines(diffOrCmd: string): string {
  return diffOrCmd
    .split("\n")
    .map((line) => {
      const safe = escapeHtml(line);
      if (line.startsWith("+++") || line.startsWith("---") || line.startsWith("@@")) {
        return `<span class="diff-line diff-line--hdr">${safe}</span>`;
      }
      if (line.startsWith("+")) {
        return `<span class="diff-line diff-line--add">${safe}</span>`;
      }
      if (line.startsWith("-")) {
        return `<span class="diff-line diff-line--del">${safe}</span>`;
      }
      return `<span class="diff-line">${safe}</span>`;
    })
    .join("\n");
}

function verdictBadgeClass(verdict: string | undefined): string {
  const norm = (verdict ?? "").toLowerCase();
  if (norm === "pass") return "badge badge--ok";
  if (norm === "concerns") return "badge badge--warn";
  if (norm === "fail") return "badge badge--bad";
  return "badge badge--neutral";
}

function riskBadgeClass(risk: string | undefined): string {
  const norm = (risk ?? "").toLowerCase();
  if (norm === "low") return "badge badge--ok";
  if (norm === "medium") return "badge badge--warn";
  if (norm === "high") return "badge badge--bad";
  return "badge badge--neutral";
}

function cardBorderClass(status: string, actionClass?: string): string {
  if (actionClass === "request" && status === "pending_approval") {
    return "action-card action-card--request action-card--pending";
  }
  switch (status) {
    case "pending_approval":
      return "action-card action-card--pending";
    case "approved":
    case "provided":
    case "auto_executed_grant":
      return "action-card action-card--approved";
    case "rejected":
    case "denied":
    case "declined":
    case "auto_declined":
      return "action-card action-card--denied";
    case "skipped":
      return "action-card action-card--skipped";
    default:
      return "action-card";
  }
}

function cardStatusBadge(card: ActionCard): string {
  switch (card.status) {
    case "pending_approval":
      if (card.action_class === "request") {
        return `<span class="badge badge--warn">Pending Request (${escapeHtml(card.request_kind?.toUpperCase() ?? "REQUEST")})</span>`;
      }
      return `<span class="badge badge--warn">Pending Approval (${escapeHtml(card.grant_level)})</span>`;
    case "auto_executed_grant":
      return `<span class="badge badge--ok">Executed Under Grant (${escapeHtml(card.grant_level)})</span>`;
    case "approved":
      return `<span class="badge badge--ok">${card.action_class === "request" ? "Granted (Task-Scoped)" : "Approved & Executed"}</span>`;
    case "provided":
      return `<span class="badge badge--ok">Information Provided</span>`;
    case "declined":
      return `<span class="badge badge--bad">Declined</span>`;
    case "auto_declined":
      return `<span class="badge badge--bad">Auto-Declined (Deny List)</span>`;
    case "skipped":
      return `<span class="badge badge--warn">Skipped</span>`;
    case "rejected":
      return `<span class="badge badge--bad">Rejected</span>`;
    case "denied":
      return `<span class="badge badge--bad">Blocked by Enforcer / Deny List</span>`;
    case "dry_run":
      return `<span class="badge badge--warn">Dry-Run Suppressed (Zero Writes)</span>`;
    default:
      return `<span class="badge badge--neutral">${escapeHtml(card.status)}</span>`;
  }
}

async function refreshAllState(): Promise<void> {
  try {
    const [cfg, cruise, health, hist, journal, mems] = await Promise.all([
      invoke<AppConfig>("get_config"),
      invoke<boolean>("get_cruise_control"),
      invoke<HealthStatus>("check_health"),
      invoke<TaskRecord[]>("get_task_history"),
      invoke<JournalEntry[]>("get_journal_entries", { limit: 50 }),
      invoke<MemoryEntry[]>("get_memories"),
    ]);
    state.config = cfg;
    state.cruiseControl = cruise;
    state.health = health;
    state.history = hist;
    state.journal = journal;
    state.memories = mems;
  } catch (err) {
    state.uiBannerError = `State refresh error: ${String(err)}`;
  }
  renderApp();
  checkCloudStatusToast();
}

function renderHeader(): string {
  const cfg = state.config;
  const pendingCount =
    state.currentTask?.action_cards.filter((c) => c.status === "pending_approval").length ?? 0;

  return `
    <header class="app-header">
      <div class="brand-group">
        <img src="${iconUrl}" alt="Kodiak" class="brand-logo" />
        <h1 class="brand-title">Kodiak</h1>
        <span class="brand-subtitle">Local-First AI Task Delegation</span>
      </div>

      <nav class="nav-tabs" aria-label="Main Navigation">
        <button type="button" class="nav-tab ${state.tab === "workbench" ? "is-active" : ""}" data-nav="workbench">
          <span>Workbench</span>
          ${pendingCount > 0 ? `<span class="nav-tab__badge">${pendingCount}</span>` : ""}
        </button>
        <button type="button" class="nav-tab ${state.tab === "wiki" ? "is-active" : ""}" data-nav="wiki">
          <span>Projects Wiki</span>
        </button>
        <button type="button" class="nav-tab ${state.tab === "settings" ? "is-active" : ""}" data-nav="settings">
          <span>Settings</span>
          <span class="nav-tab__badge">${cfg?.whitelist.length ?? 0} dirs</span>
        </button>
        <button type="button" class="nav-tab ${state.tab === "about" ? "is-active" : ""}" data-nav="about">
          <span>About</span>
        </button>
      </nav>

      <div class="row-inline">
        ${
          state.readOnlyHistorical
            ? `<button type="button" id="return-workbench-btn" class="btn btn--sm btn--primary" title="Exit historical mode and return to active workbench">← Return to Live Workbench</button>`
            : ""
        }
        ${
          state.currentTask
            ? `<button type="button" id="copy-task-transcript-btn" class="btn btn--sm" title="Copy full task transcript as Markdown">📋 Copy Task</button>
               <button type="button" id="view-current-run-log-btn" class="btn btn--sm" data-task-id="${escapeHtml(state.currentTask.task_id)}" title="View diagnostic run log">📜 Run Log</button>`
            : ""
        }
        ${
          cfg?.dry_run
            ? `<span class="badge badge--warn" title="Dry-run suppresses all writes and command execution">DRY-RUN ON</span>`
            : ""
        }
        <button
          type="button"
          id="cruise-toggle-btn"
          class="cruise-switch"
          aria-pressed="${state.cruiseControl}"
          title="Session-only master switch. OFF queues all writes/commands; ON allows unattended actions permitted by each directory's grant level."
        >
          <span class="cruise-dot"></span>
          <span>Cruise Control: ${state.cruiseControl ? "ON" : "OFF"}</span>
        </button>
      </div>
    </header>
  `;
}

function renderHealthStrip(): string {
  const h = state.health;
  const cfg = state.config;
  if (!h || !cfg) return "";

  const bridgeDot = h.bridge_reachable ? "status-dot is-ok" : "status-dot is-bad";
  const ollamaDot = h.ollama_reachable ? "status-dot is-ok" : "status-dot is-bad";
  const modelsLoaded = h.ollama_models.length;
  const modelsDot = modelsLoaded > 0 ? "status-dot is-ok" : "status-dot is-warn";
  const projDot = h.active_project ? "status-dot is-ok" : "status-dot is-warn";

  return `
    <div class="health-strip" role="status" aria-label="System Health">
      <span class="health-item">
        <span class="${bridgeDot}"></span>
        <span>Lumo Bridge (${escapeHtml(cfg.cloud_model)}): <strong>${
          h.bridge_reachable ? "Online" : "Bridge Offline"
        }</strong></span>
        ${
          !h.bridge_reachable
            ? `<button type="button" id="start-bridge-btn" class="btn btn--accent btn--sm" ${
                state.startingBridge ? "disabled" : ""
              }>${state.startingBridge ? "Starting lumo-tamer..." : "Start Bridge"}</button>`
            : ""
        }
      </span>

      <span class="health-item">
        <span class="${ollamaDot}"></span>
        <span>Ollama: <strong>${h.ollama_reachable ? "Online" : "Offline"}</strong></span>
      </span>

      <span class="health-item">
        <span class="${modelsDot}"></span>
        <span>Local Models: <strong>${
          modelsLoaded > 0 ? escapeHtml(h.ollama_models.slice(0, 3).join(", ")) : "None detected"
        }</strong></span>
      </span>

      <span class="health-item">
        <span class="${projDot}"></span>
        <span>Active Project: <strong>${escapeHtml(h.active_project ?? "None (configure in Settings)")}</strong></span>
        ${
          h.active_project_grant
            ? `<span class="badge">${escapeHtml(h.active_project_grant)}</span>`
            : ""
        }
      </span>

      <div class="health-actions">
        <button type="button" id="refresh-health-btn" class="btn btn--sm">Refresh Health</button>
      </div>
    </div>
  `;
}

function renderWorkbenchTab(): string {
  const cfg = state.config!;
  const h = state.health;
  const task = state.currentTask;

  const postureText = cfg.dry_run
    ? `Dry-Run Mode (Zero Writes) — ${cfg.whitelist.length} dirs in whitelist`
    : state.cruiseControl
    ? `Cruise: ${cfg.whitelist.length} dirs granted`
    : "Manual approval";

  const postureClass = cfg.dry_run
    ? "posture-banner is-dryrun"
    : state.cruiseControl
    ? "posture-banner is-cruise"
    : "posture-banner";

  const autoCount =
    task?.action_cards.filter((c) => c.status === "auto_executed_grant").length ?? 0;
  const queuedCount =
    task?.action_cards.filter((c) => c.status === "pending_approval").length ?? 0;
  const deniedCount = task?.action_cards.filter((c) => c.status === "denied").length ?? 0;

  return `
    <div class="${postureClass}">
      <div class="row-inline">
        <strong>Posture:</strong>
        <span>${escapeHtml(postureText)}</span>
        ${
          state.readOnlyHistorical
            ? `<span class="badge badge--warn">Read-Only Historical View</span>`
            : ""
        }
      </div>
      <div class="row-inline">
        <span class="badge badge--ok">Auto-Executed: ${autoCount}</span>
        <span class="badge badge--warn">Queued for Approval: ${queuedCount}</span>
        <span class="badge badge--bad">Blocked/Denied: ${deniedCount}</span>
        ${
          state.readOnlyHistorical
            ? `<button type="button" id="load-to-workbench-btn" class="btn btn--sm btn--accent" title="Prefill prompt and project in workbench without running">Load into Workbench</button>
               <button type="button" id="rerun-btn" class="btn btn--sm btn--accent">↻ Re-run in same project</button>
               <button type="button" id="exit-readonly-btn" class="btn btn--sm btn--primary">← Return to Live Workbench</button>`
            : ""
        }
      </div>
    </div>

    ${
      state.uiBannerError
        ? `<div class="stage-alert">
            <span><strong>Error:</strong> ${escapeHtml(state.uiBannerError)}</span>
            <button type="button" id="dismiss-error-btn" class="btn btn--sm">Dismiss</button>
          </div>`
        : ""
    }
    ${
      state.uiBannerInfo
        ? `<div class="posture-banner is-cruise">
            <span>${escapeHtml(state.uiBannerInfo)}</span>
            <button type="button" id="dismiss-info-btn" class="btn btn--sm">Dismiss</button>
          </div>`
        : ""
    }

    <!-- Prompt & Model Tier Panel -->
    <section class="panel-card">
      <div class="panel-header">
        <h2 class="panel-title">1. Task Delegation Prompt</h2>
        <div class="row-inline">
          <span class="badge" title="Cloud Interpreter & Verifier">1. Interpret & 3. Verify: ${escapeHtml(
            cfg.cloud_model
          )}</span>
          <span class="badge badge--neutral" title="Local Fast Indexer & Summarizer">2a. Fast/Search: ${escapeHtml(
            cfg.fast_model
          )}</span>
          <span class="badge badge--neutral" title="Local Default Coder">2b. Draft/Review: ${escapeHtml(
            cfg.coder_model
          )}</span>
        </div>
      </div>

      ${
        cfg.whitelist.length === 0
          ? `<div class="stage-alert">
              <span>No whitelisted project directories configured yet. Per human authority (${renderCharterChip(2, "Nothing happens without you")}), Kodiak touches nothing outside whitelisted directories.</span>
              <button type="button" class="btn btn--accent btn--sm" data-nav="settings">Add Project in Settings →</button>
            </div>`
          : ""
      }

      ${
        !h?.bridge_reachable
          ? `<div class="stage-alert">
              <span><strong>Lumo Bridge Offline:</strong> Cloud interpret/verify requires <code>127.0.0.1:3003</code> (${escapeHtml(
                h?.bridge_error ?? "unreachable"
              )}).</span>
              <button type="button" id="start-bridge-inline-btn" class="btn btn--primary btn--sm" ${
                state.startingBridge ? "disabled" : ""
              }>${state.startingBridge ? "Starting via systemd..." : "Start Lumo Bridge"}</button>
            </div>`
          : ""
      }

      <div class="form-field prompt-wrap">
        <div class="row-between">
          <label class="form-label" for="task-prompt-input">
            Plain-language task (typos/shorthand welcome — type <code>@</code> to cite memory, Cloud interprets, Local executes, Cloud verifies)
          </label>
          <button type="button" id="quick-new-memory-btn" class="btn btn--sm" style="margin-block-end:0.25rem;">+ New Memory</button>
        </div>
        ${renderMentionPicker()}
        <textarea
          id="task-prompt-input"
          class="form-textarea"
          placeholder="e.g., compare @frank-base-specs with new laptop specs..."
          ${state.runningPipeline || state.readOnlyHistorical ? "disabled" : ""}
        >${escapeHtml(state.readOnlyHistorical && task ? task.prompt : state.promptText)}</textarea>
      </div>

      <div class="row-between">
        <div class="row-inline">
          <label class="form-label" for="active-project-QuickSelect">Target Whitelisted Project:</label>
          <select id="active-project-QuickSelect" class="form-select" ${
            state.runningPipeline || cfg.whitelist.length === 0 ? "disabled" : ""
          }>
            ${
              cfg.whitelist.length === 0
                ? `<option value="">-- No whitelisted projects --</option>`
                : cfg.whitelist
                    .map(
                      (w) =>
                        `<option value="${escapeHtml(w.path)}" ${
                          cfg.active_project === w.path ? "selected" : ""
                        }>${escapeHtml(w.path)} [${escapeHtml(w.level)}]</option>`
                    )
                    .join("")
            }
          </select>
        </div>

        <div class="row-inline">
          <span class="muted" style="font-size:0.75rem;">Cloud plans & verifies; local builds. ${renderCharterChip(1, "Your files stay home")} • ${renderCharterChip(5, "Local first")}</span>
          <button
            type="button"
            id="run-pipeline-btn"
            class="btn btn--primary"
            ${
              state.runningPipeline ||
              state.readOnlyHistorical ||
              cfg.whitelist.length === 0
                ? "disabled"
                : ""
            }
          >
            ${
              state.runningPipeline
                ? "Running Tiered Pipeline..."
                : cfg.dry_run
                ? "▶ Run Pipeline (Dry-Run)"
                : "▶ Run Pipeline"
            }
          </button>
        </div>
      </div>
    </section>

    <!-- Slice 2: live stage progress bar -->
    <div class="stage-progress" id="stage-progress" aria-label="Pipeline stage progress">${renderStageProgress()}</div>

    ${
      state.ticker.length > 0 || state.runningPipeline
        ? `<section class="panel-card">
            <div class="panel-header">
              <h2 class="panel-title">Live Event Ticker</h2>
              <div class="row-inline">
                <button type="button" id="ticker-copy-all-btn" class="btn btn--sm" title="Copy all ticker log lines as text">Copy All Lines</button>
                <span class="muted">${state.tickerLocked ? "Scroll locked" : "Auto-scroll (pauses on hover)"}</span>
                <button type="button" id="ticker-lock-btn" class="btn btn--sm">${state.tickerLocked ? "Unlock scroll" : "Lock scroll"}</button>
              </div>
            </div>
            <div class="ticker-body" id="ticker-body">${state.ticker.map(tickerLineHtml).join("")}</div>
          </section>`
        : ""
    }

    <div id="live-draft-host">${state.runningPipeline || state.liveDraft?.error ? liveDraftHtml() : ""}</div>

    ${task ? renderTaskResults(task) : ""}
    ${task && state.readOnlyHistorical ? renderTaskJournalRefs(task) : ""}
  `;
}

function renderTaskJournalRefs(task: TaskRecord): string {
  const entries = state.journal.filter((j) => j.task_id === task.task_id);
  return `
    <section class="panel-card">
      <div class="panel-header">
        <h2 class="panel-title">Journal References</h2>
        <span class="muted">${entries.length} entr${entries.length === 1 ? "y" : "ies"} in <code>${escapeHtml(task.journal_path)}</code></span>
      </div>
      ${
        entries.length === 0
          ? `<p class="muted">No recent journal entries for this task.</p>`
          : `<div class="queue-grid">${entries
              .map(
                (j) => `<div class="row-inline mono" style="font-size:0.78rem;">
                  <span class="badge badge--neutral">${escapeHtml(j.decision)}</span>
                  <span>${escapeHtml(j.target_path)}</span>
                  <span class="muted">— ${escapeHtml(j.reason)}</span>
                  <span class="muted">${escapeHtml(j.timestamp.slice(0, 19))}</span>
                </div>`
              )
              .join("")}</div>`
      }
    </section>`;
}

// ---- Slice 2.1: Clipboard & Memory Bank Helpers ----
async function copyToClipboard(text: string, label: string = "Copied"): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
    state.uiBannerInfo = `${label} to clipboard.`;
    renderApp();
  } catch (err) {
    state.uiBannerError = `Failed to copy: ${String(err)}`;
    renderApp();
  }
}

function taskAsMarkdown(task: TaskRecord): string {
  let md = `# Kodiak Task: ${taskTitle(task)}\n\n`;
  md += `- **Timestamp:** ${task.timestamp}\n`;
  md += `- **Project:** \`${task.project_dir}\`\n`;
  md += `- **Posture:** ${task.posture_summary}\n`;
  md += `- **Cloud Model:** ${task.cloud_model}\n\n`;
  md += `## 1. Task Prompt\n\n${task.prompt}\n\n`;

  if (task.plan) {
    md += `## 2. Cloud Plan\n\n**Goal:** ${task.plan.goal}\n`;
    md += `**Estimated Risk:** ${task.plan.estimated_risk}\n`;
    md += `**Files Involved:** ${task.plan.files_involved.join(", ") || "none"}\n`;
    if (task.plan.memories_used && task.plan.memories_used.length > 0) {
      md += `**Memories Used:** ${task.plan.memories_used.map((m) => "@" + m).join(", ")}\n`;
    }
    md += `\n### Plan Steps\n`;
    for (const s of task.plan.steps) {
      md += `\n#### Step #${s.id}: ${s.title}\n`;
      md += `- **Action:** ${s.action_class}\n`;
      md += `- **Route:** ${s.route_tool}\n`;
      md += `- **Target Path:** \`${s.target_path}\`\n`;
      md += `- **Instruction:** ${s.instruction}\n`;
      if (s.memories_used && s.memories_used.length > 0) {
        md += `- **Memories:** ${s.memories_used.map((m) => "@" + m).join(", ")}\n`;
      }
    }
    md += `\n`;
  }

  if (task.step_logs.length > 0) {
    md += `## 3. Local Model Routing & Execution Log\n\n`;
    for (const log of task.step_logs) {
      md += `### Step #${log.step_id}: ${log.title}\n`;
      md += `- **Tier/Model:** ${log.tier} (${log.model})\n`;
      md += `- **Route:** ${log.route_tool} (${log.action_class}) on \`${log.target_path}\`\n`;
      md += `- **Status:** ${log.status}\n`;
      if (log.warning_chip) {
        md += `- **Warning:** ${log.warning_chip}\n`;
      }
      if (log.memory_delivery_chip) {
        md += `- **Memory:** ${log.memory_delivery_chip}\n`;
      }
      md += `\n\`\`\`\n${log.summary}\n\`\`\`\n\n`;
    }
  }

  if (task.action_cards.length > 0) {
    md += `## 4. Approval Queue & Action Audit Cards\n\n`;
    for (const card of task.action_cards) {
      md += `### Step #${card.step_id}: ${card.title}\n`;
      md += `- **Action:** ${card.action_class}\n`;
      md += `- **Target Path:** \`${card.target_path}\`\n`;
      md += `- **Grant Level:** ${card.grant_level}\n`;
      md += `- **Status:** ${card.status}\n`;
      md += `- **Decision Reason:** ${card.decision_reason}\n`;
      if (card.warning_chip) {
        md += `- **Warning:** ${card.warning_chip}\n`;
      }
      if (card.memory_delivery_chip) {
        md += `- **Memory:** ${card.memory_delivery_chip}\n`;
      }
      md += `\n`;
      if (card.diff_or_command.startsWith("---") || card.diff_or_command.includes("\n+++")) {
        md += `\`\`\`diff\n${card.diff_or_command}\n\`\`\`\n\n`;
      } else {
        md += `\`\`\`bash\n${card.diff_or_command}\n\`\`\`\n\n`;
      }
      if (card.execution_output) {
        md += `**Execution Output:**\n\n\`\`\`\n${card.execution_output}\n\`\`\`\n\n`;
      }
    }
  }

  if (task.verification) {
    md += `## 5. Cloud Verification Verdict\n\n`;
    md += `**Verdict:** ${task.verification.verdict.toUpperCase()}\n\n`;
    md += `${task.verification.rationale}\n\n`;
  }

  return md;
}

function renderMentionPicker(): string {
  if (state.mentionQuery === null) return "";
  const q = state.mentionQuery.toLowerCase();
  const filtered = state.memories.filter((m) => {
    // Project scope check
    if (m.scope.startsWith("project:") && state.config?.active_project) {
      const proj = m.scope.slice(8).trim().toLowerCase();
      if (!state.config.active_project.toLowerCase().includes(proj)) return false;
    }
    return m.id.toLowerCase().includes(q) || m.title.toLowerCase().includes(q) || m.tags.some((t) => t.toLowerCase().includes(q));
  });

  if (filtered.length === 0) {
    return `<div class="mention-picker"><div class="mention-item muted">No matching memories</div></div>`;
  }

  return `
    <div class="mention-picker" id="mention-picker">
      ${filtered
        .map(
          (m) => `
        <button type="button" class="mention-item" data-pick-mention="${escapeHtml(m.id)}">
          <span class="mention-item__title">@${escapeHtml(m.id)} (${escapeHtml(m.title)})</span>
          <span class="mention-item__tags">${m.cloud_share ? "☁ cloud" : "🔒 local"} ${escapeHtml(m.tags.join(","))}</span>
        </button>`
        )
        .join("")}
    </div>`;
}

function sideMemoryItem(m: MemoryEntry): string {
  const cloudIcon = m.cloud_share ? `<span title="cloud_share: yes (available in cloud prompts)">☁</span>` : `<span title="cloud_share: no (local-only knowledge)">🔒</span>`;
  const warnIcon = m.warning ? `<span title="${escapeHtml(m.warning)}" style="color:var(--status-warn);">⚠️</span>` : "";
  const tagChips = m.tags.slice(0, 2).map((t) => `<span class="badge badge--neutral" style="font-size:0.65rem; padding:0 0.25rem;">${escapeHtml(t)}</span>`).join("");
  return `
    <div class="side-item" data-view-memory="${escapeHtml(m.id)}" title="${escapeHtml(m.title)} (@${escapeHtml(m.id)})" role="button" tabindex="0">
      <span class="side-item__label">${escapeHtml(truncate(m.title, 40))}</span>
      ${tagChips}
      ${warnIcon}
      ${cloudIcon}
      <button type="button" class="side-icon-btn" data-edit-memory="${escapeHtml(m.id)}" title="Edit memory">✎</button>
      <button type="button" class="side-icon-btn" data-delete-memory="${escapeHtml(m.id)}" title="Delete memory">🗑</button>
    </div>`;
}

// ---- Slice 2: Sidebar (Projects + Tasks) ----
function taskTitle(rec: TaskRecord): string {
  return (rec.title && rec.title.trim()) || rec.plan?.goal || rec.prompt;
}

function truncate(s: string, n: number): string {
  return s.length > n ? `${s.slice(0, n - 1).trimEnd()}…` : s;
}

function dayLabel(iso: string): string {
  const d = new Date(iso);
  const today = new Date();
  const startOf = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const diffDays = Math.round((startOf(today) - startOf(d)) / 86400000);
  if (diffDays === 0) return "Today";
  if (diffDays === 1) return "Yesterday";
  return d.toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" });
}

function sideTaskItem(rec: TaskRecord): string {
  const title = taskTitle(rec);
  const active = state.readOnlyHistorical && state.currentTask?.task_id === rec.task_id;
  return `
    <div class="side-item ${active ? "is-active" : ""}" data-open-task="${escapeHtml(rec.task_id)}" title="${escapeHtml(title)}" role="button" tabindex="0">
      <span class="side-item__label">${escapeHtml(truncate(title, 55))}</span>
      <button type="button" class="side-icon-btn btn-view-run-log" data-task-id="${escapeHtml(rec.task_id)}" title="View diagnostic run log">📜</button>
      <button type="button" class="side-icon-btn ${rec.starred ? "is-on" : ""}" data-star-task="${escapeHtml(rec.task_id)}" title="${rec.starred ? "Unstar" : "Star"}">${rec.starred ? "★" : "☆"}</button>
      <button type="button" class="side-icon-btn" data-delete-task="${escapeHtml(rec.task_id)}" title="Delete history record">🗑</button>
    </div>`;
}

function renderSidebar(): string {
  if (state.sidebarCollapsed) {
    return `<aside class="sidebar is-collapsed"><div class="sidebar__top"><button type="button" class="side-icon-btn" id="sidebar-toggle-btn" title="Expand sidebar">»</button></div></aside>`;
  }
  const cfg = state.config;
  const projects = (cfg?.whitelist ?? [])
    .map((w) => {
      const name = w.path.split("/").filter(Boolean).pop() ?? w.path;
      return `<button type="button" class="side-item ${cfg?.active_project === w.path ? "is-active" : ""}" data-select-project="${escapeHtml(w.path)}" title="${escapeHtml(w.path)}">
        <span class="side-item__label">${escapeHtml(name)}</span>
        <span class="badge badge--neutral">${escapeHtml(w.level)}</span>
      </button>`;
    })
    .join("");

  // Visible memories based on active project scope
  const activeProj = cfg?.active_project?.toLowerCase() ?? "";
  const visibleMemories = state.memories.filter((m) => {
    if (m.scope === "global" || !m.scope.startsWith("project:")) return true;
    const p = m.scope.slice(8).trim().toLowerCase();
    return activeProj.includes(p) || p.includes(activeProj);
  });

  const memoriesHtml =
    visibleMemories.length === 0
      ? `<p class="muted" style="font-size:0.78rem;">No memories match scope.</p>`
      : visibleMemories.map(sideMemoryItem).join("");

  const sorted = [...state.history].sort((a, b) => b.timestamp.localeCompare(a.timestamp));
  const deferredTasks = sorted.filter((r) => r.deferred);
  const nonDeferred = sorted.filter((r) => !r.deferred);
  const favs = nonDeferred.filter((r) => r.starred);
  const groups = new Map<string, TaskRecord[]>();
  for (const r of nonDeferred.filter((x) => !x.starred)) {
    const k = dayLabel(r.timestamp);
    if (!groups.has(k)) groups.set(k, []);
    groups.get(k)!.push(r);
  }
  const tasksHtml =
    nonDeferred.length === 0
      ? `<p class="muted" style="font-size:0.78rem;">No tasks yet.</p>`
      : `${favs.length ? `<div><h4 class="sidebar__heading">Favorites</h4>${favs.map(sideTaskItem).join("")}</div>` : ""}
         ${[...groups.entries()]
           .map(([k, recs]) => `<div><h4 class="sidebar__heading">${escapeHtml(k)}</h4>${recs.map(sideTaskItem).join("")}</div>`)
           .join("")}`;

  const deferredHtml =
    deferredTasks.length === 0
      ? ""
      : `<div>
          <div class="row-between" style="margin-block-end:0.25rem;">
            <h3 class="sidebar__heading" style="margin:0;">Deferred <span class="badge badge--warn">${deferredTasks.length}</span></h3>
          </div>
          <div class="queue-grid" style="gap:0.4rem;">
            ${deferredTasks
              .map((rec) => {
                const title = rec.title?.trim() || rec.prompt.trim() || rec.task_id;
                return `
                  <div class="side-item side-item--deferred" style="display:flex; flex-direction:column; gap:0.35rem; padding:0.45rem 0.6rem; border:1px solid var(--status-warn); border-radius:var(--wiki-radius-sm); margin-bottom:0.25rem; background:var(--wiki-surface);">
                    <div class="row-between" style="width:100%;">
                      <span class="side-item__label" style="font-weight:600; cursor:pointer;" data-select-task="${escapeHtml(rec.task_id)}" title="${escapeHtml(rec.prompt)}">
                        ${escapeHtml(truncate(title, 36))}
                      </span>
                      <button type="button" class="btn btn--sm btn--primary btn-resume-deferred" data-task-id="${escapeHtml(rec.task_id)}" title="Validate cloud health and resume parked stage" ${state.runningPipeline ? "disabled" : ""}>
                        Resume
                      </button>
                    </div>
                    <div class="row-between" style="width:100%; font-size:0.72rem;">
                      <span class="badge badge--warn" style="font-size:0.68rem;">Waiting for cloud availability</span>
                      <span class="mono muted">Stage: ${escapeHtml(rec.deferred_stage || "interpret")}</span>
                    </div>
                  </div>
                `;
              })
              .join("")}
          </div>
        </div>`;

  return `
    <aside class="sidebar">
      <div class="sidebar__top">
        <button type="button" class="side-icon-btn" id="sidebar-toggle-btn" title="Collapse sidebar">«</button>
      </div>
      <div class="sidebar__scroll">
        <div>
          <div class="row-between">
            <h3 class="sidebar__heading">Projects</h3>
          </div>
          ${projects || `<p class="muted" style="font-size:0.78rem;">None whitelisted.</p>`}
        </div>

        <div>
          <div class="row-between" style="margin-block-end:0.25rem;">
            <h3 class="sidebar__heading" style="margin:0;">Memories</h3>
            <div class="row-inline">
              <button type="button" class="side-icon-btn" id="refresh-memories-btn" title="Rescan memories from disk">↻</button>
              <button type="button" class="side-icon-btn" id="add-memory-btn" title="New Memory">+</button>
            </div>
          </div>
          ${memoriesHtml}
        </div>

        ${deferredHtml}

        <div>
          <h3 class="sidebar__heading">Tasks</h3>
          ${tasksHtml}
        </div>
      </div>
    </aside>

    ${renderDeleteTaskModal()}
    ${renderDeleteMemoryModal()}
    ${renderMemoryViewModal()}
    ${renderMemoryEditModal()}
  `;
}

function renderDeleteTaskModal(): string {
  if (!state.pendingDeleteId) return "";
  return `
    <div class="modal-backdrop"><div class="modal-box" role="dialog" aria-modal="true">
      <strong>Delete this task from history?</strong>
      <span class="muted">Only the history record is removed (and logged to the journal). Project files are never touched.</span>
      <div class="row-inline">
        <button type="button" class="btn btn--sm" id="delete-cancel-btn">Cancel</button>
        <button type="button" class="btn btn--sm btn--primary" id="delete-confirm-btn">Delete</button>
      </div>
    </div></div>`;
}

function renderDeleteMemoryModal(): string {
  if (!state.pendingDeleteMemoryId) return "";
  return `
    <div class="modal-backdrop"><div class="modal-box" role="dialog" aria-modal="true">
      <strong>Delete memory @${escapeHtml(state.pendingDeleteMemoryId)}?</strong>
      <span class="muted">The markdown file in ~/.local/share/kodiak/memory/ will be removed. Project files and task history are never touched.</span>
      <div class="row-inline">
        <button type="button" class="btn btn--sm" id="delete-mem-cancel-btn">Cancel</button>
        <button type="button" class="btn btn--sm btn--primary" id="delete-mem-confirm-btn">Delete</button>
      </div>
    </div></div>`;
}

function renderMemoryViewModal(): string {
  if (!state.activeMemoryModal || state.isEditingMemory) return "";
  const m = state.activeMemoryModal;
  return `
    <div class="modal-backdrop"><div class="modal-box" style="max-inline-size:38rem; inline-size:90%;" role="dialog" aria-modal="true">
      <div class="row-between">
        <div class="row-inline">
          <strong>${escapeHtml(m.title)}</strong>
          <span class="mono muted">@${escapeHtml(m.id)}</span>
        </div>
        <div class="row-inline">
          <span class="badge ${m.cloud_share ? "badge--ok" : "badge--neutral"}">${m.cloud_share ? "☁ cloud_share: yes" : "🔒 local-only"}</span>
          <span class="badge badge--neutral">${escapeHtml(m.scope)}</span>
          <button type="button" class="copy-btn-inline" data-copy-text="${escapeHtml(m.body)}" data-copy-label="Memory body">Copy</button>
          <button type="button" class="side-icon-btn" id="view-mem-edit-btn" title="Edit">✎</button>
          <button type="button" class="side-icon-btn" id="view-mem-close-btn" title="Close">✕</button>
        </div>
      </div>
      ${m.warning ? `<div class="badge badge--warn" style="display:block;">${escapeHtml(m.warning)}</div>` : ""}
      <div class="diff-view" style="max-block-size:24rem; overflow-y:auto; white-space:pre-wrap;">${escapeHtml(m.body)}</div>
      <div class="row-between muted" style="font-size:0.75rem;">
        <span>Tags: ${escapeHtml(m.tags.join(", ") || "none")}</span>
        <span>Updated: ${escapeHtml(m.updated)}</span>
      </div>
    </div></div>`;
}

function renderMemoryEditModal(): string {
  if (!state.activeMemoryModal || !state.isEditingMemory) return "";
  const m = state.activeMemoryModal;
  const isNew = !state.memories.some((x) => x.id === m.id);
  const cfg = state.config;

  return `
    <div class="modal-backdrop"><div class="modal-box" style="max-inline-size:42rem; inline-size:90%;" role="dialog" aria-modal="true">
      <div class="row-between">
        <strong>${isNew ? "New Memory" : `Edit Memory (@${escapeHtml(m.id)})`}</strong>
        <button type="button" class="side-icon-btn" id="edit-mem-close-btn" title="Cancel">✕</button>
      </div>

      <div style="display:grid; grid-template-columns:1fr 1fr; gap:0.6rem;">
        <div class="form-field">
          <label class="form-label" for="mem-edit-title">Title</label>
          <input type="text" id="mem-edit-title" class="form-input" value="${escapeHtml(m.title)}" placeholder="e.g. Frank base specs" />
        </div>
        <div class="form-field">
          <label class="form-label" for="mem-edit-id">Identifier / Slug</label>
          <input type="text" id="mem-edit-id" class="form-input" value="${escapeHtml(m.id)}" ${!isNew ? "disabled" : ""} placeholder="frank-base-specs" />
        </div>
      </div>

      <div style="display:grid; grid-template-columns:1fr 1fr; gap:0.6rem;">
        <div class="form-field">
          <label class="form-label" for="mem-edit-tags">Tags (comma-separated)</label>
          <input type="text" id="mem-edit-tags" class="form-input" value="${escapeHtml(m.tags.join(", "))}" placeholder="hardware, frank, specs" />
        </div>
        <div class="form-field">
          <label class="form-label" for="mem-edit-scope">Scope</label>
          <select id="mem-edit-scope" class="form-select">
            <option value="global" ${m.scope === "global" ? "selected" : ""}>global (all projects)</option>
            ${(cfg?.whitelist ?? []).map((w) => `<option value="project:${escapeHtml(w.path)}" ${m.scope === `project:${w.path}` ? "selected" : ""}>project: ${escapeHtml(w.path)}</option>`).join("")}
          </select>
        </div>
      </div>

      <div class="row-between" style="padding:0.25rem 0;">
        <label class="row-inline" style="cursor:pointer;">
          <input type="checkbox" id="mem-edit-cloudshare" ${m.cloud_share ? "checked" : ""} />
          <span><strong>cloud_share: yes</strong> (Allow full text to be shared with cloud interpret/verify stages)</span>
        </label>
      </div>

      <div class="form-field">
        <label class="form-label" for="mem-edit-body">Markdown Body</label>
        <textarea id="mem-edit-body" class="form-textarea" style="min-block-size:12rem; font-family:var(--md-code-font-family);">${escapeHtml(m.body)}</textarea>
      </div>

      <div class="row-between">
        <button type="button" class="btn btn--sm" id="edit-mem-cancel-btn">Cancel</button>
        <button type="button" class="btn btn--sm btn--primary" id="edit-mem-save-btn">Save Memory</button>
      </div>
    </div></div>`;
}
function renderChoiceCard(task: TaskRecord): string {
  const hasAlt = (state.config?.alt_provider && state.config.alt_provider.length > 0) ?? false;
  const failedStage = task.failed_stage || "interpret";
  return `
    <div class="choice-card">
      <div class="row-between">
        <div class="row-inline">
          <span class="badge badge--warn">CLOUD QUOTA EXHAUSTED</span>
          <strong>Waiting for your decision on stage '${escapeHtml(failedStage)}'</strong>
        </div>
        <span class="mono muted" style="font-size:0.75rem;">${escapeHtml(task.stage_error ?? "")}</span>
      </div>
      <div class="choice-options-grid">
        <!-- Option 1: Defer -->
        <div class="choice-option-card">
          <div class="choice-option-card__action">
            <strong>Defer task</strong>
            <button type="button" class="btn btn--sm btn-choice-defer" data-task-id="${escapeHtml(task.task_id)}" ${state.runningPipeline ? "disabled" : ""}>
              Defer task
            </button>
          </div>
          <div class="choice-option-card__consequence">
            Parks task in defer queue with status "Waiting for cloud availability" until quota resets.
          </div>
        </div>

        <!-- Option 2: Local Interpret / Verify -->
        <div class="choice-option-card">
          <div class="choice-option-card__action">
            <strong>Continue with local ${escapeHtml(failedStage)}</strong>
            <button type="button" class="btn btn--sm btn-choice-local" data-task-id="${escapeHtml(task.task_id)}" ${state.runningPipeline ? "disabled" : ""}>
              Run local ${escapeHtml(failedStage)}
            </button>
          </div>
          <div class="choice-option-card__consequence">
            Re-runs failed stage locally via ${escapeHtml(state.config?.heavy_model || "qwen2.5-coder:14b")}. Local execution keeps files home — ${renderCharterChip(1, "Files stay home")}.
          </div>
        </div>

        <!-- Option 3: Alternate Provider -->
        <div class="choice-option-card">
          <div class="choice-option-card__action">
            <strong>Use alternate provider</strong>
            ${
              hasAlt
                ? `<button type="button" class="btn btn--sm btn--primary btn-choice-alt" data-task-id="${escapeHtml(task.task_id)}" ${state.runningPipeline ? "disabled" : ""}>
                    Use provider
                  </button>`
                : ""
            }
          </div>
          ${
            hasAlt
              ? `
                <div style="margin-top:0.25rem;">
                  <select id="choice-alt-provider-select" class="form-select form-select--sm">
                    ${(state.config?.alt_provider ?? [])
                      .map((p) => `<option value="${escapeHtml(p.name)}">${escapeHtml(p.name)} (${escapeHtml(p.model)})</option>`)
                      .join("")}
                  </select>
                </div>
                <div class="choice-consent-notice">
                  <strong>Privacy Notice:</strong> This sends your task text and plan context to a non-Proton service. Alternate providers do not inherit the privacy guarantee — see ${renderCharterChip(1, "Your files stay home")}.
                  <label style="display:flex; align-items:center; gap:0.5rem; margin-top:0.35rem; cursor:pointer;">
                    <input type="checkbox" id="choice-alt-consent-check" ${state.sessionAltConsent ? "checked" : ""}>
                    <span>I consent to sending task context to this provider for this session.</span>
                  </label>
                </div>
                <div class="choice-option-card__consequence">
                  Routes stage to selected non-Proton OpenAI-compatible endpoint. Consent required; logged in journal.
                </div>
              `
              : `
                <div class="choice-option-card__consequence muted">
                  No alternate providers configured. Click Settings → Cloud & Providers to add an OpenAI-compatible endpoint or read the setup guides.
                </div>
              `
          }
        </div>
      </div>
    </div>
  `;
}

function renderDeferredCard(task: TaskRecord): string {
  return `
    <div class="choice-card">
      <div class="row-between">
        <div class="row-inline">
          <span class="badge badge--warn">TASK DEFERRED</span>
          <strong>Waiting for cloud availability (stage: ${escapeHtml(task.deferred_stage || "interpret")})</strong>
        </div>
        <button type="button" class="btn btn--sm btn--primary btn-resume-deferred" data-task-id="${escapeHtml(task.task_id)}" ${state.runningPipeline ? "disabled" : ""}>
          Resume Task
        </button>
      </div>
      <p class="muted" style="margin:0; font-size:0.8rem;">
        Parked reason: ${escapeHtml(task.defer_reason || "Cloud quota exhausted")}. Resume re-validates cloud health first, then continues without re-executing completed work.
      </p>
    </div>
  `;
}

function renderResolvedChoiceCards(task: TaskRecord): string {
  if (!task.choice_cards || task.choice_cards.length === 0) return "";
  const resolved = task.choice_cards.filter((c) => c.status === "resolved");
  if (resolved.length === 0) return "";
  return resolved
    .map(
      (c) => `
        <div class="choice-card choice-card--resolved">
          <div class="row-between">
            <div class="row-inline">
              <span class="badge badge--neutral">CHOICE CARD</span>
              <strong>Cloud Failure Decision (${escapeHtml(c.stage)})</strong>
            </div>
            <span class="badge badge--ok">Resolved: ${escapeHtml(c.chosen_option || "unknown")}</span>
          </div>
          <p class="muted" style="margin:0; font-size:0.8rem;">
            Option chosen: <strong>${escapeHtml(c.chosen_option || "")}</strong>
            ${c.chosen_provider ? `via <code>${escapeHtml(c.chosen_provider)}</code>` : ""}
            ${c.chosen_at ? `at ${escapeHtml(c.chosen_at.slice(0, 19).replace("T", " "))}` : ""}
          </p>
        </div>`
    )
    .join("");
}

function renderTaskResults(task: TaskRecord): string {
  const isQuota =
    task.stage_error_kind === "quota_exhausted" ||
    (task.stage_error ? /quota|rate[- ]?limit|usage limit|429/i.test(task.stage_error) : false);

  return `
    <div class="row-between" style="margin-bottom:0.75rem; padding:0.4rem 0.75rem; border:1px solid var(--wiki-border); border-radius:var(--wiki-radius-sm); background:var(--wiki-surface);">
      <div class="row-inline">
        <span class="badge badge--ok">Task Run Diagnostics</span>
        <span class="mono muted" style="font-size:0.8rem;">ID: ${escapeHtml(task.task_id)}</span>
      </div>
      <button type="button" class="btn btn--sm btn-view-run-log" data-task-id="${escapeHtml(task.task_id)}" title="View diagnostic run log">
        📜 View Run Log
      </button>
    </div>

    ${task.deferred ? renderDeferredCard(task) : ""}
    ${renderResolvedChoiceCards(task)}
    ${
      task.failed_stage && isQuota && !task.deferred
        ? renderChoiceCard(task)
        : task.failed_stage && !task.deferred
        ? `<div class="stage-alert">
            <div>
              <strong>Stage '${escapeHtml(task.failed_stage)}' failed:</strong>
              <span>${escapeHtml(task.stage_error ?? "Unknown error")}</span>
            </div>
            <button
              type="button"
              id="retry-stage-btn"
              data-task-id="${escapeHtml(task.task_id)}"
              class="btn btn--primary btn--sm"
              ${state.runningPipeline ? "disabled" : ""}
            >
              ↻ Retry Stage '${escapeHtml(task.failed_stage)}' Only
            </button>
          </div>`
        : ""
    }

    <!-- Stage 1: Cloud Interpret Plan -->
    ${
      task.plan
        ? `<section class="panel-card">
            <button type="button" class="copy-btn-corner" data-copy-text="${escapeHtml(
              `Goal: ${task.plan.goal}\nRisk: ${task.plan.estimated_risk}\nFiles: ${task.plan.files_involved.join(", ")}\n` +
              task.plan.steps.map((s) => `Step #${s.id} [${s.action_class} / ${s.route_tool}] on ${s.target_path}: ${s.instruction}`).join("\n")
            )}" data-copy-label="Cloud Plan">Copy</button>
            <div class="panel-header">
              <div class="row-inline">
                <h2 class="panel-title">2. Plan (${escapeHtml(task.cloud_model)})</h2>
                ${
                  task.local_grade_plan
                    ? `<span class="badge badge--warn" title="Planned via local model during cloud unavailability">LOCAL-GRADE PLANNING (${renderCharterChip(5, "Local first")})</span>`
                    : ""
                }
                ${
                  task.provider_interpret && !task.local_grade_plan && task.provider_interpret !== task.cloud_model
                    ? `<span class="badge badge--neutral">Provider: ${escapeHtml(task.provider_interpret)}</span>`
                    : ""
                }
              </div>
              <div class="row-inline">
                ${
                  task.plan.memories_used && task.plan.memories_used.length > 0
                    ? `<span class="badge" title="Memories used by cloud plan">Memories: ${escapeHtml(
                        task.plan.memories_used.map((m) => "@" + m).join(", ")
                      )}</span>`
                    : ""
                }
                <span class="${riskBadgeClass(task.plan.estimated_risk)}">Risk: ${escapeHtml(
                  task.plan.estimated_risk
                )}</span>
                <span class="badge badge--neutral">Files: ${escapeHtml(
                  task.plan.files_involved.join(", ") || "none"
                )}</span>
              </div>
            </div>
            <p style="margin:0;"><strong>Goal:</strong> ${escapeHtml(task.plan.goal)}</p>
            <div class="queue-grid">
              ${task.plan.steps
                .map(
                  (s) => `
                  <div class="row-between" style="padding:0.45rem 0.75rem; border:1px solid var(--wiki-border); border-radius:var(--wiki-radius-sm);">
                    <div class="row-inline">
                      <span class="badge">#${s.id}</span>
                      <strong>${escapeHtml(s.title)}</strong>
                      ${
                        s.target_path && s.target_path.trim().length > 0 && s.target_path !== "."
                          ? `<span class="mono muted">${escapeHtml(s.target_path)}</span>`
                          : ""
                      }
                      ${
                        s.question
                          ? `<span class="muted" style="font-size:0.8rem;">— <em>${escapeHtml(s.question)}</em></span>`
                          : ""
                      }
                    </div>
                    <div class="row-inline">
                      ${
                        s.memories_used && s.memories_used.length > 0
                          ? `<span class="badge" title="Step cited memory">@${escapeHtml(s.memories_used.join(", @"))}</span>`
                          : ""
                      }
                      <span class="badge badge--neutral">Action: ${escapeHtml(s.action_class)}</span>
                      <span class="badge">Route: ${escapeHtml(s.route_tool)}</span>
                      <button type="button" class="copy-btn-inline" data-copy-text="${escapeHtml(
                        `Step #${s.id}: ${s.title}\nAction: ${s.action_class}\nRoute: ${s.route_tool}${s.target_path ? `\nTarget: ${s.target_path}` : ""}${s.question ? `\nQuestion: ${s.question}` : ""}\nInstruction: ${s.instruction}`
                      )}" data-copy-label="Plan Step #${s.id}">Copy</button>
                    </div>
                  </div>`
                )
                .join("")}
            </div>
          </section>`
        : ""
    }

    <!-- Stage 2: Local Model Routing & Step Execution Logs -->
    ${
      task.step_logs.length > 0
        ? `<section class="panel-card">
            <div class="panel-header">
              <h2 class="panel-title">3. Local Model Routing & Execution Log</h2>
              <span class="muted">Zero raw file dumps sent to cloud (§1 Hard Rule)</span>
            </div>
            <div class="queue-grid">
              ${task.step_logs
                .map(
                  (log) => `
                  <div class="action-card">
                    <button type="button" class="copy-btn-corner" data-copy-text="${escapeHtml(
                      `Step #${log.step_id} (${log.title})\nModel: ${log.model} (${log.tier})\n${log.summary}`
                    )}" data-copy-label="Step #${log.step_id} Log">Copy</button>
                    <div class="row-between">
                      <div class="row-inline">
                        <span class="badge">Step #${log.step_id}</span>
                        <strong>${escapeHtml(log.title)}</strong>
                        <span class="mono muted">${escapeHtml(log.target_path)}</span>
                      </div>
                      <div class="row-inline">
                        <span class="badge">${escapeHtml(log.tier)}</span>
                        <span class="badge badge--neutral">Model: ${escapeHtml(log.model)}</span>
                        <span class="badge badge--neutral">${escapeHtml(log.route_tool)} (${escapeHtml(
                          log.action_class
                        )})</span>
                      </div>
                    </div>
                    ${
                      log.warning_chip
                        ? `<div class="warning-row">
                            <span class="chip-warning">${escapeHtml(log.warning_chip)}</span>
                            <button type="button" class="btn-user-authored" data-project="${escapeHtml(
                              task.project_dir
                            )}" data-target="${escapeHtml(log.target_path)}">Mark as user-authored</button>
                          </div>`
                        : ""
                    }
                    ${
                      log.memory_delivery_chip
                        ? `<div class="memory-delivery-row">
                            <span class="chip-memory">${escapeHtml(log.memory_delivery_chip)}</span>
                          </div>`
                        : ""
                    }
                    <div class="diff-view">${escapeHtml(log.summary)}</div>
                  </div>`
                )
                .join("")}
            </div>
          </section>`
        : ""
    }

    <!-- Approval Queue & Grant / Deny Cards -->
    ${
      task.action_cards.length > 0
        ? `<section class="panel-card">
            <div class="panel-header">
              <h2 class="panel-title">4. Approval Queue & Action Audit Cards</h2>
              <span class="muted">Journal: <code>${escapeHtml(task.journal_path)}</code></span>
            </div>
            <div class="queue-grid">
              ${task.action_cards
                .map(
                  (card) => `
                  <article class="${cardBorderClass(card.status, card.action_class)}">
                    <button type="button" class="copy-btn-corner" data-copy-text="${escapeHtml(
                      card.diff_or_command.startsWith("---") || card.diff_or_command.includes("\n+++")
                        ? card.diff_or_command
                        : `\`\`\`\n${card.diff_or_command}\n\`\`\``
                    )}" data-copy-label="Diff / Command">Copy</button>
                    <div class="row-between">
                      <div class="row-inline">
                        <span class="badge">Step #${card.step_id}</span>
                        <strong>${escapeHtml(card.title)}</strong>
                        <span class="badge badge--neutral">${escapeHtml(card.action_class)}</span>
                        ${card.request_kind ? `<span class="badge badge--accent">${escapeHtml(card.request_kind.toUpperCase())}</span>` : ""}
                        ${
                          card.target_path && card.target_path.trim().length > 0 && card.target_path !== "."
                            ? `<span class="mono">${escapeHtml(card.target_path)}</span>`
                            : ""
                        }
                      </div>
                      ${cardStatusBadge(card)}
                    </div>
                    ${
                      card.warning_chip
                        ? `<div class="warning-row">
                            <span class="chip-warning">${escapeHtml(card.warning_chip)}</span>
                            ${
                              card.warning_chip.includes("PIPELINE-GENERATED")
                                ? `<button type="button" class="btn-user-authored" data-project="${escapeHtml(
                                    card.directory
                                  )}" data-target="${escapeHtml(card.target_path)}">Mark as user-authored</button>`
                                : ""
                            }
                          </div>`
                        : ""
                    }
                    ${
                      card.memory_delivery_chip
                        ? `<div class="memory-delivery-row">
                            <span class="chip-memory">${escapeHtml(card.memory_delivery_chip)}</span>
                          </div>`
                        : ""
                    }
                    ${
                      card.inconsistency_note
                        ? `<div class="inconsistency-box">
                            <strong>⚠️ Safety Warning:</strong> ${escapeHtml(card.inconsistency_note)}
                          </div>`
                        : ""
                    }

                    ${
                      card.question
                        ? `<div class="request-field request-field--question" style="font-size:0.95rem; margin-block:0.35rem; color:var(--wiki-text);"><strong>Question:</strong> ${escapeHtml(card.question)}</div>`
                        : ""
                    }
                    ${
                      card.answer_format
                        ? `<div class="request-field muted" style="font-size:0.8rem; margin-block-end:0.35rem;"><strong>Format:</strong> <code>${escapeHtml(card.answer_format)}</code></div>`
                        : ""
                    }
                    ${
                      card.justification
                        ? `<div class="request-field"><strong>Justification:</strong> ${escapeHtml(card.justification)}</div>`
                        : ""
                    }
                    ${
                      card.consequences_of_decline
                        ? `<div class="request-field"><strong>Consequences if declined:</strong> ${escapeHtml(card.consequences_of_decline)}</div>`
                        : ""
                    }
                    ${
                      card.user_provided_info
                        ? `<div class="request-field request-field--provided"><strong>Provided Info:</strong> ${escapeHtml(card.user_provided_info)}</div>`
                        : ""
                    }

                    <p class="muted" style="margin:0; font-size:0.8rem;">
                      ${escapeHtml(card.decision_reason)}
                    </p>

                    ${
                      card.diff_or_command && card.diff_or_command.trim().length > 0
                        ? `<div class="diff-view">${renderDiffLines(card.diff_or_command)}</div>`
                        : ""
                    }

                    ${
                      card.execution_output
                        ? `<div class="diff-view"><strong>Execution Output:</strong>\n${escapeHtml(
                            card.execution_output
                          )}</div>`
                        : ""
                    }

                    ${
                      card.status === "pending_approval" && !state.readOnlyHistorical
                        ? card.action_class === "request" && card.request_kind === "information"
                          ? `<div class="request-info-input-area" style="margin-block-start:0.5rem; display:flex; flex-direction:column; gap:0.4rem;">
                              ${
                                card.suggested_default
                                  ? `<div class="row-between" style="background:var(--wiki-bg-card); padding:0.4rem 0.6rem; border:1px solid var(--wiki-border); border-radius:var(--wiki-radius-sm); font-size:0.82rem;">
                                      <span><strong>Suggested:</strong> <code>${escapeHtml(card.suggested_default)}</code></span>
                                      <button type="button" class="btn btn--sm use-suggestion-btn" data-card-id="${escapeHtml(card.card_id)}" data-suggestion="${escapeHtml(card.suggested_default)}">
                                        Use suggestion
                                      </button>
                                    </div>`
                                  : ""
                              }
                              <textarea
                                class="form-input request-info-textarea"
                                data-card-id="${escapeHtml(card.card_id)}"
                                placeholder="Enter requested information here..."
                                rows="2"
                                style="width:100%; font-family:var(--wiki-font-mono); font-size:0.82rem;"
                              ></textarea>
                              <div class="row-between">
                                <span class="muted" style="font-size:0.8rem;">Stays local; only concise excerpt sent to verify.</span>
                                <div class="row-inline">
                                  <button
                                    type="button"
                                    class="btn btn--ok provide-info-btn"
                                    data-task-id="${escapeHtml(task.task_id)}"
                                    data-card-id="${escapeHtml(card.card_id)}"
                                  >
                                    ✓ Provide Information
                                  </button>
                                  <button
                                    type="button"
                                    class="btn btn--bad skip-info-btn"
                                    data-task-id="${escapeHtml(task.task_id)}"
                                    data-card-id="${escapeHtml(card.card_id)}"
                                  >
                                    ↷ Skip
                                  </button>
                                </div>
                              </div>
                            </div>`
                          : card.action_class === "request" && card.request_kind === "capability"
                          ? `<div class="row-between" style="margin-block-start:0.5rem;">
                              <span class="muted" style="font-size:0.8rem;">Grants task-scoped access to external path. config.toml remains untouched.</span>
                              <div class="row-inline">
                                <button
                                  type="button"
                                  class="btn btn--ok approve-card-btn"
                                  data-task-id="${escapeHtml(task.task_id)}"
                                  data-card-id="${escapeHtml(card.card_id)}"
                                >
                                  ✓ Grant (Task-Scoped)
                                </button>
                                <button
                                  type="button"
                                  class="btn btn--bad reject-card-btn"
                                  data-task-id="${escapeHtml(task.task_id)}"
                                  data-card-id="${escapeHtml(card.card_id)}"
                                >
                                  ✕ Decline
                                </button>
                              </div>
                            </div>`
                          : `<div class="row-between" style="margin-block-start:0.25rem;">
                              <div class="row-inline" style="flex:1 1 320px;">
                                <input
                                  type="text"
                                  class="form-input reject-reason-input"
                                  data-card-id="${escapeHtml(card.card_id)}"
                                  placeholder="Reason for rejection (logged to journal.jsonl)..."
                                  style="flex:1;"
                                />
                              </div>
                              <div class="row-inline">
                                <button
                                  type="button"
                                  class="btn btn--ok approve-card-btn"
                                  data-task-id="${escapeHtml(task.task_id)}"
                                  data-card-id="${escapeHtml(card.card_id)}"
                                >
                                  ✓ Approve & Execute
                                </button>
                                <button
                                  type="button"
                                  class="btn btn--bad reject-card-btn"
                                  data-task-id="${escapeHtml(task.task_id)}"
                                  data-card-id="${escapeHtml(card.card_id)}"
                                >
                                  ✕ Reject
                                </button>
                              </div>
                            </div>`
                        : ""
                    }
                  </article>`
                )
                .join("")}
            </div>
          </section>`
        : ""
    }

    <!-- Stage 3: Verification Verdict -->
    ${
      task.verification
        ? `<section class="panel-card">
            <button type="button" class="copy-btn-corner" data-copy-text="${escapeHtml(
              `Verdict: ${task.verification.verdict.toUpperCase()}\n\n${task.verification.rationale}`
            )}" data-copy-label="Verification Verdict">Copy</button>
            <div class="panel-header">
              <div class="row-inline">
                <h2 class="panel-title">5. Verification Verdict (${escapeHtml(task.cloud_model)})</h2>
                ${
                  task.local_grade_verify
                    ? `<span class="badge badge--warn" title="Verified via local model during cloud unavailability">LOCAL-GRADE VERIFICATION (${renderCharterChip(5, "Local first")})</span>`
                    : ""
                }
                ${
                  task.provider_verify && !task.local_grade_verify && task.provider_verify !== task.cloud_model
                    ? `<span class="badge badge--neutral">Provider: ${escapeHtml(task.provider_verify)}</span>`
                    : ""
                }
              </div>
              <span class="${verdictBadgeClass(task.verification.verdict)}">
                Verdict: ${escapeHtml(task.verification.verdict.toUpperCase())}
              </span>
            </div>
            <p style="margin:0;">${escapeHtml(task.verification.rationale)}</p>
          </section>`
        : ""
    }
  `;
}

function renderJournalPanel(): string {
  return `
    <section class="panel-card">
      <div class="panel-header">
        <h2 class="panel-title">Local Decision Journal (<code>~/.local/share/kodiak/journal.jsonl</code>)</h2>
        <span class="muted">Showing latest ${state.journal.length} entries</span>
      </div>
      ${
        state.journal.length === 0
          ? `<p class="muted">No journal entries logged yet.</p>`
          : `<div class="queue-grid">
              ${state.journal
                .map(
                  (j) => `
                  <div class="row-between" style="padding:0.45rem 0.75rem; border:1px solid var(--wiki-border); border-radius:var(--wiki-radius-sm); font-size:0.78rem;">
                    <div class="row-inline">
                      <span class="badge ${
                        j.decision === "denied" || j.decision === "rejected"
                          ? "badge--bad"
                          : j.decision === "accepted" || j.decision === "grant_auto_executed"
                          ? "badge--ok"
                          : "badge--warn"
                      }">${escapeHtml(j.decision)}</span>
                      <span class="badge badge--neutral">${escapeHtml(j.action_class)}</span>
                      <span class="mono">${escapeHtml(j.target_path)}</span>
                      <span class="muted">— ${escapeHtml(j.reason)}</span>
                    </div>
                    <div class="row-inline muted mono">
                      <span>[${escapeHtml(j.grant_level)} | Cruise=${j.cruise_control ? "ON" : "OFF"}]</span>
                      <span>${escapeHtml(j.timestamp.slice(0, 19))}</span>
                    </div>
                  </div>`
                )
                .join("")}
            </div>`
      }
    </section>
  `;
}

function renderWikiTab(): string {
  const h = state.health;
  const cfg = state.config;
  if (!h || !cfg) return "";

  if (!h.wiki_reachable || !h.wiki_url) {
    return `
      <section class="panel-card" style="margin:1.25rem;">
        <div class="panel-header">
          <h2 class="panel-title">Projects Wiki Offline</h2>
          <span class="badge badge--bad">127.0.0.1:80 / :8000 Unreachable</span>
        </div>
        <p class="muted">
          Kodiak embeds the live Projects Wiki from <code>${escapeHtml(cfg.wiki_dir)}</code>.
          The background MkDocs + Sync API server is currently stopped.
        </p>
        <div class="row-inline">
          <button
            type="button"
            id="start-wiki-btn"
            class="btn btn--primary"
            ${state.startingWiki ? "disabled" : ""}
          >
            ${state.startingWiki ? "Starting Projects Wiki..." : "▶ Start Projects Wiki Server"}
          </button>
          <button type="button" id="recheck-wiki-btn" class="btn">Re-check Connection</button>
        </div>
      </section>
    `;
  }

  return `
    <div style="display:flex; flex-direction:column; block-size:100%; inline-size:100%;">
      <div class="row-between" style="padding:0.4rem 1.25rem; background:var(--wiki-surface); border-block-end:1px solid var(--wiki-border); font-size:0.78rem;">
        <span class="muted">Connected to live Projects Wiki: <code class="mono">${escapeHtml(
          h.wiki_url
        )}</code> (Source: <code>${escapeHtml(cfg.wiki_dir)}</code>)</span>
        <button type="button" id="reload-wiki-iframe-btn" class="btn btn--sm">↻ Reload Wiki</button>
      </div>
      <iframe
        id="projects-wiki-iframe"
        class="wiki-iframe"
        src="${escapeHtml(h.wiki_url)}"
        title="Projects Wiki"
      ></iframe>
    </div>
  `;
}

function renderCascadeOrderList(cfg: AppConfig): string {
  const allProviders = [
    { id: "primary", name: "Lumo (Primary Cloud)", tier: cfg.tier || "medium" },
    ...(cfg.alt_provider ?? []).map((p) => ({
      id: p.name,
      name: `${p.name} (Alternate)`,
      tier: p.tier || "medium",
    })),
  ];

  const orderIds =
    cfg.cascade?.order && cfg.cascade.order.length > 0
      ? cfg.cascade.order
      : allProviders.map((p) => p.id);

  const orderedList: Array<{ id: string; name: string; tier: string }> = [];
  for (const id of orderIds) {
    const found = allProviders.find((p) => p.id === id);
    if (found && !orderedList.some((x) => x.id === found.id)) {
      orderedList.push(found);
    }
  }
  for (const p of allProviders) {
    if (!orderedList.some((x) => x.id === p.id)) {
      orderedList.push(p);
    }
  }

  if (orderedList.length === 0) {
    return `<p class="muted">No providers configured for cascade.</p>`;
  }

  return orderedList
    .map(
      (item, idx) => `
      <div class="cascade-order-item" data-provider-id="${escapeHtml(item.id)}">
        <div class="row-inline">
          <span class="badge badge--neutral">#${idx + 1}</span>
          <strong>${escapeHtml(item.name)}</strong>
          <span class="badge">Tier: ${escapeHtml(item.tier)}</span>
          ${item.id === "primary" ? `<span class="badge badge--ok">Zero-Access</span>` : ""}
        </div>
        <div class="row-inline">
          <button
            type="button"
            class="btn btn--sm btn-cascade-up"
            data-idx="${idx}"
            ${idx === 0 ? "disabled" : ""}
            title="Move earlier in failover order"
          >▲ Up</button>
          <button
            type="button"
            class="btn btn--sm btn-cascade-down"
            data-idx="${idx}"
            ${idx === orderedList.length - 1 ? "disabled" : ""}
            title="Move later in failover order"
          >▼ Down</button>
        </div>
      </div>
    `
    )
    .join("");
}

function renderSettingsTab(): string {
  const cfg = state.config!;

  return `
    ${
      state.uiBannerError
        ? `<div class="stage-alert">
            <span><strong>Error:</strong> ${escapeHtml(state.uiBannerError)}</span>
            <button type="button" id="dismiss-error-btn" class="btn btn--sm">Dismiss</button>
          </div>`
        : ""
    }
    ${
      state.uiBannerInfo
        ? `<div class="posture-banner is-cruise">
            <span>${escapeHtml(state.uiBannerInfo)}</span>
            <button type="button" id="dismiss-info-btn" class="btn btn--sm">Dismiss</button>
          </div>`
        : ""
    }

    <!-- Project Whitelist & Per-Directory Grant Matrix -->
    <section class="panel-card">
      <div class="panel-header">
        <div class="row-inline">
          <h2 class="panel-title">Project Whitelist & Grant Matrix</h2>
          ${renderCharterChip(2, "Human authority")}
        </div>
        <span class="muted">Only whitelisted directories are readable/writable. Levels compose downward only.</span>
      </div>

      <div class="row-inline" style="align-items:flex-end;">
        <div class="form-field" style="flex:1 1 360px;">
          <label class="form-label" for="new-whitelist-path">Directory Path (absolute or ~/...)</label>
          <input
            type="text"
            id="new-whitelist-path"
            class="form-input"
            placeholder="e.g., ~/Projects/MyProject"
          />
        </div>
        <div class="form-field">
          <label class="form-label" for="new-whitelist-level">Grant Level</label>
          <select id="new-whitelist-level" class="form-select">
            <option value="READ">READ — Read & propose only; all writes queue</option>
            <option value="CREATE">CREATE — READ + create new files named in plan (never overwrite)</option>
            <option value="BUILD">BUILD — CREATE + mkdir + build/test commands</option>
            <option value="AUTONOMOUS">AUTONOMOUS — Full R/W/X inside tree (bounded by deny list)</option>
          </select>
        </div>
        <button type="button" id="add-whitelist-btn" class="btn btn--primary">+ Add to Whitelist</button>
      </div>

      ${
        cfg.whitelist.length === 0
          ? `<p class="muted">Whitelist is currently empty (<code>whitelist = []</code> by default).</p>`
          : `<div class="queue-grid">
              ${cfg.whitelist
                .map(
                  (entry, idx) => `
                  <div class="row-between" style="padding:0.65rem 0.9rem; border:1px solid var(--wiki-border); border-radius:var(--wiki-radius-sm);">
                    <div class="row-inline">
                      <span class="mono"><strong>${escapeHtml(entry.path)}</strong></span>
                      ${
                        cfg.active_project === entry.path
                          ? `<span class="badge badge--ok">Active Project</span>`
                          : `<button type="button" class="btn btn--sm set-active-proj-btn" data-path="${escapeHtml(
                              entry.path
                            )}">Set Active</button>`
                      }
                    </div>
                    <div class="row-inline">
                      <select class="form-select whitelist-level-select" data-idx="${idx}">
                        <option value="READ" ${entry.level === "READ" ? "selected" : ""}>READ</option>
                        <option value="CREATE" ${entry.level === "CREATE" ? "selected" : ""}>CREATE</option>
                        <option value="BUILD" ${entry.level === "BUILD" ? "selected" : ""}>BUILD</option>
                        <option value="AUTONOMOUS" ${
                          entry.level === "AUTONOMOUS" ? "selected" : ""
                        }>AUTONOMOUS</option>
                      </select>
                      <button type="button" class="btn btn--bad btn--sm remove-whitelist-btn" data-idx="${idx}">Remove</button>
                    </div>
                  </div>`
                )
                .join("")}
            </div>`
      }
    </section>

    <!-- Cloud & Providers ([[alt_provider]]) -->
    <section class="panel-card">
      <div class="panel-header">
        <div class="row-inline">
          <h2 class="panel-title">Cloud & Providers ([[alt_provider]])</h2>
          ${renderCharterChip(1, "Your files stay home")}
        </div>
        <span class="muted">Configured OpenAI-compatible endpoints for standby & failover cascade.</span>
      </div>
      <p class="muted" style="margin:0 0 0.75rem 0; font-size:0.85rem;">
        Alternate providers are standby endpoints invoked when cloud quota is exhausted or via automated failover cascade. ${renderCharterChip(1, "Privacy Note")}
      </p>

      <div class="row-inline" style="display:grid; grid-template-columns:repeat(auto-fit, minmax(180px, 1fr)); gap:0.5rem; align-items:flex-end;">
        <div class="form-field">
          <label class="form-label" for="new-alt-preset">Preset</label>
          <select id="new-alt-preset" class="form-select">
            <option value="">-- Choose Preset --</option>
            <option value="gemini">Gemini (OpenAI-compat)</option>
            <option value="lumo">Lumo (lumo-tamer bridge)</option>
            <option value="groq">Groq</option>
            <option value="openrouter">OpenRouter</option>
            <option value="ollama">Self-hosted Ollama</option>
          </select>
        </div>
        <div class="form-field">
          <label class="form-label" for="new-alt-name">Name</label>
          <input type="text" id="new-alt-name" class="form-input" placeholder="e.g. Gemini" />
        </div>
        <div class="form-field">
          <label class="form-label" for="new-alt-url">Base URL</label>
          <input type="text" id="new-alt-url" class="form-input" placeholder="https://.../v1" />
        </div>
        <div class="form-field">
          <label class="form-label" for="new-alt-key">API Key</label>
          <input type="password" id="new-alt-key" class="form-input" placeholder="API key (stored locally)" />
        </div>
        <div class="form-field">
          <label class="form-label" for="new-alt-tier">Tier</label>
          <select id="new-alt-tier" class="form-select">
            <option value="low">Low (Fast / Cheap)</option>
            <option value="medium" selected>Medium (Standard)</option>
            <option value="high">High (Premium / Deep)</option>
          </select>
        </div>
        <div class="form-field">
          <label class="form-label" for="new-alt-model">Model</label>
          <input type="text" id="new-alt-model" class="form-input" placeholder="e.g. gemini-2.5-flash" />
        </div>
        <div class="form-field">
          <label class="form-label" for="new-alt-max-chars">Max Context Chars</label>
          <input type="number" id="new-alt-max-chars" class="form-input" placeholder="Optional" />
        </div>
        <div>
          <button type="button" id="add-alt-provider-btn" class="btn btn--primary" style="width:100%;">+ Add Provider</button>
        </div>
      </div>

      ${
        !cfg.alt_provider || cfg.alt_provider.length === 0
          ? `<p class="muted" style="margin-top:0.75rem;">No alternate providers configured. Standby failover and Choice Cards will offer Defer and Local options only.</p>`
          : `<div class="queue-grid" style="margin-top:0.75rem;">
              ${cfg.alt_provider
                .map(
                  (p, idx) => `
                  <div class="row-between" style="padding:0.65rem 0.9rem; border:1px solid var(--wiki-border); border-radius:var(--wiki-radius-sm); gap:0.75rem; align-items:flex-start;">
                    <div style="display:flex; flex-direction:column; gap:0.3rem; flex:1;">
                      <div class="row-inline">
                        <span class="badge badge--neutral"><strong>${escapeHtml(p.name)}</strong></span>
                        <span class="badge">Tier: ${escapeHtml(p.tier || "medium")}</span>
                        <span class="badge">Model: ${escapeHtml(p.model)}</span>
                        <span class="mono muted">${escapeHtml(p.base_url)}</span>
                        ${p.max_context_chars ? `<span class="badge badge--neutral">Max: ${p.max_context_chars} chars</span>` : ""}
                      </div>
                      <div class="muted" style="font-size:0.8rem;">
                        Endpoint slot: completion queries to <code>${escapeHtml(p.base_url)}</code> with model <code>${escapeHtml(p.model)}</code>.
                      </div>
                      <div id="test-provider-result-${idx}" style="font-size:0.8rem; margin-top:0.2rem;"></div>
                    </div>
                    <div class="row-inline" style="align-self:center;">
                      <button type="button" class="btn btn--sm test-alt-provider-btn" data-idx="${idx}">
                        Test connection
                      </button>
                      <button type="button" class="btn btn--bad btn--sm remove-alt-provider-btn" data-idx="${idx}">Remove</button>
                    </div>
                  </div>`
                )
                .join("")}
            </div>`
      }

      <!-- Failover cascade panel (§2) -->
      <div style="margin-top:1.25rem; border-top:1px solid var(--wiki-border); padding-top:1rem;">
        <div class="panel-header" style="margin-bottom:0.4rem;">
          <div class="row-inline">
            <h3 style="margin:0; font-size:1rem; font-weight:600;">Failover Cascade</h3>
            <span class="badge ${cfg.cascade?.enabled ? "badge--ok" : "badge--neutral"}">
              ${cfg.cascade?.enabled ? "Enabled" : "Disabled (Default)"}
            </span>
          </div>
          <span class="muted" style="font-size:0.8rem;">Ordered failover when cloud quota or retryable errors occur</span>
        </div>

        <div class="stage-alert" style="margin-bottom:0.75rem;">
          <div>
            <strong>Privacy Governance:</strong> Automatic failover sends task prompts to providers further down this list without asking each time. Alternate providers do not inherit the privacy guarantee — see ${renderCharterChip(1, "Your files stay home")}.
          </div>
          <label style="display:flex; align-items:center; gap:0.5rem; margin-top:0.35rem; cursor:pointer;">
            <input type="checkbox" id="cfg-cascade-consent" ${cfg.cascade?.consent ? "checked" : ""} />
            <span><strong>Explicit Consent:</strong> I understand and consent to automatic failover routing task prompts to alternate providers.</span>
          </label>
        </div>

        <div style="margin-bottom:0.75rem;">
          <label class="row-inline" style="cursor:pointer;">
            <input type="checkbox" id="cfg-cascade-enabled" ${cfg.cascade?.enabled ? "checked" : ""} />
            <span><strong>Enable Cascade:</strong> Automatically try standby providers in sequence before falling back to manual Choice Card.</span>
          </label>
        </div>

        <div class="muted" style="font-size:0.8rem; margin-bottom:0.4rem;">
          Failover evaluation sequence (top to bottom):
        </div>
        <div class="queue-grid" id="cascade-order-list">
          ${renderCascadeOrderList(cfg)}
        </div>
      </div>

      <!-- Setup guides collapsible -->
      <details style="margin-top:1rem; border:1px solid var(--wiki-border); border-radius:var(--wiki-radius-sm); background:var(--wiki-surface); padding:0.5rem 0.75rem;">
        <summary style="cursor:pointer; font-weight:600; padding:0.25rem 0;">📖 Setup Guides (Lumo, Gemini, Alternate Providers & Privacy)</summary>
        <div style="display:flex; flex-direction:column; gap:0.75rem; margin-top:0.5rem; font-size:0.85rem; line-height:1.45;">
          <div>
            <strong>1. Lumo via lumo-tamer (Primary Cloud)</strong>
            <p style="margin:0.25rem 0 0 0;" class="muted">
              Lumo is Proton's privacy-first language model. <code>lumo-tamer</code> is a local bridge listening on <code>http://127.0.0.1:3003/v1</code> translating OpenAI chat completions to Proton's internal API. Tiers: Standard (<code>lumo</code>), Lite (<code>lumo-lite</code>), Max (<code>lumo-max</code>). Pre-wired out of the box.
            </p>
          </div>
          <div>
            <strong>2. Gemini via OpenAI-Compatible Endpoint</strong>
            <p style="margin:0.25rem 0 0 0;" class="muted">
              Google Gemini offers an OpenAI-compatible endpoint at <code>https://generativelanguage.googleapis.com/v1beta/openai/</code>.
              <br/>• <strong>Tier Low (Fast/Efficient):</strong> <code>gemini-2.5-flash</code>
              <br/>• <strong>Tier High (Deep Reasoning):</strong> <code>gemini-2.5-pro</code>
              <br/>Use your Google AI Studio API key. Stored locally in <code>~/.config/kodiak/config.toml</code>.
            </p>
          </div>
          <div>
            <strong>3. Any OpenAI-Compatible Endpoint (Standby)</strong>
            <p style="margin:0.25rem 0 0 0;" class="muted">
              You can configure any API endpoint that speaks OpenAI JSON (<code>POST /chat/completions</code>).
              <br/>• <strong>Self-hosted Ollama</strong>: URL <code>http://192.168.1.x:11434/v1</code>, Key: <code>ollama</code>, Model: e.g. <code>qwen2.5-coder:14b</code>.
              <br/>• <strong>Groq / OpenRouter</strong>: URL <code>https://api.groq.com/openai/v1</code> or <code>https://openrouter.ai/api/v1</code>, Key: your secret API key, Model: e.g. <code>llama-3.3-70b-versatile</code>.
            </p>
          </div>
          <div>
            <strong>4. Privacy Governance (The Charter vs Alternate Providers)</strong>
            <p style="margin:0.25rem 0 0 0;" class="muted">
              Kodiak's core privacy rule ensures raw file dumps never leave your machine, and only Proton's zero-access privacy infrastructure handles default cloud plan generation. <strong>Alternate providers do not inherit the privacy guarantee</strong>: when you choose an alternate provider on a Choice Card or via cascade, your task prompt and plan context are transmitted to that provider. (See ${renderCharterChip(1, "Tenet 1")} and ${renderCharterChip(5, "Tenet 5")}).
            </p>
          </div>
        </div>
      </details>
    </section>

    <!-- Diagnostics & Run Logs (§3) -->
    <section class="panel-card" id="settings-diagnostics-panel">
      <div class="panel-header">
        <div class="row-inline">
          <h2 class="panel-title">Diagnostics & Run Logs</h2>
          ${renderCharterChip(6, "Paper trail")}
        </div>
        <span class="muted">Structured per-run logs saved to <code>~/.local/share/kodiak/runs/&lt;task-id&gt;/run.log</code></span>
      </div>
      <p class="muted" style="margin:0 0 0.75rem 0; font-size:0.85rem;">
        Captures stage transitions, model routing, memory delivery chips, cascade attempts, and error classifications concurrently with the Live Event Ticker. Retention pruning cleans oldest run logs only — task history and decision journals are never touched.
      </p>

      <div style="display:grid; grid-template-columns:repeat(auto-fit, minmax(220px, 1fr)); gap:0.85rem;">
        <div class="form-field">
          <label class="form-label" for="cfg-run-log-level">Log Level</label>
          <select id="cfg-run-log-level" class="form-select">
            <option value="debug" ${cfg.run_log_level === "debug" ? "selected" : ""}>debug (verbose, payloads & timings)</option>
            <option value="info" ${(cfg.run_log_level ?? "info") === "info" ? "selected" : ""}>info (default)</option>
            <option value="warn" ${cfg.run_log_level === "warn" ? "selected" : ""}>warn</option>
            <option value="error" ${cfg.run_log_level === "error" ? "selected" : ""}>error</option>
          </select>
        </div>

        <div class="form-field">
          <label class="form-label" for="cfg-run-log-max-mb">Max Log Storage (MB)</label>
          <input
            type="number"
            id="cfg-run-log-max-mb"
            class="form-input"
            min="5"
            max="1000"
            value="${cfg.run_log_max_mb ?? 50}"
          />
        </div>

        <div class="form-field">
          <label class="form-label" for="cfg-run-log-keep-runs">Keep Maximum Runs</label>
          <input
            type="number"
            id="cfg-run-log-keep-runs"
            class="form-input"
            min="1"
            max="500"
            value="${cfg.run_log_keep_runs ?? 20}"
          />
        </div>
      </div>
    </section>

    <!-- Model & Execution Settings -->
    <section class="panel-card">
      <div class="panel-header">
        <h2 class="panel-title">Pipeline, Models & Safety Settings</h2>
        <span class="muted">Saved to <code>~/.config/kodiak/config.toml</code></span>
      </div>

      <div style="display:grid; grid-template-columns:repeat(auto-fit, minmax(260px, 1fr)); gap:0.85rem;">
        <!-- Two-level Cloud Model picker (§1) -->
        <div class="form-field">
          <label class="form-label" for="cfg-cloud-provider">Cloud Provider</label>
          <select id="cfg-cloud-provider" class="form-select">
            <option value="primary" selected>Lumo (Primary Bridge)</option>
            ${(cfg.alt_provider ?? []).map(
              (p) => `<option value="${escapeHtml(p.name)}">${escapeHtml(p.name)} (Alternate)</option>`
            ).join("")}
          </select>
        </div>

        <div class="form-field">
          <label class="form-label" for="cfg-cloud-tier">Model Tier</label>
          <select id="cfg-cloud-tier" class="form-select">
            <option value="low" ${cfg.tier === "low" || cfg.cloud_model === "lumo-lite" ? "selected" : ""}>Low (Fast / Lite)</option>
            <option value="medium" ${!cfg.tier || cfg.tier === "medium" || cfg.cloud_model === "lumo" ? "selected" : ""}>Medium (Standard)</option>
            <option value="high" ${cfg.tier === "high" || cfg.cloud_model === "lumo-max" ? "selected" : ""}>High (Premium / Max)</option>
          </select>
        </div>

        <div class="form-field">
          <label class="form-label" for="cfg-cloud-model">Resolved Cloud Model ID</label>
          <input type="text" id="cfg-cloud-model" class="form-input" value="${escapeHtml(cfg.cloud_model)}" placeholder="e.g. lumo" />
        </div>

        <div class="form-field">
          <label class="form-label" for="cfg-lumo-url">Lumo Bridge URL</label>
          <input type="text" id="cfg-lumo-url" class="form-input" value="${escapeHtml(cfg.lumo_url)}" />
        </div>

        <div class="form-field">
          <label class="form-label" for="cfg-ollama-url">Ollama URL</label>
          <input type="text" id="cfg-ollama-url" class="form-input" value="${escapeHtml(cfg.ollama_url)}" />
        </div>

        <div class="form-field">
          <label class="form-label" for="cfg-fast-model">Local Fast/Indexing Model</label>
          <input type="text" id="cfg-fast-model" class="form-input" value="${escapeHtml(cfg.fast_model)}" />
        </div>

        <div class="form-field">
          <label class="form-label" for="cfg-coder-model">Local Default Coder Model</label>
          <input type="text" id="cfg-coder-model" class="form-input" value="${escapeHtml(cfg.coder_model)}" />
        </div>

        <div class="form-field">
          <label class="form-label" for="cfg-heavy-model">Local Heavy Coder Model</label>
          <input type="text" id="cfg-heavy-model" class="form-input" value="${escapeHtml(cfg.heavy_model)}" />
        </div>

        <div class="form-field">
          <label class="form-label" for="cfg-timeout">Shell Command Timeout (seconds)</label>
          <input
            type="number"
            id="cfg-timeout"
            class="form-input"
            min="5"
            max="600"
            value="${cfg.command_timeout_secs}"
          />
        </div>

        <div class="form-field">
          <label class="form-label" for="cfg-wiki-dir">Projects Wiki On-Disk Directory</label>
          <input type="text" id="cfg-wiki-dir" class="form-input" value="${escapeHtml(cfg.wiki_dir)}" />
        </div>
      </div>

      <div style="display:flex; flex-direction:column; gap:0.5rem; margin-block-start:0.75rem;">
        <label class="row-inline" style="cursor:pointer;">
          <input type="checkbox" id="cfg-memory-auto-sanitize" ${cfg.memory_auto_sanitize ?? true ? "checked" : ""} />
          <span><strong>Memory Auto-Sanitize:</strong> Deterministic write-time heuristic pass to strip ASCII art logos & ANSI escapes from memory files (.orig backup kept)</span>
        </label>

        <label class="row-inline" style="cursor:pointer;">
          <input type="checkbox" id="cfg-dry-run" ${cfg.dry_run ? "checked" : ""} />
          <span><strong>Dry-Run Mode:</strong> Run Interpret → Plan → Local Drafting, but suppress all file writes and shell execution</span>
        </label>
      </div>

      <div class="row-between" style="margin-block-start:0.75rem;">
        <span></span>
        <button type="button" id="save-settings-btn" class="btn btn--primary">Save Settings</button>
      </div>
    </section>
  `;
}

function formatUptime(seconds: number): string {
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = Math.floor(seconds % 60);
  if (h > 0) return `${h}h ${m}m ${s}s`;
  if (m > 0) return `${m}m ${s}s`;
  return `${s}s`;
}

function renderAboutTab(): string {
  const h = state.health;
  const cfg = state.config;
  const uptimeSecs = Math.floor((Date.now() - state.appSessionStartTime) / 1000);

  return `
    <div style="display:flex; flex-direction:column; gap:1.25rem; max-width:960px; margin:0 auto; padding-bottom:3rem;">
      <!-- Hero / Branding -->
      <section class="about-hero">
        <img src="${bearBuiltLogoUrl}" alt="Bear Built mark" class="about-logo-img" />
        <div style="flex:1; min-width:240px;">
          <div class="row-inline" style="gap:0.75rem; align-items:baseline;">
            <h1 style="margin:0; font-size:1.8rem; font-weight:700; color:var(--md-default-fg-color);">Kodiak</h1>
            <span class="badge badge--ok" style="font-size:0.85rem;">v0.7.0</span>
          </div>
          <p style="margin:0.35rem 0 0.5rem 0; font-size:1.05rem; font-weight:500; color:var(--md-default-fg-color);">
            The bear who runs the job site — local-first AI task delegation
          </p>
          <p class="muted" style="margin:0; font-size:0.85rem; line-height:1.45;">
            Zero raw file dumps leave your machine; the cloud plans, the local bear builds.
          </p>
        </div>
      </section>

      <!-- Live Status Block -->
      <section class="panel-card">
        <div class="panel-header">
          <h2 class="panel-title">Live System Status</h2>
          <span class="muted">Live health and active session</span>
        </div>
        <div style="display:grid; grid-template-columns:repeat(auto-fit, minmax(200px, 1fr)); gap:0.75rem;">
          <div style="padding:0.75rem; border:1px solid var(--wiki-border); border-radius:var(--wiki-radius-sm); background:var(--wiki-surface);">
            <div class="muted" style="font-size:0.75rem;">Lumo Cloud Bridge</div>
            <div class="row-inline" style="margin-top:0.25rem;">
              <span class="status-dot ${h?.bridge_reachable ? "is-ok" : "is-bad"}"></span>
              <strong>${h?.bridge_reachable ? "Online (Proton Lumo)" : "Offline"}</strong>
            </div>
            <div class="muted mono" style="font-size:0.75rem; margin-top:0.2rem;">${escapeHtml(cfg?.lumo_url ?? "")}</div>
          </div>

          <div style="padding:0.75rem; border:1px solid var(--wiki-border); border-radius:var(--wiki-radius-sm); background:var(--wiki-surface);">
            <div class="muted" style="font-size:0.75rem;">Local Ollama Engine</div>
            <div class="row-inline" style="margin-top:0.25rem;">
              <span class="status-dot ${h?.ollama_reachable ? "is-ok" : "is-bad"}"></span>
              <strong>${h?.ollama_reachable ? `Online (${h?.ollama_models.length} models)` : "Offline"}</strong>
            </div>
            <div class="muted mono" style="font-size:0.75rem; margin-top:0.2rem;">${escapeHtml(cfg?.ollama_url ?? "")}</div>
          </div>

          <div style="padding:0.75rem; border:1px solid var(--wiki-border); border-radius:var(--wiki-radius-sm); background:var(--wiki-surface);">
            <div class="muted" style="font-size:0.75rem;">Active Project</div>
            <div style="font-weight:600; margin-top:0.25rem; font-size:0.9rem;" class="mono">${escapeHtml(cfg?.active_project ?? "None selected")}</div>
            <div class="muted" style="font-size:0.75rem; margin-top:0.2rem;">Grant: ${escapeHtml(h?.active_project_grant ?? "NONE")}</div>
          </div>

          <div style="padding:0.75rem; border:1px solid var(--wiki-border); border-radius:var(--wiki-radius-sm); background:var(--wiki-surface);">
            <div class="muted" style="font-size:0.75rem;">App-Session Uptime</div>
            <div style="font-weight:600; margin-top:0.25rem; font-size:0.9rem;">${formatUptime(uptimeSecs)}</div>
            <div class="muted" style="font-size:0.75rem; margin-top:0.2rem;">Started at launch</div>
          </div>
        </div>
      </section>

      <!-- The Charter Section -->
      <section class="panel-card" id="the-charter-section">
        <div class="panel-header">
          <div class="row-inline">
            <h2 class="panel-title">The Charter</h2>
            <span class="badge badge--ok">Operating Tenets</span>
          </div>
          <span class="muted">Kodiak's non-negotiable architectural principles & mechanical enforcement</span>
        </div>
        <p class="muted" style="margin:0 0 1rem 0; font-size:0.88rem; line-height:1.45;">
          The Charter is the binding operating contract of Kodiak. Every rule, badge, and permission boundary in the UI links directly to one of these six tenets.
        </p>

        <div class="charter-grid">
          ${CHARTER_TENETS.map((t) => `
            <article class="charter-card" id="charter-${t.id}" style="scroll-margin-top:2rem;">
              <div class="charter-card__header">
                <h3 class="charter-card__title" style="color:var(--wiki-accent);">
                  ${escapeHtml(t.title)}
                </h3>
                <span class="badge badge--neutral">Tenet ${t.number}</span>
              </div>
              <p class="charter-card__statement">
                ${escapeHtml(t.statement)}
              </p>
              <div class="charter-card__enforcement">
                <strong>Enforced by:</strong>
                <span style="margin-left:0.35rem;">
                  ${t.enforcedBy.map((e) => `<span class="badge badge--neutral" style="margin:0.15rem 0.25rem 0.15rem 0;">${escapeHtml(e)}</span>`).join("")}
                </span>
              </div>
            </article>
          `).join("")}
        </div>
      </section>

      <!-- Links & Licenses -->
      <section class="panel-card">
        <div class="panel-header">
          <h2 class="panel-title">Links & Licenses</h2>
        </div>
        <div class="row-between" style="font-size:0.85rem; flex-wrap:wrap; gap:0.75rem;">
          <div class="row-inline">
            <span><strong>App License:</strong> Apache-2.0</span>
            <span>•</span>
            <span><strong>Brand Asset:</strong> Bear Built mark — Aureus</span>
          </div>
          <div class="row-inline muted">
            <span>Kodiak Task Delegation System (v0.7.0)</span>
          </div>
        </div>
      </section>
    </div>
  `;
}

function renderRunLogModal(): string {
  if (!state.activeRunLog) return "";
  const log = state.activeRunLog;

  return `
    <div class="run-log-modal-backdrop" id="run-log-modal-overlay">
      <div class="run-log-modal" role="dialog" aria-modal="true" aria-labelledby="run-log-title">
        <div class="run-log-modal__header">
          <div class="row-inline">
            <h2 id="run-log-title" class="panel-title" style="margin:0;">Diagnostic Run Log</h2>
            <span class="mono muted">[${escapeHtml(log.task_id)}]</span>
            ${log.pruned ? `<span class="badge badge--warn">Pruned</span>` : log.exists ? `<span class="badge badge--ok">Recorded</span>` : `<span class="badge badge--neutral">Unavailable</span>`}
          </div>
          <div class="row-inline">
            ${
              log.content
                ? `<button type="button" class="btn btn--sm" id="copy-run-log-btn" title="Copy full log to clipboard">📋 Copy All</button>`
                : ""
            }
            <button type="button" class="side-icon-btn" id="close-run-log-modal-btn" title="Close run log">✕</button>
          </div>
        </div>
        <div class="run-log-modal__body">
          ${
            log.pruned || !log.exists
              ? `<div class="stage-alert" style="margin-bottom:0.75rem;">
                  <span><strong>Notice:</strong> This diagnostic log was pruned or is unavailable (retention limit reached). Note: Task history and results remain fully preserved. (${renderCharterChip(6, "Paper trail")})</span>
                </div>`
              : ""
          }
          <pre class="run-log-viewer-pre">${escapeHtml(
            log.content || (log.pruned ? "(Run log was pruned per retention limit)" : "(No diagnostic log found for this task)")
          )}</pre>
        </div>
      </div>
    </div>
  `;
}

function renderCloudToast(): string {
  if (!state.cloudToast || !state.cloudToast.show || state.cloudToast.count === 0) return "";
  return `
    <div class="cloud-toast">
      <span><strong>Cloud is back</strong> — ${state.cloudToast.count} deferred task(s) ready.</span>
      <div class="row-inline">
        <button type="button" class="btn btn--sm btn--primary" id="toast-review-btn">Review</button>
        <button type="button" class="btn btn--sm" id="toast-later-btn">Later</button>
      </div>
    </div>
  `;
}

function renderApp(): void {
  const appEl = document.getElementById("app");
  if (!appEl || !state.config) return;

  const isFullBleed = state.tab === "wiki" && state.health?.wiki_reachable;

  let tabHtml = "";
  switch (state.tab) {
    case "workbench":
      tabHtml = renderWorkbenchTab();
      break;
    case "wiki":
      tabHtml = renderWikiTab();
      break;
    case "settings":
      tabHtml = renderSettingsTab() + renderJournalPanel();
      break;
    case "about":
      tabHtml = renderAboutTab();
      break;
  }

  const prevTicker = document.getElementById("ticker-body");
  const prevTop = prevTicker?.scrollTop ?? 0;

  appEl.innerHTML = `
    ${renderSidebar()}
    <div class="app-body">
      ${renderHeader()}
      ${renderHealthStrip()}
      <main class="main-content ${isFullBleed ? "is-fullbleed" : ""}">
        ${tabHtml}
      </main>
      ${renderCloudToast()}
    </div>
    ${renderRunLogModal()}
  `;

  const tk = document.getElementById("ticker-body");
  if (tk) tk.scrollTop = state.tickerLocked || tickerHover ? prevTop : tk.scrollHeight;

  bindEvents();
}

function openTaskReadOnly(taskId: string): void {
  const found = state.history.find((h) => h.task_id === taskId);
  if (!found) return;
  state.currentTask = found;
  state.readOnlyHistorical = true;
  state.tab = "workbench";
  state.uiBannerError = null;
  resetLiveRun(false);
  renderApp();
}

function bindEvents(): void {
  // Global Clipboard copy click handler
  document.querySelectorAll<HTMLElement>("[data-copy-text]").forEach((btn) => {
    btn.addEventListener("click", async (ev) => {
      ev.stopPropagation();
      const txt = btn.getAttribute("data-copy-text") ?? "";
      const lbl = btn.getAttribute("data-copy-label") ?? "Content";
      await copyToClipboard(txt, lbl);
    });
  });

  // Copy full task transcript button
  document.getElementById("copy-task-transcript-btn")?.addEventListener("click", async () => {
    if (state.currentTask) {
      await copyToClipboard(taskAsMarkdown(state.currentTask), "Full task transcript");
    }
  });

  // Copy all ticker lines
  document.getElementById("ticker-copy-all-btn")?.addEventListener("click", async () => {
    const lines = state.ticker.map((t) => `[${t.timestamp.slice(11, 19)}] [${t.level}] ${t.message}`).join("\n");
    await copyToClipboard(lines, "All ticker lines");
  });

  // Mark as user-authored button handler (§2.4 Slice 2.2)
  document.querySelectorAll<HTMLButtonElement>(".btn-user-authored").forEach((btn) => {
    btn.addEventListener("click", async (ev) => {
      ev.stopPropagation();
      const projectDir = btn.getAttribute("data-project") ?? "";
      const targetPath = btn.getAttribute("data-target") ?? "";
      if (!projectDir || !targetPath) return;

      const confirmed = confirm(
        `Mark "${targetPath}" as a user-authored grounding source?\n\nThis records an override in the audit journal so future AI plans treat this file as an authoritative source rather than unverified output.`
      );
      if (!confirmed) return;

      try {
        await invoke("mark_file_user_authored", { projectDir, targetPath });
        state.uiBannerInfo = `Marked "${targetPath}" as user-authored in journal.`;
        if (btn.parentElement) {
          btn.parentElement.innerHTML = `<span class="chip-user-authored">USER-AUTHORED (OVERRIDE)</span>`;
        }
      } catch (err) {
        state.uiBannerError = `Failed to mark file as user-authored: ${err}`;
        renderApp();
      }
    });
  });

  // Refresh memories from disk
  document.getElementById("refresh-memories-btn")?.addEventListener("click", async () => {
    try {
      state.memories = await invoke<MemoryEntry[]>("get_memories");
      state.uiBannerInfo = "Rescanned memories from ~/.local/share/kodiak/memory/.";
      renderApp();
    } catch (err) {
      state.uiBannerError = String(err);
      renderApp();
    }
  });

  // Open New Memory modal
  const handleOpenNewMemory = () => {
    state.activeMemoryModal = {
      id: "",
      title: "",
      tags: [],
      scope: state.config?.active_project ? `project:${state.config.active_project}` : "global",
      cloud_share: false,
      updated: new Date().toISOString(),
      body: "",
      warning: null,
    };
    state.isEditingMemory = true;
    renderApp();
  };
  document.getElementById("add-memory-btn")?.addEventListener("click", handleOpenNewMemory);
  document.getElementById("quick-new-memory-btn")?.addEventListener("click", handleOpenNewMemory);

  // View memory modal
  document.querySelectorAll<HTMLElement>("[data-view-memory]").forEach((el) => {
    el.addEventListener("click", () => {
      const id = el.getAttribute("data-view-memory");
      const found = state.memories.find((m) => m.id === id);
      if (found) {
        state.activeMemoryModal = found;
        state.isEditingMemory = false;
        renderApp();
      }
    });
  });

  // Close / Edit from view memory modal
  document.getElementById("view-mem-close-btn")?.addEventListener("click", () => {
    state.activeMemoryModal = null;
    renderApp();
  });
  document.getElementById("view-mem-edit-btn")?.addEventListener("click", () => {
    state.isEditingMemory = true;
    renderApp();
  });

  // Edit memory modal actions
  document.querySelectorAll<HTMLElement>("[data-edit-memory]").forEach((el) => {
    el.addEventListener("click", (ev) => {
      ev.stopPropagation();
      const id = el.getAttribute("data-edit-memory");
      const found = state.memories.find((m) => m.id === id);
      if (found) {
        state.activeMemoryModal = { ...found };
        state.isEditingMemory = true;
        renderApp();
      }
    });
  });
  document.getElementById("edit-mem-close-btn")?.addEventListener("click", () => {
    state.activeMemoryModal = null;
    state.isEditingMemory = false;
    renderApp();
  });
  document.getElementById("edit-mem-cancel-btn")?.addEventListener("click", () => {
    state.activeMemoryModal = null;
    state.isEditingMemory = false;
    renderApp();
  });

  // Save memory
  document.getElementById("edit-mem-save-btn")?.addEventListener("click", async () => {
    const title = (document.getElementById("mem-edit-title") as HTMLInputElement)?.value.trim() ?? "";
    const id = (document.getElementById("mem-edit-id") as HTMLInputElement)?.value.trim() ?? "";
    const tagsRaw = (document.getElementById("mem-edit-tags") as HTMLInputElement)?.value.trim() ?? "";
    const scope = (document.getElementById("mem-edit-scope") as HTMLSelectElement)?.value ?? "global";
    const cloudShare = (document.getElementById("mem-edit-cloudshare") as HTMLInputElement)?.checked ?? false;
    const body = (document.getElementById("mem-edit-body") as HTMLTextAreaElement)?.value ?? "";

    if (!title) {
      state.uiBannerError = "Please enter a memory title.";
      renderApp();
      return;
    }

    const tags = tagsRaw
      .split(",")
      .map((t) => t.trim())
      .filter(Boolean);

    const slug = id || title.toLowerCase().replace(/[^a-z0-9_-]+/g, "-");

    const entryToSave: MemoryEntry = {
      id: slug,
      title,
      tags,
      scope,
      cloud_share: cloudShare,
      updated: new Date().toISOString(),
      body,
      warning: null,
    };

    try {
      await invoke<MemoryEntry>("save_memory", { entry: entryToSave });
      state.memories = await invoke<MemoryEntry[]>("get_memories");
      state.activeMemoryModal = null;
      state.isEditingMemory = false;
      state.uiBannerInfo = `Saved memory @${slug}.`;
      renderApp();
    } catch (err) {
      state.uiBannerError = String(err);
      renderApp();
    }
  });

  // Delete memory confirmation
  document.querySelectorAll<HTMLElement>("[data-delete-memory]").forEach((el) => {
    el.addEventListener("click", (ev) => {
      ev.stopPropagation();
      state.pendingDeleteMemoryId = el.getAttribute("data-delete-memory");
      renderApp();
    });
  });
  document.getElementById("delete-mem-cancel-btn")?.addEventListener("click", () => {
    state.pendingDeleteMemoryId = null;
    renderApp();
  });
  document.getElementById("delete-mem-confirm-btn")?.addEventListener("click", async () => {
    const id = state.pendingDeleteMemoryId;
    state.pendingDeleteMemoryId = null;
    if (!id) return;
    try {
      state.memories = await invoke<MemoryEntry[]>("delete_memory", { id });
      if (state.activeMemoryModal?.id === id) {
        state.activeMemoryModal = null;
        state.isEditingMemory = false;
      }
      state.uiBannerInfo = `Deleted memory @${id}.`;
      renderApp();
    } catch (err) {
      state.uiBannerError = String(err);
      renderApp();
    }
  });

  // Prompt Mention picking and autocomplete
  document.querySelectorAll<HTMLElement>("[data-pick-mention]").forEach((btn) => {
    btn.addEventListener("click", () => {
      const id = btn.getAttribute("data-pick-mention");
      if (!id) return;
      const promptEl = document.getElementById("task-prompt-input") as HTMLTextAreaElement | null;
      if (!promptEl) return;
      const text = promptEl.value;
      const atIdx = text.lastIndexOf("@");
      if (atIdx >= 0) {
        state.promptText = text.slice(0, atIdx) + `@${id} `;
      } else {
        state.promptText = text + `@${id} `;
      }
      state.mentionQuery = null;
      renderApp();
      const newPromptEl = document.getElementById("task-prompt-input") as HTMLTextAreaElement | null;
      newPromptEl?.focus();
    });
  });

  // Sidebar
  document.getElementById("sidebar-toggle-btn")?.addEventListener("click", () => {
    state.sidebarCollapsed = !state.sidebarCollapsed;
    renderApp();
  });
  document.querySelectorAll<HTMLElement>("[data-select-project]").forEach((el) => {
    el.addEventListener("click", async () => {
      if (!state.config || state.runningPipeline) return;
      state.config.active_project = el.getAttribute("data-select-project");
      state.tab = "workbench";
      await invoke<AppConfig>("update_config", { newConfig: state.config });
      await refreshAllState();
    });
  });
  document.querySelectorAll<HTMLElement>("[data-open-task]").forEach((el) => {
    el.addEventListener("click", () => openTaskReadOnly(el.getAttribute("data-open-task") ?? ""));
  });
  document.querySelectorAll<HTMLElement>("[data-star-task]").forEach((el) => {
    el.addEventListener("click", async (ev) => {
      ev.stopPropagation();
      try {
        await invoke<TaskRecord>("toggle_task_star", { taskId: el.getAttribute("data-star-task") });
        await refreshAllState();
      } catch (err) {
        state.uiBannerError = String(err);
        renderApp();
      }
    });
  });
  document.querySelectorAll<HTMLElement>("[data-delete-task]").forEach((el) => {
    el.addEventListener("click", (ev) => {
      ev.stopPropagation();
      state.pendingDeleteId = el.getAttribute("data-delete-task");
      renderApp();
    });
  });
  document.getElementById("delete-cancel-btn")?.addEventListener("click", () => {
    state.pendingDeleteId = null;
    renderApp();
  });
  document.getElementById("delete-confirm-btn")?.addEventListener("click", async () => {
    const id = state.pendingDeleteId;
    state.pendingDeleteId = null;
    if (!id) return;
    try {
      await invoke<TaskRecord[]>("delete_task_record", { taskId: id });
      if (state.currentTask?.task_id === id) {
        state.currentTask = null;
        state.readOnlyHistorical = false;
      }
      await refreshAllState();
    } catch (err) {
      state.uiBannerError = String(err);
      renderApp();
    }
  });

  // Load into Workbench: prefill prompt + active project, stay editable, leave read-only mode (do not run)
  document.getElementById("load-to-workbench-btn")?.addEventListener("click", async () => {
    const t = state.currentTask;
    if (!t) return;
    state.promptText = t.prompt;
    if (state.config && state.config.whitelist.some((w) => w.path === t.project_dir)) {
      state.config.active_project = t.project_dir;
      await invoke<AppConfig>("update_config", { newConfig: state.config });
    }
    state.readOnlyHistorical = false;
    state.currentTask = null;
    state.stages = { interpret: "idle", execute: "idle", verify: "idle", done: "idle" };
    resetLiveRun(false);
    await refreshAllState();
  });

  // Re-run in same project: prefill prompt + target, leave read-only mode, and execute immediately
  document.getElementById("rerun-btn")?.addEventListener("click", async () => {
    const t = state.currentTask;
    if (!t || !state.config) return;
    state.promptText = t.prompt;
    if (state.config.whitelist.some((w) => w.path === t.project_dir)) {
      state.config.active_project = t.project_dir;
      await invoke<AppConfig>("update_config", { newConfig: state.config });
    }
    state.readOnlyHistorical = false;
    state.currentTask = null;
    state.stages = { interpret: "idle", execute: "idle", verify: "idle", done: "idle" };
    resetLiveRun(false);
    await refreshAllState();
    document.getElementById("run-pipeline-btn")?.click();
  });

  // Ticker scroll lock + hover pause
  document.getElementById("ticker-lock-btn")?.addEventListener("click", () => {
    state.tickerLocked = !state.tickerLocked;
    renderApp();
  });
  const tickerEl = document.getElementById("ticker-body");
  tickerEl?.addEventListener("mouseenter", () => (tickerHover = true));
  tickerEl?.addEventListener("mouseleave", () => {
    tickerHover = false;
    if (!state.tickerLocked && tickerEl) tickerEl.scrollTop = tickerEl.scrollHeight;
  });

  // Navigation tabs (exiting historical mode if clicking Workbench)
  document.querySelectorAll<HTMLElement>("[data-nav]").forEach((btn) => {
    btn.addEventListener("click", () => {
      const target = btn.getAttribute("data-nav") as ActiveTab;
      if (target) {
        state.tab = target;
        if (target === "workbench" && state.readOnlyHistorical) {
          state.readOnlyHistorical = false;
          state.currentTask = null;
        }
        state.uiBannerError = null;
        state.uiBannerInfo = null;
        renderApp();
      }
    });
  });

  // Dismiss banners
  document.getElementById("dismiss-error-btn")?.addEventListener("click", () => {
    state.uiBannerError = null;
    renderApp();
  });
  document.getElementById("dismiss-info-btn")?.addEventListener("click", () => {
    state.uiBannerInfo = null;
    renderApp();
  });

  // Cruise Control toggle
  document.getElementById("cruise-toggle-btn")?.addEventListener("click", async () => {
    try {
      const next = !state.cruiseControl;
      state.cruiseControl = await invoke<boolean>("set_cruise_control", { enabled: next });
      renderApp();
    } catch (err) {
      state.uiBannerError = String(err);
      renderApp();
    }
  });

  // Refresh health
  document.getElementById("refresh-health-btn")?.addEventListener("click", async () => {
    await refreshAllState();
  });

  // Start Lumo Bridge button (in health strip or workbench banner)
  const handleStartBridge = async () => {
    state.startingBridge = true;
    state.uiBannerError = null;
    renderApp();
    try {
      state.health = await invoke<HealthStatus>("start_lumo_bridge");
      state.uiBannerInfo = "Lumo Bridge started and verified healthy via GET /v1/models.";
    } catch (err) {
      state.uiBannerError = String(err);
    } finally {
      state.startingBridge = false;
      renderApp();
    }
  };
  document.getElementById("start-bridge-btn")?.addEventListener("click", handleStartBridge);
  document.getElementById("start-bridge-inline-btn")?.addEventListener("click", handleStartBridge);

  // Prompt input sync & @ mention detection
  const promptEl = document.getElementById("task-prompt-input") as HTMLTextAreaElement | null;
  promptEl?.addEventListener("input", () => {
    state.promptText = promptEl.value;
    const val = promptEl.value;
    const selStart = promptEl.selectionStart ?? val.length;
    const textBeforeCursor = val.slice(0, selStart);
    const lastAt = textBeforeCursor.lastIndexOf("@");
    if (lastAt >= 0) {
      const query = textBeforeCursor.slice(lastAt + 1);
      if (!query.includes(" ")) {
        state.mentionQuery = query;
        const picker = document.getElementById("mention-picker");
        if (!picker) {
          renderApp();
          const p = document.getElementById("task-prompt-input") as HTMLTextAreaElement | null;
          p?.focus();
          p?.setSelectionRange(selStart, selStart);
        } else {
          picker.outerHTML = renderMentionPicker();
          bindMentionPickerItems();
        }
        return;
      }
    }
    if (state.mentionQuery !== null) {
      state.mentionQuery = null;
      const picker = document.getElementById("mention-picker");
      if (picker) picker.remove();
    }
  });

  function bindMentionPickerItems() {
    document.querySelectorAll<HTMLElement>("[data-pick-mention]").forEach((btn) => {
      btn.addEventListener("click", () => {
        const id = btn.getAttribute("data-pick-mention");
        if (!id) return;
        const pEl = document.getElementById("task-prompt-input") as HTMLTextAreaElement | null;
        if (!pEl) return;
        const text = pEl.value;
        const atIdx = text.lastIndexOf("@");
        if (atIdx >= 0) {
          state.promptText = text.slice(0, atIdx) + `@${id} `;
        } else {
          state.promptText = text + `@${id} `;
        }
        state.mentionQuery = null;
        renderApp();
        const newEl = document.getElementById("task-prompt-input") as HTMLTextAreaElement | null;
        newEl?.focus();
      });
    });
  }

  // Quick project selector in Workbench
  const quickProjEl = document.getElementById("active-project-QuickSelect") as HTMLSelectElement | null;
  quickProjEl?.addEventListener("change", async () => {
    if (!state.config) return;
    state.config.active_project = quickProjEl.value || null;
    await invoke<AppConfig>("update_config", { newConfig: state.config });
    await refreshAllState();
  });

  // Exit read-only historical mode
  const exitReadOnly = () => {
    state.readOnlyHistorical = false;
    state.currentTask = null;
    renderApp();
  };
  document.getElementById("exit-readonly-btn")?.addEventListener("click", exitReadOnly);
  document.getElementById("return-workbench-btn")?.addEventListener("click", exitReadOnly);

  // Run pipeline
  document.getElementById("run-pipeline-btn")?.addEventListener("click", async () => {
    const prompt = state.promptText.trim();
    if (!prompt) {
      state.uiBannerError = "Please enter a task prompt before running the pipeline.";
      renderApp();
      return;
    }
    state.runningPipeline = true;
    state.readOnlyHistorical = false;
    state.uiBannerError = null;
    state.uiBannerInfo = null;
    state.stages = { interpret: "idle", execute: "idle", verify: "idle", done: "idle" };
    state.currentTask = null;
    resetLiveRun(false);
    renderApp();

    try {
      const record = await invoke<TaskRecord>("run_task_pipeline", { prompt });
      state.currentTask = record;
      state.liveDraft = record.failed_stage === "execute" ? state.liveDraft : null;
      await refreshAllState();
    } catch (err) {
      state.uiBannerError = String(err);
    } finally {
      state.runningPipeline = false;
      renderApp();
    }
  });

  // Retry failed stage only (§7)
  document.getElementById("retry-stage-btn")?.addEventListener("click", async (ev) => {
    const btn = ev.currentTarget as HTMLElement;
    const taskId = btn.getAttribute("data-task-id");
    if (!taskId) return;
    // Seed progress from the persisted task so the bar re-enters at the retried stage
    const seeded = stagesFromTask(state.currentTask);
    const failed = state.currentTask?.failed_stage as StageName | undefined;
    if (failed) {
      const idx = STAGE_ORDER.indexOf(failed);
      STAGE_ORDER.forEach((s, i) => {
        if (i >= idx) seeded[s] = "idle";
      });
    }
    state.stages = seeded;
    resetLiveRun(true);
    state.runningPipeline = true;
    state.uiBannerError = null;
    renderApp();
    try {
      const updated = await invoke<TaskRecord>("retry_task_stage", { taskId });
      state.currentTask = updated;
      state.liveDraft = updated.failed_stage === "execute" ? state.liveDraft : null;
      await refreshAllState();
    } catch (err) {
      state.uiBannerError = String(err);
    } finally {
      state.runningPipeline = false;
      renderApp();
    }
  });

  // Approve card
  document.querySelectorAll<HTMLElement>(".approve-card-btn").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const taskId = btn.getAttribute("data-task-id");
      const cardId = btn.getAttribute("data-card-id");
      if (!taskId || !cardId) return;
      try {
        const updated = await invoke<TaskRecord>("approve_card", { taskId, cardId });
        state.currentTask = updated;
        await refreshAllState();
      } catch (err) {
        state.uiBannerError = String(err);
        renderApp();
      }
    });
  });

  // Reject card / Decline request
  document.querySelectorAll<HTMLElement>(".reject-card-btn").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const taskId = btn.getAttribute("data-task-id");
      const cardId = btn.getAttribute("data-card-id");
      if (!taskId || !cardId) return;
      const reasonInput = document.querySelector<HTMLInputElement>(
        `.reject-reason-input[data-card-id="${cardId}"]`
      );
      const reason = reasonInput ? reasonInput.value : "Declined by user";
      try {
        const updated = await invoke<TaskRecord>("reject_card", {
          taskId,
          cardId,
          reason,
        });
        state.currentTask = updated;
        await refreshAllState();
      } catch (err) {
        state.uiBannerError = String(err);
        renderApp();
      }
    });
  });

  // Use suggested default for information request
  document.querySelectorAll<HTMLElement>(".use-suggestion-btn").forEach((btn) => {
    btn.addEventListener("click", () => {
      const cardId = btn.getAttribute("data-card-id");
      const suggestion = btn.getAttribute("data-suggestion");
      if (!cardId || suggestion === null) return;
      const textarea = document.querySelector<HTMLTextAreaElement>(
        `.request-info-textarea[data-card-id="${cardId}"]`
      );
      if (textarea) {
        textarea.value = suggestion;
        textarea.focus();
      }
    });
  });

  // Provide information request
  document.querySelectorAll<HTMLElement>(".provide-info-btn").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const taskId = btn.getAttribute("data-task-id");
      const cardId = btn.getAttribute("data-card-id");
      if (!taskId || !cardId) return;
      const textarea = document.querySelector<HTMLTextAreaElement>(
        `.request-info-textarea[data-card-id="${cardId}"]`
      );
      const userInfo = textarea?.value?.trim() ?? "";
      if (!userInfo) {
        state.uiBannerError = "Please enter information before providing.";
        renderApp();
        return;
      }
      try {
        const updated = await invoke<TaskRecord>("provide_request_info", {
          taskId,
          cardId,
          userInfo,
        });
        state.currentTask = updated;
        await refreshAllState();
      } catch (err) {
        state.uiBannerError = String(err);
        renderApp();
      }
    });
  });

  // Skip information request
  document.querySelectorAll<HTMLElement>(".skip-info-btn").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const taskId = btn.getAttribute("data-task-id");
      const cardId = btn.getAttribute("data-card-id");
      if (!taskId || !cardId) return;
      try {
        const updated = await invoke<TaskRecord>("skip_request_info", {
          taskId,
          cardId,
        });
        state.currentTask = updated;
        await refreshAllState();
      } catch (err) {
        state.uiBannerError = String(err);
        renderApp();
      }
    });
  });

  // History: open task as read-only
  document.querySelectorAll<HTMLElement>(".open-history-btn").forEach((btn) => {
    btn.addEventListener("click", () => openTaskReadOnly(btn.getAttribute("data-task-id") ?? ""));
  });

  // Wiki tab controls
  document.getElementById("start-wiki-btn")?.addEventListener("click", async () => {
    state.startingWiki = true;
    state.uiBannerError = null;
    renderApp();
    try {
      state.health = await invoke<HealthStatus>("start_wiki_server");
    } catch (err) {
      state.uiBannerError = String(err);
    } finally {
      state.startingWiki = false;
      renderApp();
    }
  });

  document.getElementById("recheck-wiki-btn")?.addEventListener("click", async () => {
    await refreshAllState();
  });

  document.getElementById("reload-wiki-iframe-btn")?.addEventListener("click", () => {
    const iframe = document.getElementById("projects-wiki-iframe") as HTMLIFrameElement | null;
    if (iframe && state.health?.wiki_url) {
      iframe.src = state.health.wiki_url;
    }
  });

  // Settings: Add whitelist entry
  document.getElementById("add-whitelist-btn")?.addEventListener("click", async () => {
    if (!state.config) return;
    const pathEl = document.getElementById("new-whitelist-path") as HTMLInputElement | null;
    const levelEl = document.getElementById("new-whitelist-level") as HTMLSelectElement | null;
    const rawPath = pathEl?.value.trim() ?? "";
    const level = (levelEl?.value as GrantLevel) ?? "READ";

    if (!rawPath) {
      state.uiBannerError = "Please enter a directory path to whitelist.";
      renderApp();
      return;
    }

    if (!state.config.whitelist.some((w) => w.path === rawPath)) {
      state.config.whitelist.push({ path: rawPath, level });
    }
    if (!state.config.active_project) {
      state.config.active_project = rawPath;
    }
    try {
      state.config = await invoke<AppConfig>("update_config", { newConfig: state.config });
      state.uiBannerInfo = `Added '${rawPath}' [${level}] to Project Whitelist.`;
      await refreshAllState();
    } catch (err) {
      state.uiBannerError = String(err);
      renderApp();
    }
  });

  // Settings: Change whitelist entry level
  document.querySelectorAll<HTMLSelectElement>(".whitelist-level-select").forEach((sel) => {
    sel.addEventListener("change", async () => {
      if (!state.config) return;
      const idx = Number(sel.getAttribute("data-idx"));
      if (!Number.isNaN(idx) && state.config.whitelist[idx]) {
        state.config.whitelist[idx].level = sel.value as GrantLevel;
        state.config = await invoke<AppConfig>("update_config", { newConfig: state.config });
        state.uiBannerInfo = `Updated grant level for '${state.config.whitelist[idx].path}' to ${sel.value}.`;
        await refreshAllState();
      }
    });
  });

  // Settings: Set active project
  document.querySelectorAll<HTMLElement>(".set-active-proj-btn").forEach((btn) => {
    btn.addEventListener("click", async () => {
      if (!state.config) return;
      const p = btn.getAttribute("data-path");
      if (p) {
        state.config.active_project = p;
        state.config = await invoke<AppConfig>("update_config", { newConfig: state.config });
        await refreshAllState();
      }
    });
  });

  // Settings: Remove whitelist entry
  document.querySelectorAll<HTMLElement>(".remove-whitelist-btn").forEach((btn) => {
    btn.addEventListener("click", async () => {
      if (!state.config) return;
      const idx = Number(btn.getAttribute("data-idx"));
      if (!Number.isNaN(idx) && state.config.whitelist[idx]) {
        const removed = state.config.whitelist.splice(idx, 1)[0];
        if (state.config.active_project === removed.path) {
          state.config.active_project = state.config.whitelist[0]?.path ?? null;
        }
        state.config = await invoke<AppConfig>("update_config", { newConfig: state.config });
        await refreshAllState();
      }
    });
  });

  // Settings: Cloud model two-level picker update
  const cloudProvEl = document.getElementById("cfg-cloud-provider") as HTMLSelectElement | null;
  const cloudTierEl = document.getElementById("cfg-cloud-tier") as HTMLSelectElement | null;
  const cloudModelInput = document.getElementById("cfg-cloud-model") as HTMLInputElement | null;

  const updateResolvedCloudModel = () => {
    if (!cloudProvEl || !cloudTierEl || !cloudModelInput) return;
    const provider = cloudProvEl.value;
    const tier = cloudTierEl.value;

    if (provider === "primary") {
      if (tier === "low") cloudModelInput.value = "lumo-lite";
      else if (tier === "high") cloudModelInput.value = "lumo-max";
      else cloudModelInput.value = "lumo";
    } else {
      const found = state.config?.alt_provider?.find((p) => p.name === provider);
      if (found) {
        const isGemini = /gemini/i.test(found.name) || /generativelanguage/i.test(found.base_url);
        if (isGemini) {
          cloudModelInput.value = tier === "high" ? "gemini-2.5-pro" : "gemini-2.5-flash";
        } else {
          cloudModelInput.value = found.model;
        }
      }
    }
  };

  cloudProvEl?.addEventListener("change", updateResolvedCloudModel);
  cloudTierEl?.addEventListener("change", updateResolvedCloudModel);

  // Settings: Failover cascade toggles
  document.getElementById("cfg-cascade-enabled")?.addEventListener("change", (e) => {
    if (!state.config) return;
    const enabled = (e.target as HTMLInputElement).checked;
    if (!state.config.cascade) {
      state.config.cascade = { enabled: false, consent: false, order: [] };
    }
    if (enabled && !state.config.cascade.consent) {
      const consentBox = document.getElementById("cfg-cascade-consent") as HTMLInputElement | null;
      if (consentBox) consentBox.focus();
      state.uiBannerError = "Enabling failover cascade requires explicit consent (see Charter, Tenet 1).";
      (e.target as HTMLInputElement).checked = false;
      renderApp();
      return;
    }
    state.config.cascade.enabled = enabled;
  });

  document.getElementById("cfg-cascade-consent")?.addEventListener("change", (e) => {
    if (!state.config) return;
    const consent = (e.target as HTMLInputElement).checked;
    if (!state.config.cascade) {
      state.config.cascade = { enabled: false, consent: false, order: [] };
    }
    state.config.cascade.consent = consent;
    if (!consent && state.config.cascade.enabled) {
      state.config.cascade.enabled = false;
      const enableBox = document.getElementById("cfg-cascade-enabled") as HTMLInputElement | null;
      if (enableBox) enableBox.checked = false;
    }
  });

  // Cascade Move Up / Move Down
  document.querySelectorAll<HTMLButtonElement>(".btn-cascade-up").forEach((btn) => {
    btn.addEventListener("click", () => {
      if (!state.config) return;
      const idx = Number(btn.getAttribute("data-idx"));
      if (Number.isNaN(idx) || idx <= 0) return;
      if (!state.config.cascade) {
        state.config.cascade = {
          enabled: false,
          consent: false,
          order: ["primary", ...(state.config.alt_provider?.map((p) => p.name) ?? [])],
        };
      }
      if (!state.config.cascade.order || state.config.cascade.order.length === 0) {
        state.config.cascade.order = ["primary", ...(state.config.alt_provider?.map((p) => p.name) ?? [])];
      }
      const order = state.config.cascade.order;
      const tmp = order[idx - 1];
      order[idx - 1] = order[idx];
      order[idx] = tmp;
      renderApp();
    });
  });

  document.querySelectorAll<HTMLButtonElement>(".btn-cascade-down").forEach((btn) => {
    btn.addEventListener("click", () => {
      if (!state.config) return;
      const idx = Number(btn.getAttribute("data-idx"));
      if (!state.config.cascade) {
        state.config.cascade = {
          enabled: false,
          consent: false,
          order: ["primary", ...(state.config.alt_provider?.map((p) => p.name) ?? [])],
        };
      }
      if (!state.config.cascade.order || state.config.cascade.order.length === 0) {
        state.config.cascade.order = ["primary", ...(state.config.alt_provider?.map((p) => p.name) ?? [])];
      }
      const order = state.config.cascade.order;
      if (Number.isNaN(idx) || idx >= order.length - 1) return;
      const tmp = order[idx + 1];
      order[idx + 1] = order[idx];
      order[idx] = tmp;
      renderApp();
    });
  });

  // Settings: Save general settings
  document.getElementById("save-settings-btn")?.addEventListener("click", async () => {
    if (!state.config) return;
    const cloudModel = (document.getElementById("cfg-cloud-model") as HTMLInputElement)?.value.trim();
    const cloudTier = (document.getElementById("cfg-cloud-tier") as HTMLSelectElement)?.value;
    const lumoUrl = (document.getElementById("cfg-lumo-url") as HTMLInputElement)?.value.trim();
    const ollamaUrl = (document.getElementById("cfg-ollama-url") as HTMLInputElement)?.value.trim();
    const fastModel = (document.getElementById("cfg-fast-model") as HTMLInputElement)?.value.trim();
    const coderModel = (document.getElementById("cfg-coder-model") as HTMLInputElement)?.value.trim();
    const heavyModel = (document.getElementById("cfg-heavy-model") as HTMLInputElement)?.value.trim();
    const timeoutVal = Number((document.getElementById("cfg-timeout") as HTMLInputElement)?.value);
    const wikiDir = (document.getElementById("cfg-wiki-dir") as HTMLInputElement)?.value.trim();
    const dryRun = (document.getElementById("cfg-dry-run") as HTMLInputElement)?.checked ?? false;
    const memoryAutoSanitize =
      (document.getElementById("cfg-memory-auto-sanitize") as HTMLInputElement)?.checked ?? true;
    const runLogLevel = (document.getElementById("cfg-run-log-level") as HTMLSelectElement)?.value;
    const runLogMaxMb = Number((document.getElementById("cfg-run-log-max-mb") as HTMLInputElement)?.value);
    const runLogKeepRuns = Number((document.getElementById("cfg-run-log-keep-runs") as HTMLInputElement)?.value);
    const cascadeEnabled = (document.getElementById("cfg-cascade-enabled") as HTMLInputElement)?.checked ?? false;
    const cascadeConsent = (document.getElementById("cfg-cascade-consent") as HTMLInputElement)?.checked ?? false;

    state.config.cloud_model = cloudModel || "lumo";
    state.config.tier = cloudTier || "medium";
    state.config.lumo_url = lumoUrl || "http://127.0.0.1:3003/v1";
    state.config.ollama_url = ollamaUrl || "http://127.0.0.1:11434";
    state.config.fast_model = fastModel || "llama3.2:3b";
    state.config.coder_model = coderModel || "qwen2.5-coder:7b";
    state.config.heavy_model = heavyModel || "qwen2.5-coder:14b";
    state.config.command_timeout_secs = timeoutVal > 0 ? timeoutVal : 60;
    state.config.wiki_dir = wikiDir || "~/Nextcloud/ProjectsWiki";
    state.config.dry_run = dryRun;
    state.config.memory_auto_sanitize = memoryAutoSanitize;
    state.config.run_log_level = runLogLevel || "info";
    state.config.run_log_max_mb = runLogMaxMb > 0 ? runLogMaxMb : 50;
    state.config.run_log_keep_runs = runLogKeepRuns > 0 ? runLogKeepRuns : 20;

    if (!state.config.cascade) {
      state.config.cascade = {
        enabled: false,
        consent: false,
        order: ["primary", ...(state.config.alt_provider?.map((p) => p.name) ?? [])],
      };
    }
    state.config.cascade.enabled = cascadeEnabled && cascadeConsent;
    state.config.cascade.consent = cascadeConsent;

    try {
      state.config = await invoke<AppConfig>("update_config", { newConfig: state.config });
      state.uiBannerInfo = "Settings saved to ~/.config/kodiak/config.toml.";
      await refreshAllState();
    } catch (err) {
      state.uiBannerError = String(err);
      renderApp();
    }
  });

  // Settings: Preset dropdown & Tier change in Add Provider form
  const presetEl = document.getElementById("new-alt-preset") as HTMLSelectElement | null;
  const newAltNameEl = document.getElementById("new-alt-name") as HTMLInputElement | null;
  const newAltUrlEl = document.getElementById("new-alt-url") as HTMLInputElement | null;
  const newAltKeyEl = document.getElementById("new-alt-key") as HTMLInputElement | null;
  const newAltTierEl = document.getElementById("new-alt-tier") as HTMLSelectElement | null;
  const newAltModelEl = document.getElementById("new-alt-model") as HTMLInputElement | null;

  presetEl?.addEventListener("change", () => {
    const val = presetEl.value;
    if (val === "lumo") {
      if (newAltNameEl) newAltNameEl.value = "Lumo-Alt";
      if (newAltUrlEl) newAltUrlEl.value = "http://127.0.0.1:3003/v1";
      if (newAltKeyEl) newAltKeyEl.value = "lumo";
      if (newAltTierEl) newAltTierEl.value = "medium";
      if (newAltModelEl) newAltModelEl.value = "lumo";
    } else if (val === "gemini") {
      if (newAltNameEl) newAltNameEl.value = "Gemini";
      if (newAltUrlEl) newAltUrlEl.value = "https://generativelanguage.googleapis.com/v1beta/openai/";
      if (newAltKeyEl && !newAltKeyEl.value) newAltKeyEl.placeholder = "Google AI Studio API key";
      if (newAltTierEl) newAltTierEl.value = "low";
      if (newAltModelEl) newAltModelEl.value = "gemini-2.5-flash";
    } else if (val === "groq") {
      if (newAltNameEl) newAltNameEl.value = "Groq";
      if (newAltUrlEl) newAltUrlEl.value = "https://api.groq.com/openai/v1";
      if (newAltKeyEl && !newAltKeyEl.value) newAltKeyEl.placeholder = "Groq API key";
      if (newAltTierEl) newAltTierEl.value = "high";
      if (newAltModelEl) newAltModelEl.value = "llama-3.3-70b-versatile";
    } else if (val === "openrouter") {
      if (newAltNameEl) newAltNameEl.value = "OpenRouter";
      if (newAltUrlEl) newAltUrlEl.value = "https://openrouter.ai/api/v1";
      if (newAltKeyEl && !newAltKeyEl.value) newAltKeyEl.placeholder = "OpenRouter API key";
      if (newAltTierEl) newAltTierEl.value = "high";
      if (newAltModelEl) newAltModelEl.value = "anthropic/claude-3.5-sonnet";
    } else if (val === "ollama") {
      if (newAltNameEl) newAltNameEl.value = "Self-Hosted-Ollama";
      if (newAltUrlEl) newAltUrlEl.value = "http://127.0.0.1:11434/v1";
      if (newAltKeyEl) newAltKeyEl.value = "ollama";
      if (newAltTierEl) newAltTierEl.value = "medium";
      if (newAltModelEl) newAltModelEl.value = "qwen2.5-coder:7b";
    }
  });

  newAltTierEl?.addEventListener("change", () => {
    const preset = presetEl?.value;
    const tier = newAltTierEl.value;
    if (preset === "gemini" && newAltModelEl) {
      newAltModelEl.value = tier === "high" ? "gemini-2.5-pro" : "gemini-2.5-flash";
    } else if (preset === "lumo" && newAltModelEl) {
      if (tier === "low") newAltModelEl.value = "lumo-lite";
      else if (tier === "high") newAltModelEl.value = "lumo-max";
      else newAltModelEl.value = "lumo";
    }
  });

  // Settings: Add alternate provider
  document.getElementById("add-alt-provider-btn")?.addEventListener("click", async () => {
    if (!state.config) return;
    const nameEl = document.getElementById("new-alt-name") as HTMLInputElement | null;
    const urlEl = document.getElementById("new-alt-url") as HTMLInputElement | null;
    const keyEl = document.getElementById("new-alt-key") as HTMLInputElement | null;
    const tierEl = document.getElementById("new-alt-tier") as HTMLSelectElement | null;
    const modelEl = document.getElementById("new-alt-model") as HTMLInputElement | null;
    const maxCharsEl = document.getElementById("new-alt-max-chars") as HTMLInputElement | null;

    const name = nameEl?.value.trim() ?? "";
    const base_url = urlEl?.value.trim() ?? "";
    const api_key = keyEl?.value.trim() ?? "";
    const tier = tierEl?.value || "medium";
    const model = modelEl?.value.trim() ?? "";
    const max_chars_raw = maxCharsEl?.value.trim() ?? "";
    const max_context_chars = max_chars_raw ? Number(max_chars_raw) : undefined;

    if (!name || !base_url || !api_key || !model) {
      state.uiBannerError = "Name, Base URL, API Key, and Model are required for an alternate provider.";
      renderApp();
      return;
    }

    if (!state.config.alt_provider) {
      state.config.alt_provider = [];
    }
    state.config.alt_provider.push({
      name,
      base_url,
      api_key,
      tier,
      model,
      max_context_chars,
    });

    if (state.config.cascade?.order && !state.config.cascade.order.includes(name)) {
      state.config.cascade.order.push(name);
    }

    try {
      state.config = await invoke<AppConfig>("update_config", { newConfig: state.config });
      state.uiBannerInfo = `Added alternate provider '${name}'.`;
      await refreshAllState();
    } catch (err) {
      state.uiBannerError = String(err);
      renderApp();
    }
  });

  // Settings: Remove alternate provider
  document.querySelectorAll<HTMLElement>(".remove-alt-provider-btn").forEach((btn) => {
    btn.addEventListener("click", async () => {
      if (!state.config || !state.config.alt_provider) return;
      const idx = Number(btn.getAttribute("data-idx"));
      if (!Number.isNaN(idx) && state.config.alt_provider[idx]) {
        const removed = state.config.alt_provider.splice(idx, 1)[0];
        if (state.config.cascade?.order) {
          state.config.cascade.order = state.config.cascade.order.filter((n) => n !== removed.name);
        }
        try {
          state.config = await invoke<AppConfig>("update_config", { newConfig: state.config });
          state.uiBannerInfo = `Removed alternate provider '${removed.name}'.`;
          await refreshAllState();
        } catch (err) {
          state.uiBannerError = String(err);
          renderApp();
        }
      }
    });
  });

  // Settings: Test alternate provider connection
  document.querySelectorAll<HTMLButtonElement>(".test-alt-provider-btn").forEach((btn) => {
    btn.addEventListener("click", async () => {
      if (!state.config || !state.config.alt_provider) return;
      const idx = Number(btn.getAttribute("data-idx"));
      if (Number.isNaN(idx) || !state.config.alt_provider[idx]) return;
      const provider = state.config.alt_provider[idx];
      const resultEl = document.getElementById(`test-provider-result-${idx}`);
      if (resultEl) {
        resultEl.innerHTML = `<span class="badge badge--neutral">Testing connection...</span>`;
      }
      btn.disabled = true;
      try {
        const res = await invoke<{
          success: boolean;
          status_code: number;
          latency_ms: number;
          error_message?: string;
        }>("test_alt_provider_connection", { provider });
        if (resultEl) {
          if (res.success) {
            resultEl.innerHTML = `<span class="badge badge--ok">HTTP ${res.status_code} (${res.latency_ms}ms): OK</span>`;
          } else {
            const err = res.error_message ? `: ${escapeHtml(res.error_message)}` : "";
            resultEl.innerHTML = `<span class="badge badge--bad">HTTP ${res.status_code} (${res.latency_ms}ms)${err}</span>`;
          }
        }
      } catch (err) {
        if (resultEl) {
          resultEl.innerHTML = `<span class="badge badge--bad">Error: ${escapeHtml(String(err))}</span>`;
        }
      } finally {
        btn.disabled = false;
      }
    });
  });

  // Diagnostic Run Log viewer handlers
  const handleOpenRunLog = async (taskId: string) => {
    try {
      const res = await invoke<TaskRunLogResponse>("get_task_run_log", { taskId });
      state.activeRunLog = res;
      renderApp();
    } catch (err) {
      state.uiBannerError = `Failed to load run log: ${err}`;
      renderApp();
    }
  };

  document.querySelectorAll<HTMLElement>(".btn-view-run-log").forEach((btn) => {
    btn.addEventListener("click", async (e) => {
      e.stopPropagation();
      const taskId = btn.getAttribute("data-task-id");
      if (taskId) await handleOpenRunLog(taskId);
    });
  });

  document.getElementById("view-current-run-log-btn")?.addEventListener("click", async () => {
    const taskId = state.currentTask?.task_id;
    if (taskId) await handleOpenRunLog(taskId);
  });

  document.getElementById("close-run-log-modal-btn")?.addEventListener("click", () => {
    state.activeRunLog = null;
    renderApp();
  });

  document.getElementById("run-log-modal-overlay")?.addEventListener("click", (e) => {
    if ((e.target as HTMLElement)?.id === "run-log-modal-overlay") {
      state.activeRunLog = null;
      renderApp();
    }
  });

  document.getElementById("copy-run-log-btn")?.addEventListener("click", async () => {
    if (state.activeRunLog?.content) {
      await copyToClipboard(state.activeRunLog.content, "Run Log");
    }
  });

  // Charter link chips: switch to About tab and scroll to tenet
  document.querySelectorAll<HTMLElement>(".charter-link-chip").forEach((btn) => {
    btn.addEventListener("click", (e) => {
      e.stopPropagation();
      const tenetTarget = btn.getAttribute("data-tenet-target");
      state.tab = "about";
      renderApp();
      if (tenetTarget) {
        setTimeout(() => {
          const el = document.getElementById(`charter-${tenetTarget}`);
          if (el) {
            el.scrollIntoView({ behavior: "smooth", block: "center" });
            el.classList.add("highlight-target");
            setTimeout(() => el.classList.remove("highlight-target"), 2000);
          }
        }, 50);
      }
    });
  });

  // Choice Card: Defer task
  document.querySelectorAll<HTMLElement>(".btn-choice-defer").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const taskId = btn.getAttribute("data-task-id");
      if (!taskId) return;
      try {
        const updated = await invoke<TaskRecord>("defer_task", { taskId });
        state.currentTask = updated;
        state.uiBannerInfo = "Task deferred: waiting for cloud availability.";
        await refreshAllState();
      } catch (err) {
        state.uiBannerError = String(err);
        renderApp();
      }
    });
  });

  // Choice Card: Continue with local model
  document.querySelectorAll<HTMLElement>(".btn-choice-local").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const taskId = btn.getAttribute("data-task-id");
      if (!taskId) return;
      state.runningPipeline = true;
      state.uiBannerError = null;
      renderApp();
      try {
        const updated = await invoke<TaskRecord>("resolve_choice_local", { taskId });
        state.currentTask = updated;
        await refreshAllState();
      } catch (err) {
        state.uiBannerError = String(err);
      } finally {
        state.runningPipeline = false;
        renderApp();
      }
    });
  });

  // Choice Card: Consent checkbox sync
  document.getElementById("choice-alt-consent-check")?.addEventListener("change", (e) => {
    state.sessionAltConsent = (e.target as HTMLInputElement).checked;
  });

  // Choice Card: Use alternate provider
  document.querySelectorAll<HTMLElement>(".btn-choice-alt").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const taskId = btn.getAttribute("data-task-id");
      if (!taskId) return;
      const sel = document.getElementById("choice-alt-provider-select") as HTMLSelectElement | null;
      const providerName = sel?.value;
      if (!providerName) {
        state.uiBannerError = "No alternate provider selected.";
        renderApp();
        return;
      }
      const chk = document.getElementById("choice-alt-consent-check") as HTMLInputElement | null;
      const consent = state.sessionAltConsent || (chk?.checked ?? false);
      if (!consent) {
        state.uiBannerError = "Explicit consent is required to route task data to an alternate provider.";
        renderApp();
        return;
      }
      state.sessionAltConsent = true;
      state.runningPipeline = true;
      state.uiBannerError = null;
      renderApp();
      try {
        const updated = await invoke<TaskRecord>("resolve_choice_alt_provider", {
          taskId,
          providerName,
          consentGiven: true,
        });
        state.currentTask = updated;
        await refreshAllState();
      } catch (err) {
        state.uiBannerError = String(err);
      } finally {
        state.runningPipeline = false;
        renderApp();
      }
    });
  });

  // Resume deferred task (from sidebar or banner)
  document.querySelectorAll<HTMLElement>(".btn-resume-deferred").forEach((btn) => {
    btn.addEventListener("click", async (ev) => {
      ev.stopPropagation();
      const taskId = btn.getAttribute("data-task-id");
      if (!taskId) return;
      state.runningPipeline = true;
      state.uiBannerError = null;
      renderApp();
      try {
        const updated = await invoke<TaskRecord>("resume_deferred_task", { taskId });
        state.currentTask = updated;
        state.readOnlyHistorical = false;
        state.tab = "workbench";
        await refreshAllState();
      } catch (err) {
        state.uiBannerError = `Cannot resume task: ${err}`;
      } finally {
        state.runningPipeline = false;
        renderApp();
      }
    });
  });

  // Toast actions
  document.getElementById("toast-review-btn")?.addEventListener("click", () => {
    state.cloudToast = null;
    state.tab = "workbench";
    const def = state.history.find((t) => t.deferred);
    if (def) {
      state.currentTask = def;
      state.readOnlyHistorical = true;
    }
    renderApp();
  });
  document.getElementById("toast-later-btn")?.addEventListener("click", () => {
    state.cloudToast = null;
    renderApp();
  });
}

async function checkCloudStatusToast(force = false): Promise<void> {
  try {
    const res = await invoke<{ available: boolean; deferred_count: number; detail: string; throttled: boolean }>(
      "probe_cloud_status",
      { force }
    );
    if (res.available && res.deferred_count > 0) {
      state.cloudToast = { show: true, count: res.deferred_count };
      renderApp();
    }
  } catch (err) {
    console.debug("Cloud probe check error:", err);
  }
}

listen<PipelineEvent>("kodiak://pipeline-event", (e) => handlePipelineEvent(e.payload));

refreshAllState();
