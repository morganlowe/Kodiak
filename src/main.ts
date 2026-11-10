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

interface QueueItem {
  queue_id: string;
  task_id: string | null;
  prompt: string;
  project_root: string;
  wait_for_previous_approval: boolean;
  status: "queued" | "interpreting" | "waiting_for_dependency" | "local_working" | "verifying" | "waiting_approval" | "completed" | "failed" | "cancelled";
  created_at: string;
  started_at: string | null;
  completed_at: string | null;
  pending_cards_count: number;
  approved_cards_count: number;
  denied_cards_count: number;
  error: string | null;
}

interface QueueEvent {
  kind: "queue_updated" | "item_status_changed";
  items?: QueueItem[];
  queue_id?: string;
  task_id?: string | null;
  status?: string;
  error?: string | null;
}

interface AltProviderConfig {
  name: string;
  base_url: string;
  api_key: string;
  model: string;
  max_context_chars?: number;
  tier?: string;
}

interface ProviderPresetModel {
  id: string;
  label: string;
  tier?: "low" | "medium" | "high";
}

const PRESET_MODELS: Record<string, ProviderPresetModel[]> = {
  gemini: [
    { id: "gemini-3.0-flash", label: "gemini-3.0-flash (Recommended Fast / 1M ctx)", tier: "low" },
    { id: "gemini-2.5-pro", label: "gemini-2.5-pro (Flagship Reasoning / 2M ctx)", tier: "high" },
    { id: "gemini-1.5-pro", label: "gemini-1.5-pro (Production Reasoning / 2M ctx)", tier: "high" },
    { id: "gemini-2.0-flash", label: "gemini-2.0-flash (Next-Gen Fast)", tier: "medium" },
    { id: "gemini-1.5-flash", label: "gemini-1.5-flash (Standard Fast)", tier: "low" },
    { id: "gemini-2.0-flash-lite", label: "gemini-2.0-flash-lite (Cost-Efficient)", tier: "low" },
  ],
  lumo: [
    { id: "lumo", label: "lumo (Standard / Medium)", tier: "medium" },
    { id: "lumo-lite", label: "lumo-lite (Fast / Low)", tier: "low" },
    { id: "lumo-max", label: "lumo-max (Premium / High)", tier: "high" },
  ],
  groq: [
    { id: "llama-3.3-70b-versatile", label: "llama-3.3-70b-versatile (Recommended)", tier: "high" },
    { id: "llama-3.1-8b-instant", label: "llama-3.1-8b-instant (Fast)", tier: "low" },
    { id: "mixtral-8x7b-32768", label: "mixtral-8x7b-32768", tier: "medium" },
  ],
  openrouter: [
    { id: "anthropic/claude-3.5-sonnet", label: "anthropic/claude-3.5-sonnet (Recommended)", tier: "high" },
    { id: "openai/gpt-4o", label: "openai/gpt-4o", tier: "high" },
    { id: "google/gemini-2.0-flash-001", label: "google/gemini-2.0-flash-001", tier: "low" },
    { id: "meta-llama/llama-3.3-70b-instruct", label: "meta-llama/llama-3.3-70b-instruct", tier: "medium" },
  ],
  ollama: [
    { id: "qwen2.5-coder:7b", label: "qwen2.5-coder:7b (Recommended Coder)", tier: "medium" },
    { id: "llama3.2:3b", label: "llama3.2:3b (Fast / Lightweight)", tier: "low" },
    { id: "qwen2.5-coder:14b", label: "qwen2.5-coder:14b (Heavy Offline Coder)", tier: "high" },
    { id: "deepseek-r1:7b", label: "deepseek-r1:7b (Local Reasoning)", tier: "medium" },
  ],
};

function getPresetForProvider(p: AltProviderConfig): string {
  if (/generativelanguage/i.test(p.base_url) || /gemini/i.test(p.name)) return "gemini";
  if (/:3003/i.test(p.base_url) || /lumo/i.test(p.name)) return "lumo";
  if (/groq/i.test(p.base_url) || /groq/i.test(p.name)) return "groq";
  if (/openrouter/i.test(p.base_url) || /openrouter/i.test(p.name)) return "openrouter";
  if (/:11434/i.test(p.base_url) || /ollama/i.test(p.name)) return "ollama";
  return "";
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
  wiki_enabled?: boolean;
  wiki_configured?: boolean;
  projects_dir?: string;
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

interface GpuInfo {
  name: string;
  vram_total_mb: number;
  vram_free_mb: number;
  is_dedicated: boolean;
}

interface RecommendedModels {
  fast_model: string;
  coder_model: string;
  heavy_model: string;
  concurrency_mode: string;
  unload_after_run: boolean;
  rationale: string;
}

interface HardwareProfile {
  system_ram_total_mb: number;
  system_ram_available_mb: number;
  gpu: GpuInfo | null;
  hardware_tier: "Tier1Entry" | "Tier2Mid" | "Tier3Performance" | "Tier4Workstation";
  tier_label: string;
  tier_description: string;
  recommended_models: RecommendedModels;
}

interface WikiStatusDetails {
  reachable: boolean;
  url: string | null;
  api_reachable: boolean;
  wiki_dir: string;
  projects_dir: string;
  doc_count: number;
  catalog_count: number;
  categories: string[];
}

interface WikiProjectItem {
  name: string;
  title: string;
  file: string;
  url: string;
  category: string;
  summary: string;
  tags: string[];
}

interface WikiSetupInfo {
  wiki_enabled: boolean;
  wiki_configured: boolean;
  projects_dir: string;
  wiki_dir: string;
  detected_projects: string[];
  hosts_configured: boolean;
  port80_configured: boolean;
  service_active: boolean;
  service_enabled: boolean;
}

interface WikiSetupPayload {
  enable_wiki: boolean;
  projects_dir: string;
  add_to_whitelist: boolean;
  enable_service: boolean;
  start_now: boolean;
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

interface BatchCardTarget {
  task_id: string;
  card_id: string;
}

interface TestHookResult {
  project_dir: string;
  command: string;
  success: boolean;
  exit_code: number | null;
  raw_output: string;
  digested_summary: string | null;
}

interface BatchApprovalResult {
  approved_count: number;
  rejected_count: number;
  affected_projects: string[];
  test_hook_results: TestHookResult[];
}

interface UnifiedInboxItem {
  task_id: string;
  task_title: string;
  project_dir: string;
  card: ActionCard;
  line_ranges: [number, number][];
  has_conflict: boolean;
  conflicting_tasks: string[];
  conflict_description: string | null;
}

interface UnifiedInboxResponse {
  items: UnifiedInboxItem[];
  total_pending: number;
  conflict_count: number;
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

interface AltProviderFormDraft {
  preset: string;
  name: string;
  base_url: string;
  api_key: string;
  tier: string;
  model: string;
  max_context_chars?: number;
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
  stoppingWiki: boolean;
  syncingWiki: boolean;
  rebuildingWikiAssets: boolean;
  buildingWikiAi: boolean;
  wikiDetails: WikiStatusDetails | null;
  wikiProjects: WikiProjectItem[];
  wikiAiModalOpen: boolean;
  wikiAiSelectedProject: string;
  wikiAiSelectedModel: string;
  wikiAiOverwrite: boolean;
  wikiAiStatusMessage: string | null;
  wikiAiOutput: string | null;
  wikiWizardOpen: boolean;
  wikiWizardStep: 1 | 2 | 3 | 4;
  wikiWizardInfo: WikiSetupInfo | null;
  wikiWizardProjectsDir: string;
  wikiWizardAddToWhitelist: boolean;
  wikiWizardEnableService: boolean;
  wikiWizardStartNow: boolean;
  wikiWizardScanning: boolean;
  wikiWizardSaving: boolean;
  wikiWizardDetected: string[];
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
  sidebarProjectsCollapsed: boolean;
  sidebarMemoriesCollapsed: boolean;
  sidebarTasksCollapsed: boolean;
  settingsProjectsCollapsed: boolean;
  journalCollapsed: boolean;
  editingAltProviderIdx: number | null;
  altProviderFormDraft: AltProviderFormDraft | null;
  fetchingLiveModels: boolean;
  fetchModelsStatus: string | null;
  fetchedModelsForPreset: Record<string, string[]>;
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
  hardwareProfile: HardwareProfile | null;
  queueItems: QueueItem[];
  queueDrawerOpen: boolean;
  queueWaitForApproval: boolean;
  queueDrawerTab: "queue" | "inbox";
  unifiedInbox: UnifiedInboxResponse | null;
  selectedInboxCardIds: string[];
  inboxRunTestHook: boolean;
  lastBatchApprovalResult: BatchApprovalResult | null;
  batchApproving: boolean;
  inboxFilter: "all" | "conflicts" | "safe";
} = {
  tab: "workbench",
  config: null,
  hardwareProfile: null,
  queueItems: [],
  queueDrawerOpen: false,
  queueWaitForApproval: true,
  queueDrawerTab: "queue",
  unifiedInbox: null,
  selectedInboxCardIds: [],
  inboxRunTestHook: true,
  lastBatchApprovalResult: null,
  batchApproving: false,
  inboxFilter: "all",
  health: null,
  cruiseControl: false,
  promptText: "",
  runningPipeline: false,
  startingBridge: false,
  startingWiki: false,
  stoppingWiki: false,
  syncingWiki: false,
  rebuildingWikiAssets: false,
  buildingWikiAi: false,
  wikiDetails: null,
  wikiProjects: [],
  wikiAiModalOpen: false,
  wikiAiSelectedProject: "",
  wikiAiSelectedModel: "qwen2.5-coder:7b",
  wikiAiOverwrite: true,
  wikiAiStatusMessage: null,
  wikiAiOutput: null,
  wikiWizardOpen: false,
  wikiWizardStep: 1,
  wikiWizardInfo: null,
  wikiWizardProjectsDir: "~/Projects",
  wikiWizardAddToWhitelist: true,
  wikiWizardEnableService: true,
  wikiWizardStartNow: true,
  wikiWizardScanning: false,
  wikiWizardSaving: false,
  wikiWizardDetected: [],
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
  sidebarProjectsCollapsed: false,
  sidebarMemoriesCollapsed: false,
  sidebarTasksCollapsed: false,
  settingsProjectsCollapsed: true,
  journalCollapsed: true,
  editingAltProviderIdx: null,
  altProviderFormDraft: null,
  fetchingLiveModels: false,
  fetchModelsStatus: null,
  fetchedModelsForPreset: {},
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
    const [cfg, cruise, health, hist, journal, mems, hwProfile, qItems, uInbox] = await Promise.all([
      invoke<AppConfig>("get_config"),
      invoke<boolean>("get_cruise_control"),
      invoke<HealthStatus>("check_health"),
      invoke<TaskRecord[]>("get_task_history"),
      invoke<JournalEntry[]>("get_journal_entries", { limit: 50 }),
      invoke<MemoryEntry[]>("get_memories"),
      invoke<HardwareProfile>("get_hardware_profile").catch(() => null),
      invoke<QueueItem[]>("get_queue_items").catch(() => []),
      invoke<UnifiedInboxResponse>("get_unified_inbox").catch(() => null),
    ]);
    state.config = cfg;
    state.cruiseControl = cruise;
    state.health = health;
    state.history = hist;
    state.journal = journal;
    state.memories = mems;
    if (hwProfile) state.hardwareProfile = hwProfile;
    if (qItems) state.queueItems = qItems;
    if (uInbox) state.unifiedInbox = uInbox;

    try {
      const [wDetails, wProjects] = await Promise.all([
        invoke<WikiStatusDetails>("get_wiki_details"),
        invoke<WikiProjectItem[]>("list_wiki_projects"),
      ]);
      state.wikiDetails = wDetails;
      state.wikiProjects = wProjects;
      if (!state.wikiAiSelectedProject && state.config?.active_project) {
        const baseName = state.config.active_project.split("/").pop() || "";
        state.wikiAiSelectedProject = baseName;
      }
    } catch {
      // Wiki backend details probe failed or server stopped
    }
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
        ${
          cfg?.wiki_enabled !== false
            ? `<button type="button" class="nav-tab ${state.tab === "wiki" ? "is-active" : ""}" data-nav="wiki">
                <span>Projects Wiki</span>
                ${state.health?.wiki_reachable ? `<span class="nav-tab__badge" style="background:var(--wiki-accent); color:#fff;">Live</span>` : ""}
              </button>`
            : ""
        }
        <button type="button" class="nav-tab ${state.tab === "settings" ? "is-active" : ""}" data-nav="settings">
          <span>Settings</span>
          <span class="nav-tab__badge">${cfg?.whitelist.length ?? 0} dirs</span>
        </button>
        <button type="button" class="nav-tab ${state.tab === "about" ? "is-active" : ""}" data-nav="about">
          <span>About</span>
        </button>
      </nav>

      <div class="row-inline">
        <button
          type="button"
          id="queue-drawer-toggle-btn"
          class="btn btn--sm ${
            state.unifiedInbox && state.unifiedInbox.conflict_count > 0
              ? 'btn--warn'
              : state.queueItems.some((i) => i.status === 'interpreting' || i.status === 'local_working' || i.status === 'verifying')
              ? 'btn--primary'
              : ''
          }"
          title="Toggle Multi-Task Queue & Unified Approval Inbox"
        >
          <span>📋 Queue</span>
          ${
            state.queueItems.length > 0
              ? `<span class="nav-tab__badge" style="background:${
                  state.queueItems.some((i) => i.status === 'waiting_approval')
                    ? '#a855f7'
                    : state.queueItems.some((i) => i.status === 'interpreting' || i.status === 'local_working' || i.status === 'verifying')
                    ? 'var(--accent)'
                    : '#4b5563'
                }; color:#fff; font-size:0.7rem; margin-inline-start:0.3rem;">${state.queueItems.length}</span>`
              : ""
          }
          ${
            state.unifiedInbox && state.unifiedInbox.total_pending > 0
              ? `<span class="nav-tab__badge" style="background:${
                  state.unifiedInbox.conflict_count > 0 ? '#ef4444' : '#a855f7'
                }; color:#fff; font-size:0.7rem; margin-inline-start:0.3rem;" title="${state.unifiedInbox.total_pending} card(s) pending approval across all tasks${state.unifiedInbox.conflict_count > 0 ? ' (' + state.unifiedInbox.conflict_count + ' diff conflict!)' : ''}">📥 ${state.unifiedInbox.total_pending}${state.unifiedInbox.conflict_count > 0 ? ' ⚠️' : ''}</span>`
              : ""
          }
        </button>
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
        ${
          h.active_project && cfg.wiki_enabled !== false
            ? `<button type="button" class="btn btn--sm quick-wiki-btn" title="View or document active project in Projects Wiki" style="padding:0.15rem 0.5rem; font-size:0.75rem; margin-inline-start:0.35rem;">📖 Wiki</button>`
            : ""
        }
      </span>

      ${
        cfg.wiki_enabled !== false
          ? `<span class="health-item">
              <span class="${h.wiki_reachable ? "status-dot is-ok" : "status-dot is-warn"}"></span>
              <span>Projects Wiki: <strong>${h.wiki_reachable ? "Live" : "Offline"}</strong></span>
            </span>`
          : ""
      }

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
    ${
      !cfg.wiki_configured
        ? `<div class="stage-alert" style="background:var(--wiki-surface); border:1px solid var(--wiki-accent); border-radius:var(--wiki-radius-sm); padding:0.75rem 1rem; margin-block-end:1rem; display:flex; justify-content:space-between; align-items:center; gap:1rem; flex-wrap:wrap;">
            <div style="display:flex; flex-direction:column; gap:0.25rem;">
              <strong style="color:var(--wiki-accent); font-size:0.95rem;">📖 Projects Wiki & AI Documentation Hub Available</strong>
              <span class="muted" style="font-size:0.82rem;">Would you like to set up the Projects Wiki for your repositories or skip it? You can always change this in Settings.</span>
            </div>
            <div class="row-inline" style="gap:0.5rem;">
              <button type="button" id="banner-open-wizard-btn" class="btn btn--sm btn--primary">🛠 Set Up Wiki</button>
              <button type="button" id="banner-skip-wizard-btn" class="btn btn--sm">Skip for Now</button>
            </div>
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
  const renderProjectButton = (w: WhitelistEntry) => {
    const name = w.path.split("/").filter(Boolean).pop() ?? w.path;
    const isActive = cfg?.active_project === w.path;
    return `<button type="button" class="side-item ${isActive ? "is-active" : ""}" data-select-project="${escapeHtml(w.path)}" title="${escapeHtml(w.path)}">
      <span class="side-item__label" style="overflow:hidden; text-overflow:ellipsis; white-space:nowrap; flex:1;">${escapeHtml(name)}</span>
      <span class="badge badge--neutral" style="font-size:0.65rem; flex-shrink:0;">${escapeHtml(w.level)}</span>
    </button>`;
  };

  const allProjectsHtml = (cfg?.whitelist ?? []).map(renderProjectButton).join("");
  const activeEntry = (cfg?.whitelist ?? []).find((w) => w.path === cfg?.active_project);
  const collapsedProjectHtml = activeEntry
    ? renderProjectButton(activeEntry)
    : `<p class="muted" style="font-size:0.75rem; padding-inline-start:0.35rem; margin:0.2rem 0;">(No active project)</p>`;

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
          <div class="row-between" style="cursor:pointer; margin-block-end:0.25rem; user-select:none; padding-inline-end:0.2rem;" id="sidebar-projects-toggle" title="Click to collapse/expand projects">
            <div class="row-inline" style="gap:0.4rem; align-items:center;">
              <span class="mono muted" style="font-size:0.75rem; width:0.85rem; display:inline-block; text-align:center;">${state.sidebarProjectsCollapsed ? "▶" : "▼"}</span>
              <h3 class="sidebar__heading" style="margin:0;">Projects</h3>
            </div>
            <span class="badge badge--neutral" style="font-size:0.65rem;">${(cfg?.whitelist ?? []).length}</span>
          </div>
          ${state.sidebarProjectsCollapsed ? collapsedProjectHtml : (allProjectsHtml || `<p class="muted" style="font-size:0.78rem;">None whitelisted.</p>`)}
        </div>

        <div>
          <div class="row-between" style="cursor:pointer; margin-block-end:0.25rem; user-select:none; padding-inline-end:0.2rem;" id="sidebar-memories-toggle" title="Click to collapse/expand memories">
            <div class="row-inline" style="gap:0.4rem; align-items:center;">
              <span class="mono muted" style="font-size:0.75rem; width:0.85rem; display:inline-block; text-align:center;">${state.sidebarMemoriesCollapsed ? "▶" : "▼"}</span>
              <h3 class="sidebar__heading" style="margin:0;">Memories</h3>
            </div>
            <div class="row-inline" style="align-items:center; gap:0.25rem;">
              <span class="badge badge--neutral" style="font-size:0.65rem;">${visibleMemories.length}</span>
              <button type="button" class="side-icon-btn" id="refresh-memories-btn" title="Rescan memories from disk" onclick="event.stopPropagation();">↻</button>
              <button type="button" class="side-icon-btn" id="add-memory-btn" title="New Memory" onclick="event.stopPropagation();">+</button>
            </div>
          </div>
          ${state.sidebarMemoriesCollapsed ? "" : memoriesHtml}
        </div>

        ${deferredHtml}

        <div>
          <div class="row-between" style="cursor:pointer; margin-block-end:0.25rem; user-select:none; padding-inline-end:0.2rem;" id="sidebar-tasks-toggle" title="Click to collapse/expand tasks">
            <div class="row-inline" style="gap:0.4rem; align-items:center;">
              <span class="mono muted" style="font-size:0.75rem; width:0.85rem; display:inline-block; text-align:center;">${state.sidebarTasksCollapsed ? "▶" : "▼"}</span>
              <h3 class="sidebar__heading" style="margin:0;">Tasks</h3>
            </div>
            <span class="badge badge--neutral" style="font-size:0.65rem;">${nonDeferred.length}</span>
          </div>
          ${state.sidebarTasksCollapsed ? "" : tasksHtml}
        </div>
      </div>
    </aside>

    ${renderDeleteTaskModal()}
    ${renderDeleteMemoryModal()}
    ${renderMemoryViewModal()}
    ${renderMemoryEditModal()}
    ${renderWikiAiModal()}
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
  const latest = state.journal[state.journal.length - 1];
  return `
    <section class="panel-card">
      <div class="panel-header" id="journal-toggle-btn" style="cursor:pointer; user-select:none;" title="Click to collapse/expand Decision Journal">
        <div class="row-inline">
          <h2 class="panel-title">Local Decision Journal (<code>~/.local/share/kodiak/journal.jsonl</code>)</h2>
          <span class="badge badge--neutral">${state.journal.length}</span>
        </div>
        <div class="row-inline">
          <span class="muted">Append-only audit trail of human & autonomous actions.</span>
          <span class="mono muted" style="font-size:0.8rem; font-weight:600; padding:0.15rem 0.45rem; border:1px solid var(--wiki-border); border-radius:var(--wiki-radius-sm);">
            ${state.journalCollapsed ? "▶ Expand" : "▼ Collapse"}
          </span>
        </div>
      </div>
      ${
        state.journalCollapsed
          ? `<div class="row-between" style="padding:0.75rem 0.9rem; font-size:0.85rem; background:var(--wiki-surface); border-radius:var(--wiki-radius-sm); border:1px dashed var(--wiki-border);">
              <div class="row-inline" style="gap:0.55rem; align-items:center;">
                <span class="badge ${
                  latest?.decision === "denied" || latest?.decision === "rejected"
                    ? "badge--bad"
                    : latest?.decision === "accepted" || latest?.decision === "grant_auto_executed"
                    ? "badge--ok"
                    : "badge--warn"
                }">Latest</span>
                <span class="mono"><strong>${escapeHtml(latest?.decision || "None")}</strong></span>
                <span class="muted">${escapeHtml(latest?.target_path || "")} — ${escapeHtml(latest?.reason || "No actions logged yet")}</span>
              </div>
              <span class="muted">${state.journal.length} entries &bull; Click "Expand" to view audit trail</span>
            </div>`
          : state.journal.length === 0
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

function renderWikiSetupWizard(): string {
  if (!state.wikiWizardOpen) return "";

  const info = state.wikiWizardInfo;
  const currentStep = state.wikiWizardStep;

  let stepBodyHtml = "";

  switch (currentStep) {
    case 1: {
      stepBodyHtml = `
        <div style="display:flex; flex-direction:column; gap:1rem;">
          <p class="muted" style="font-size:0.88rem; line-height:1.45; margin:0;">
            Kodiak now features a unified <strong>Projects Wiki & Knowledge Hub</strong>.
            It provides fast local browsing with MkDocs Material, hardware asset indexing (PDF datasheets, schematics, manuals),
            and offline AI documentation generation powered by local Ollama (e.g. <code>qwen2.5-coder:7b</code>) with zero cloud tokens.
          </p>

          <div style="display:grid; grid-template-columns:1fr 1fr; gap:0.85rem; margin-top:0.25rem;">
            <div style="background:var(--wiki-surface); border:2px solid var(--wiki-accent); border-radius:var(--wiki-radius); padding:1rem; display:flex; flex-direction:column; gap:0.5rem;">
              <div class="row-inline" style="gap:0.4rem; align-items:center;">
                <span style="font-size:1.15rem;">🚀</span>
                <strong style="color:var(--wiki-accent);">Set Up Projects Wiki</strong>
              </div>
              <p class="muted" style="font-size:0.8rem; margin:0; flex:1;">
                Point Kodiak to your projects folder, automatically discover your repositories, sync the whitelist, and enable the full local wiki hub.
              </p>
              <button type="button" id="wizard-next-step1-btn" class="btn btn--primary" style="margin-top:0.5rem;">
                Set Up Projects Wiki →
              </button>
            </div>

            <div style="background:var(--wiki-surface); border:1px solid var(--wiki-border); border-radius:var(--wiki-radius); padding:1rem; display:flex; flex-direction:column; gap:0.5rem;">
              <div class="row-inline" style="gap:0.4rem; align-items:center;">
                <span style="font-size:1.15rem;">⏭</span>
                <strong>Skip for Now</strong>
              </div>
              <p class="muted" style="font-size:0.8rem; margin:0; flex:1;">
                Skip wiki setup and hide the "Projects Wiki" navigation tab. You can rerun this wizard or re-enable the wiki anytime in Settings.
              </p>
              <button type="button" id="wizard-skip-step1-btn" class="btn btn--danger" style="margin-top:0.5rem;">
                Skip & Hide Wiki Button
              </button>
            </div>
          </div>
        </div>
      `;
      break;
    }
    case 2: {
      const detectedCount = state.wikiWizardDetected.length;
      const detectedChips =
        detectedCount > 0
          ? state.wikiWizardDetected
              .slice(0, 16)
              .map(
                (p) =>
                  `<span class="badge badge--neutral" style="font-size:0.75rem;">📁 ${escapeHtml(p)}</span>`
              )
              .join(" ") +
            (detectedCount > 16
              ? ` <span class="muted" style="font-size:0.75rem;">+${detectedCount - 16} more...</span>`
              : "")
          : `<span class="muted" style="font-size:0.8rem;">No repositories detected yet. Click "Scan Directory" below.</span>`;

      stepBodyHtml = `
        <div style="display:flex; flex-direction:column; gap:1rem;">
          <p class="muted" style="font-size:0.85rem; margin:0;">
            Specify the folder containing your local coding repositories and projects.
            Both Kodiak's task supervisor and the Projects Wiki catalog will use this directory.
          </p>

          <div class="form-field">
            <label class="form-label" for="wizard-projects-dir-input">
              Projects Root Directory (e.g. <code>~/Projects</code> or <code>/path/to/projects</code>)
            </label>
            <div class="row-inline" style="gap:0.5rem; align-items:center;">
              <input
                type="text"
                id="wizard-projects-dir-input"
                class="form-input"
                style="flex:1;"
                value="${escapeHtml(state.wikiWizardProjectsDir)}"
                placeholder="~/Projects"
              />
              <button
                type="button"
                id="wizard-scan-dir-btn"
                class="btn btn--accent"
                ${state.wikiWizardScanning ? "disabled" : ""}
              >
                ${state.wikiWizardScanning ? "Scanning..." : "🔍 Scan Directory"}
              </button>
            </div>
          </div>

          <div style="background:var(--wiki-surface); border:1px solid var(--wiki-border); border-radius:var(--wiki-radius-sm); padding:0.75rem; display:flex; flex-direction:column; gap:0.5rem;">
            <div class="row-between" style="align-items:center;">
              <strong style="font-size:0.82rem;">Detected Project Repositories:</strong>
              <span class="badge ${detectedCount > 0 ? "badge--good" : "badge--neutral"}">
                ${detectedCount} found
              </span>
            </div>
            <div style="display:flex; flex-wrap:wrap; gap:0.35rem; max-height:120px; overflow-y:auto; padding:0.25rem 0;">
              ${detectedChips}
            </div>
          </div>

          <div style="display:flex; align-items:flex-start; gap:0.6rem; padding:0.5rem 0.25rem;">
            <input
              type="checkbox"
              id="wizard-add-to-whitelist-cb"
              style="margin-top:0.25rem;"
              ${state.wikiWizardAddToWhitelist ? "checked" : ""}
            />
            <div>
              <label for="wizard-add-to-whitelist-cb" style="font-weight:600; font-size:0.85rem; cursor:pointer;">
                Sync with Kodiak Whitelist (Recommended)
              </label>
              <div class="muted" style="font-size:0.78rem;">
                Automatically register detected projects in Kodiak's security whitelist with <code>READ</code> grant
                so they are immediately available in Kodiak's project picker.
              </div>
            </div>
          </div>

          <div class="row-between" style="margin-top:0.5rem;">
            <button type="button" id="wizard-back-step2-btn" class="btn">← Back</button>
            <button type="button" id="wizard-next-step2-btn" class="btn btn--primary">Next: Domain & Service →</button>
          </div>
        </div>
      `;
      break;
    }
    case 3: {
      const isHostsConfigured = info?.hosts_configured ?? false;
      const port80Configured = info?.port80_configured ?? false;

      stepBodyHtml = `
        <div style="display:flex; flex-direction:column; gap:1rem;">
          <p class="muted" style="font-size:0.85rem; margin:0;">
            Configure your local network domain and background daemon options for the Projects Wiki.
          </p>

          <div style="background:var(--wiki-surface); border:1px solid var(--wiki-border); border-radius:var(--wiki-radius); padding:0.85rem; display:flex; flex-direction:column; gap:0.6rem;">
            <div class="row-between" style="align-items:center;">
              <strong style="font-size:0.9rem;">Local Domain Resolution (<code>wiki.local</code>)</strong>
              ${
                isHostsConfigured
                  ? `<span class="badge badge--good">✔ Configured</span>`
                  : `<span class="badge badge--neutral">Fallback: http://127.0.0.1:8000</span>`
              }
            </div>
            ${
              isHostsConfigured
                ? `<p class="muted" style="font-size:0.8rem; margin:0;">
                    Your <code>/etc/hosts</code> contains an entry for <code>127.0.0.1 wiki.local</code>.
                    ${port80Configured ? "Binding to port 80 allows clean URL <code>http://wiki.local</code>." : "MkDocs will serve at <code>http://wiki.local:8000</code> or <code>http://127.0.0.1:8000</code>."}
                  </p>`
                : `<div style="font-size:0.8rem; display:flex; flex-direction:column; gap:0.4rem;">
                    <span class="muted">To resolve <code>http://wiki.local</code> seamlessly in any browser without ports, add this line to your <code>/etc/hosts</code>:</span>
                    <pre class="mono" style="background:rgba(0,0,0,0.3); padding:0.4rem 0.6rem; border-radius:4px; margin:0;">127.0.0.1 wiki.local</pre>
                    <span class="muted" style="font-size:0.75rem;">No root required right now — if not present, Kodiak connects to <code>http://127.0.0.1:8000</code> automatically.</span>
                  </div>`
            }
          </div>

          <div style="display:flex; flex-direction:column; gap:0.6rem; padding:0.25rem 0;">
            <div style="display:flex; align-items:flex-start; gap:0.6rem;">
              <input
                type="checkbox"
                id="wizard-enable-service-cb"
                style="margin-top:0.25rem;"
                ${state.wikiWizardEnableService ? "checked" : ""}
              />
              <div>
                <label for="wizard-enable-service-cb" style="font-weight:600; font-size:0.85rem; cursor:pointer;">
                  Enable systemd User Service (<code>wiki.service</code>)
                </label>
                <div class="muted" style="font-size:0.78rem;">
                  Automatically runs the MkDocs server and Sync API in the background at user login.
                </div>
              </div>
            </div>

            <div style="display:flex; align-items:flex-start; gap:0.6rem;">
              <input
                type="checkbox"
                id="wizard-start-now-cb"
                style="margin-top:0.25rem;"
                ${state.wikiWizardStartNow ? "checked" : ""}
              />
              <div>
                <label for="wizard-start-now-cb" style="font-weight:600; font-size:0.85rem; cursor:pointer;">
                  Start Projects Wiki Server Immediately
                </label>
                <div class="muted" style="font-size:0.78rem;">
                  Launches MkDocs Material server and the Sync API daemon immediately upon finishing setup.
                </div>
              </div>
            </div>
          </div>

          <div class="row-between" style="margin-top:0.5rem;">
            <button type="button" id="wizard-back-step3-btn" class="btn">← Back</button>
            <button type="button" id="wizard-next-step3-btn" class="btn btn--primary">Next: Review & Finish →</button>
          </div>
        </div>
      `;
      break;
    }
    case 4: {
      const detectedCount = state.wikiWizardDetected.length;
      stepBodyHtml = `
        <div style="display:flex; flex-direction:column; gap:1rem;">
          <p class="muted" style="font-size:0.85rem; margin:0;">
            Review your configuration below. Clicking <strong>Complete Setup</strong> will save your settings,
            sync your catalog, and open the Projects Wiki tab in Kodiak.
          </p>

          <div style="background:var(--wiki-surface); border:1px solid var(--wiki-border); border-radius:var(--wiki-radius); padding:0.85rem; display:flex; flex-direction:column; gap:0.5rem; font-size:0.82rem;">
            <div class="row-between">
              <span class="muted">Projects Directory:</span>
              <strong class="mono">${escapeHtml(state.wikiWizardProjectsDir)}</strong>
            </div>
            <div class="row-between">
              <span class="muted">Detected Repositories:</span>
              <span><strong>${detectedCount}</strong> project(s)</span>
            </div>
            <div class="row-between">
              <span class="muted">Kodiak Whitelist Sync:</span>
              <span>${state.wikiWizardAddToWhitelist ? "✔ Add all detected projects (READ)" : "✕ Do not modify whitelist"}</span>
            </div>
            <div class="row-between">
              <span class="muted">Access URL:</span>
              <span class="mono">${info?.hosts_configured ? "http://wiki.local" : "http://127.0.0.1:8000"}</span>
            </div>
            <div class="row-between">
              <span class="muted">systemd Service Autostart:</span>
              <span>${state.wikiWizardEnableService ? "✔ Enabled" : "✕ Disabled"}</span>
            </div>
            <div class="row-between">
              <span class="muted">Start Server Now:</span>
              <span>${state.wikiWizardStartNow ? "✔ Yes" : "✕ No"}</span>
            </div>
          </div>

          <div class="row-between" style="margin-top:0.5rem;">
            <button type="button" id="wizard-back-step4-btn" class="btn" ${state.wikiWizardSaving ? "disabled" : ""}>← Back</button>
            <button
              type="button"
              id="wizard-finish-btn"
              class="btn btn--primary"
              ${state.wikiWizardSaving ? "disabled" : ""}
            >
              ${state.wikiWizardSaving ? "Saving & Cataloging..." : "🚀 Complete Setup & Open Wiki"}
            </button>
          </div>
        </div>
      `;
      break;
    }
  }

  return `
    <div class="modal-backdrop" id="wiki-wizard-modal-backdrop">
      <div class="modal-box" role="dialog" aria-modal="true" style="max-inline-size: 640px; width: 100%;">
        <div class="row-between" style="border-block-end: 1px solid var(--wiki-border); padding-block-end: 0.6rem; margin-block-end: 0.75rem;">
          <div style="display:flex; align-items:center; gap:0.5rem;">
            <span style="font-size:1.3rem;">📖</span>
            <div>
              <strong style="font-size:1.05rem;">Projects Wiki Setup Wizard</strong>
              <div class="muted" style="font-size:0.75rem;">Step ${currentStep} of 4</div>
            </div>
          </div>
          <button type="button" class="btn btn--sm" id="wizard-close-x-btn" title="Close Wizard">✕</button>
        </div>

        <!-- Step Indicator -->
        <div style="display:flex; justify-content:space-between; margin-block-end:0.75rem; background:rgba(0,0,0,0.15); padding:0.4rem 0.6rem; border-radius:var(--wiki-radius-sm); font-size:0.75rem;">
          <span style="${currentStep === 1 ? "color:var(--wiki-accent); font-weight:700;" : "color:var(--text-muted);"}">1. Welcome</span>
          <span class="muted">›</span>
          <span style="${currentStep === 2 ? "color:var(--wiki-accent); font-weight:700;" : "color:var(--text-muted);"}">2. Directory</span>
          <span class="muted">›</span>
          <span style="${currentStep === 3 ? "color:var(--wiki-accent); font-weight:700;" : "color:var(--text-muted);"}">3. Domain & Service</span>
          <span class="muted">›</span>
          <span style="${currentStep === 4 ? "color:var(--wiki-accent); font-weight:700;" : "color:var(--text-muted);"}">4. Finish</span>
        </div>

        ${stepBodyHtml}
      </div>
    </div>
  `;
}

function renderWikiAiModal(): string {
  if (!state.wikiAiModalOpen) return "";

  const projectNames = new Set<string>();
  if (state.config?.active_project) {
    const base = state.config.active_project.split("/").pop();
    if (base) projectNames.add(base);
  }
  for (const w of state.config?.whitelist || []) {
    const base = w.path.split("/").pop();
    if (base) projectNames.add(base);
  }
  for (const p of state.wikiProjects || []) {
    if (p.name) projectNames.add(p.name);
  }
  const projectList = Array.from(projectNames).sort();

  const optionsHtml = projectList
    .map(
      (name) =>
        `<option value="${escapeHtml(name)}" ${
          state.wikiAiSelectedProject === name ? "selected" : ""
        }>${escapeHtml(name)}</option>`
    )
    .join("");

  return `
    <div class="modal-backdrop" id="wiki-ai-modal-backdrop">
      <div class="modal-box" role="dialog" aria-modal="true" style="max-inline-size: 640px; width: 100%;">
        <div class="row-between" style="border-block-end: 1px solid var(--wiki-border); padding-block-end: 0.5rem; margin-block-end: 0.75rem;">
          <strong style="font-size: 1.05rem;">🤖 Build Project Documentation with Local AI</strong>
          <button type="button" class="btn btn--sm" id="close-wiki-ai-modal-btn">✕</button>
        </div>

        <p class="muted" style="font-size: 0.85rem; margin-block-end: 1rem;">
          Kodiak utilizes your local Ollama models (such as <code>qwen2.5-coder:7b</code>) to inspect project code,
          hardware architecture, pinouts, and assets to write comprehensive Markdown documentation for the Projects Wiki.
        </p>

        <div style="display:flex; flex-direction:column; gap:0.75rem; margin-block-end: 1rem;">
          <div>
            <label class="form-label" for="wiki-ai-project-select">Target Project</label>
            <select id="wiki-ai-project-select" class="form-select" style="width: 100%;">
              ${optionsHtml || `<option value="demo-project">demo-project</option>`}
            </select>
          </div>

          <div>
            <label class="form-label" for="wiki-ai-model-select">Local AI Model</label>
            <select id="wiki-ai-model-select" class="form-select" style="width: 100%;">
              <option value="qwen2.5-coder:7b" ${state.wikiAiSelectedModel === "qwen2.5-coder:7b" ? "selected" : ""}>qwen2.5-coder:7b (Recommended - Default Local Coder)</option>
              <option value="qwen2.5-coder" ${state.wikiAiSelectedModel === "qwen2.5-coder" ? "selected" : ""}>qwen2.5-coder (Standard)</option>
              <option value="llama3.2:3b" ${state.wikiAiSelectedModel === "llama3.2:3b" ? "selected" : ""}>llama3.2:3b (Fast Summary)</option>
              <option value="qwen2.5-coder:14b" ${state.wikiAiSelectedModel === "qwen2.5-coder:14b" ? "selected" : ""}>qwen2.5-coder:14b (Heavy Synthesis)</option>
            </select>
          </div>

          <label class="row-inline" style="gap:0.5rem; cursor:pointer; font-size:0.85rem; user-select:none;">
            <input type="checkbox" id="wiki-ai-overwrite-checkbox" ${state.wikiAiOverwrite ? "checked" : ""} />
            <span>Overwrite existing doc page if it already exists in <code>docs/</code></span>
          </label>
        </div>

        ${
          state.wikiAiStatusMessage
            ? `<div style="padding:0.6rem 0.8rem; border-radius:var(--wiki-radius-sm); margin-block-end:0.75rem; font-size:0.85rem; background:var(--wiki-surface); border:1px solid var(--wiki-border);">
                ${escapeHtml(state.wikiAiStatusMessage)}
              </div>`
            : ""
        }

        ${
          state.wikiAiOutput
            ? `<pre style="max-height:160px; overflow-y:auto; padding:0.6rem; background:#11141a; border-radius:var(--wiki-radius-sm); font-family:var(--wiki-font-mono); font-size:0.75rem; white-space:pre-wrap; margin-block-end:0.75rem;">${escapeHtml(state.wikiAiOutput)}</pre>`
            : ""
        }

        <div class="row-between" style="border-block-start: 1px solid var(--wiki-border); padding-block-start: 0.75rem;">
          <button type="button" class="btn btn--sm" id="close-wiki-ai-modal-cancel-btn">Close</button>
          <div class="row-inline" style="gap:0.5rem;">
            <button
              type="button"
              class="btn btn--sm btn--primary"
              id="run-wiki-ai-build-btn"
              ${state.buildingWikiAi ? "disabled" : ""}
            >
              ${state.buildingWikiAi ? "Generating Documentation with AI..." : "🚀 Run AI Documentation Builder"}
            </button>
          </div>
        </div>
      </div>
    </div>
  `;
}

function renderWikiTab(): string {
  const h = state.health;
  const cfg = state.config;
  if (!h || !cfg) return "";

  const wd = state.wikiDetails;
  const docCountStr = wd?.doc_count ? `${wd.doc_count} Docs` : "74 Docs";
  const catalogCountStr = wd?.catalog_count ? `${wd.catalog_count} Projects` : "74 Projects";

  if (!h.wiki_reachable || !h.wiki_url) {
    return `
      <section class="panel-card" style="margin:1.25rem;">
        <div class="panel-header">
          <h2 class="panel-title">📖 Projects Wiki & AI Documentation Hub</h2>
          <span class="badge badge--bad">MkDocs & Sync API Offline</span>
        </div>
        <p class="muted">
          Kodiak includes the Projects Wiki and local AI documentation generator directly at <code>${escapeHtml(wd?.wiki_dir || cfg.wiki_dir || "./ProjectsWiki")}</code>.
          The background MkDocs Material server and Sync API (port 8765) are currently stopped.
        </p>
        <div class="row-inline" style="gap:0.75rem; margin-top:1rem; flex-wrap:wrap;">
          <button
            type="button"
            id="start-wiki-btn"
            class="btn btn--primary"
            ${state.startingWiki ? "disabled" : ""}
          >
            ${state.startingWiki ? "Starting Projects Wiki..." : "▶ Start Projects Wiki Server"}
          </button>
          <button
            type="button"
            id="sync-wiki-btn"
            class="btn"
            ${state.syncingWiki ? "disabled" : ""}
          >
            ${state.syncingWiki ? "Syncing..." : "🔄 Sync Catalog Now"}
          </button>
          <button
            type="button"
            id="open-wiki-ai-modal-btn"
            class="btn btn--accent"
          >
            🤖 Build Wiki with AI
          </button>
          <button type="button" id="recheck-wiki-btn" class="btn">Re-check Connection</button>
        </div>
      </section>
      ${renderWikiAiModal()}
    `;
  }

  return `
    <div style="display:flex; flex-direction:column; block-size:100%; inline-size:100%;">
      <div class="row-between" style="padding:0.45rem 1.25rem; background:var(--wiki-surface); border-block-end:1px solid var(--wiki-border); font-size:0.82rem; gap:0.5rem; flex-wrap:wrap;">
        <div class="row-inline" style="gap:0.6rem; align-items:center;">
          <span class="badge badge--good" title="MkDocs HTTP Live">🟢 Wiki Live</span>
          <code class="mono" style="font-size:0.8rem;">${escapeHtml(h.wiki_url)}</code>
          ${wd?.api_reachable ? `<span class="badge badge--neutral" title="Sync & Asset API on port 8765">⚡ API :8765</span>` : ""}
          <span class="badge badge--neutral" title="Cataloged Project Docs">${escapeHtml(docCountStr)} • ${escapeHtml(catalogCountStr)}</span>
        </div>
        <div class="row-inline" style="gap:0.4rem; align-items:center;">
          <button type="button" id="open-wiki-ai-modal-btn" class="btn btn--sm btn--primary" title="Generate or refresh project docs with Ollama">
            🤖 AI Build Doc
          </button>
          <button type="button" id="sync-wiki-btn" class="btn btn--sm" ${state.syncingWiki ? "disabled" : ""} title="Refresh catalog and mkdocs navigation">
            ${state.syncingWiki ? "Syncing..." : "🔄 Sync Catalog"}
          </button>
          <button type="button" id="rebuild-wiki-assets-btn" class="btn btn--sm" ${state.rebuildingWikiAssets ? "disabled" : ""} title="Index PDFs, manuals, schematics">
            ${state.rebuildingWikiAssets ? "Indexing..." : "📦 Rebuild Assets"}
          </button>
          <button type="button" id="reload-wiki-iframe-btn" class="btn btn--sm" title="Reload embedded iframe">↻ Reload</button>
          <button type="button" id="stop-wiki-btn" class="btn btn--sm btn--danger" ${state.stoppingWiki ? "disabled" : ""} title="Stop background server">⏹ Stop</button>
          <a href="${escapeHtml(h.wiki_url)}" target="_blank" rel="noopener noreferrer" class="btn btn--sm" title="Open in system browser" style="text-decoration:none;">↗ External</a>
        </div>
      </div>
      <iframe
        id="projects-wiki-iframe"
        class="wiki-iframe"
        src="${escapeHtml(h.wiki_url)}"
        title="Projects Wiki"
      ></iframe>
    </div>
    ${renderWikiAiModal()}
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

function renderHardwareCard(hw: HardwareProfile | null): string {
  if (!hw) {
    return `
      <div class="panel-card" style="grid-column: 1 / -1; margin-block-end: 0.75rem; padding: 0.75rem 1rem;">
        <span class="muted" style="font-size: 0.85rem;">Hardware evaluation in progress...</span>
      </div>
    `;
  }

  const tierBadgeClass =
    hw.hardware_tier === "Tier4Workstation"
      ? "badge--accent"
      : hw.hardware_tier === "Tier3Performance"
      ? "badge--ok"
      : hw.hardware_tier === "Tier2Mid"
      ? "badge--neutral"
      : "badge--warn";

  const vramText = hw.gpu
    ? `${(hw.gpu.vram_total_mb / 1024).toFixed(1)} GB VRAM (${(hw.gpu.vram_free_mb / 1024).toFixed(1)} GB free)`
    : "Shared System Memory (Integrated/CPU)";

  return `
    <div class="panel-card" style="grid-column: 1 / -1; margin-block-end: 0.75rem; padding: 0.85rem 1.1rem; border: 1px solid var(--color-border); border-radius: var(--radius-sm);">
      <div style="display: flex; justify-content: space-between; align-items: flex-start; gap: 1rem; flex-wrap: wrap; margin-block-end: 0.6rem;">
        <div>
          <div style="display: flex; align-items: center; gap: 0.5rem; flex-wrap: wrap;">
            <strong style="font-size: 0.95rem;">🖥️ Host Hardware Profile & Recommendations</strong>
            <span class="badge ${tierBadgeClass}">${escapeHtml(hw.tier_label)}</span>
          </div>
          <p class="muted" style="font-size: 0.82rem; margin: 0.2rem 0 0 0;">
            ${escapeHtml(hw.tier_description)}
          </p>
        </div>
        <button type="button" id="btn-apply-hardware-models" class="btn btn--sm btn--primary" style="display: inline-flex; align-items: center; gap: 0.35rem;">
          <span>⚡ Adopt Recommended Models</span>
        </button>
      </div>

      <div style="display: grid; grid-template-columns: repeat(auto-fit, minmax(210px, 1fr)); gap: 0.6rem; font-size: 0.82rem; margin-block-end: 0.6rem;">
        <div style="background: var(--color-bg-base); padding: 0.45rem 0.65rem; border-radius: var(--radius-sm); border: 1px solid var(--color-border-subtle);">
          <span class="muted">GPU:</span> <strong>${escapeHtml(hw.gpu?.name || "Integrated / CPU-only")}</strong><br/>
          <span class="muted">VRAM:</span> <strong>${escapeHtml(vramText)}</strong>
        </div>
        <div style="background: var(--color-bg-base); padding: 0.45rem 0.65rem; border-radius: var(--radius-sm); border: 1px solid var(--color-border-subtle);">
          <span class="muted">System RAM:</span> <strong>${(hw.system_ram_total_mb / 1024).toFixed(1)} GB</strong> (${(hw.system_ram_available_mb / 1024).toFixed(1)} GB avail)<br/>
          <span class="muted">Queue Concurrency:</span> <strong style="text-transform: capitalize;">${escapeHtml(hw.recommended_models.concurrency_mode.replace("_", " "))}</strong>
        </div>
        <div style="background: var(--color-bg-base); padding: 0.45rem 0.65rem; border-radius: var(--radius-sm); border: 1px solid var(--color-border-subtle);">
          <span class="muted">Fast Model:</span> <code>${escapeHtml(hw.recommended_models.fast_model)}</code><br/>
          <span class="muted">Coder / Deep:</span> <code>${escapeHtml(hw.recommended_models.coder_model)}</code> / <code>${escapeHtml(hw.recommended_models.heavy_model)}</code>
        </div>
      </div>

      <div style="background: rgba(255, 122, 24, 0.08); border-left: 3px solid var(--color-accent); padding: 0.45rem 0.65rem; border-radius: 0 var(--radius-sm) var(--radius-sm) 0; font-size: 0.82rem;">
        <strong>Hardware Rationale:</strong> ${escapeHtml(hw.recommended_models.rationale)}
      </div>
    </div>
  `;
}

function renderSettingsTab(): string {
  const cfg = state.config!;
  const editingIdx = state.editingAltProviderIdx;
  const editingProv =
    editingIdx !== null && cfg.alt_provider && cfg.alt_provider[editingIdx]
      ? cfg.alt_provider[editingIdx]
      : null;
  const draft = editingProv
    ? {
        preset: getPresetForProvider(editingProv),
        name: editingProv.name,
        base_url: editingProv.base_url,
        api_key: editingProv.api_key,
        tier: editingProv.tier || "medium",
        model: editingProv.model,
        max_context_chars: editingProv.max_context_chars,
      }
    : state.altProviderFormDraft;
  const currentPreset = draft ? draft.preset : "";
  const liveFetched = currentPreset && state.fetchedModelsForPreset[currentPreset]
    ? state.fetchedModelsForPreset[currentPreset]
    : (draft && draft.base_url && state.fetchedModelsForPreset[draft.base_url]
      ? state.fetchedModelsForPreset[draft.base_url]
      : null);
  const modelOptions = liveFetched
    ? liveFetched.map((m) => ({ id: m, label: m }))
    : (currentPreset && PRESET_MODELS[currentPreset] ? PRESET_MODELS[currentPreset] : []);

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
    ${
      !cfg.wiki_configured
        ? `<div class="stage-alert" style="background:var(--wiki-surface); border:1px solid var(--wiki-accent); border-radius:var(--wiki-radius-sm); padding:0.75rem 1rem; margin-block-end:1rem; display:flex; justify-content:space-between; align-items:center; gap:1rem; flex-wrap:wrap;">
            <div style="display:flex; flex-direction:column; gap:0.25rem;">
              <strong style="color:var(--wiki-accent); font-size:0.95rem;">📖 Projects Wiki & AI Documentation Hub Available</strong>
              <span class="muted" style="font-size:0.82rem;">Would you like to set up the Projects Wiki for your repositories or skip it? You can always change this in Settings.</span>
            </div>
            <div class="row-inline" style="gap:0.5rem;">
              <button type="button" id="banner-open-wizard-btn" class="btn btn--sm btn--primary">🛠 Set Up Wiki</button>
              <button type="button" id="banner-skip-wizard-btn" class="btn btn--sm">Skip for Now</button>
            </div>
          </div>`
        : ""
    }

    <!-- Projects Wiki & AI Documentation Hub Integration -->
    <section class="panel-card">
      <div class="panel-header">
        <div class="row-inline">
          <h2 class="panel-title">📖 Projects Wiki & AI Documentation Hub</h2>
          ${
            cfg.wiki_enabled !== false
              ? `<span class="badge badge--good">Enabled</span>`
              : `<span class="badge badge--neutral">Disabled (Button Hidden)</span>`
          }
          ${
            state.health?.wiki_reachable
              ? `<span class="badge badge--good">Live</span>`
              : `<span class="badge badge--bad">Offline</span>`
          }
        </div>
        <span class="muted">Local-first MkDocs Material documentation hub and offline Ollama AI document builder.</span>
      </div>

      <div style="display:flex; flex-direction:column; gap:0.9rem;">
        <div class="row-between" style="padding:0.75rem 1rem; background:var(--wiki-surface); border:1px solid var(--wiki-border); border-radius:var(--wiki-radius-sm); flex-wrap:wrap; gap:0.75rem; align-items:center;">
          <div style="flex:1 1 300px;">
            <strong>Feature Visibility & Status:</strong>
            <p class="muted" style="margin:0.25rem 0 0 0; font-size:0.8rem;">
              ${
                cfg.wiki_enabled !== false
                  ? "The Projects Wiki tab is active in Kodiak's top navigation bar."
                  : "The Projects Wiki tab is hidden. Enabling it restores the navigation tab and background services."
              }
            </p>
          </div>
          <div class="row-inline" style="gap:0.5rem; flex-wrap:wrap;">
            <button
              type="button"
              id="toggle-wiki-feature-btn"
              class="btn ${cfg.wiki_enabled !== false ? "btn--danger" : "btn--primary"}"
            >
              ${cfg.wiki_enabled !== false ? "Hide Wiki Tab (Disable Feature)" : "Show Wiki Tab (Enable Feature)"}
            </button>
            <button type="button" id="settings-open-wizard-btn" class="btn btn--accent">
              🛠 Run Setup Wizard
            </button>
          </div>
        </div>

        <div style="display:grid; grid-template-columns:1fr 1fr; gap:0.75rem; font-size:0.82rem;">
          <div style="padding:0.65rem 0.85rem; border:1px solid var(--wiki-border); border-radius:var(--wiki-radius-sm); background:rgba(0,0,0,0.15);">
            <span class="muted">Projects Directory (Shared):</span>
            <div class="mono" style="margin-top:0.25rem; word-break:break-all;"><strong>${escapeHtml(cfg.projects_dir || "~/Projects")}</strong></div>
          </div>
          <div style="padding:0.65rem 0.85rem; border:1px solid var(--wiki-border); border-radius:var(--wiki-radius-sm); background:rgba(0,0,0,0.15);">
            <span class="muted">Wiki Application Path:</span>
            <div class="mono" style="margin-top:0.25rem; word-break:break-all;"><strong>${escapeHtml(cfg.wiki_dir || "./ProjectsWiki")}</strong></div>
          </div>
        </div>

        ${
          cfg.wiki_enabled !== false
            ? `<div class="row-inline" style="gap:0.5rem; flex-wrap:wrap; align-items:center;">
                <button type="button" id="settings-sync-wiki-btn" class="btn btn--sm" ${state.syncingWiki ? "disabled" : ""}>
                  ${state.syncingWiki ? "Syncing Catalog..." : "🔄 Sync Projects Catalog"}
                </button>
                <button type="button" id="settings-rebuild-assets-btn" class="btn btn--sm" ${state.rebuildingWikiAssets ? "disabled" : ""}>
                  ${state.rebuildingWikiAssets ? "Rebuilding Assets..." : "📦 Rebuild Assets & PDFs"}
                </button>
                ${
                  state.health?.wiki_reachable
                    ? `<button type="button" id="settings-stop-wiki-btn" class="btn btn--sm btn--danger" ${state.stoppingWiki ? "disabled" : ""}>⏹ Stop Server</button>`
                    : `<button type="button" id="settings-start-wiki-btn" class="btn btn--sm btn--primary" ${state.startingWiki ? "disabled" : ""}>▶ Start Server</button>`
                }
                <button type="button" id="settings-open-ai-modal-btn" class="btn btn--sm btn--accent">🤖 AI Doc Builder</button>
              </div>`
            : ""
        }
      </div>
    </section>

    <!-- Project Whitelist & Per-Directory Grant Matrix -->
    <section class="panel-card">
      <div class="panel-header" id="settings-projects-toggle" style="cursor:pointer; user-select:none;" title="Click to collapse/expand">
        <div class="row-inline">
          <h2 class="panel-title">Project Whitelist & Grant Matrix</h2>
          <span class="badge badge--neutral">${cfg.whitelist.length}</span>
          ${renderCharterChip(2, "Human authority")}
        </div>
        <div class="row-inline">
          <span class="muted">Only whitelisted directories are readable/writable. Levels compose downward only.</span>
          <span class="mono muted" style="font-size:0.8rem; font-weight:600; padding:0.15rem 0.45rem; border:1px solid var(--wiki-border); border-radius:var(--wiki-radius-sm);">
            ${state.settingsProjectsCollapsed ? "▶ Expand" : "▼ Collapse"}
          </span>
        </div>
      </div>

      ${
        state.settingsProjectsCollapsed
          ? `<div class="row-between" style="padding:0.75rem 0.9rem; font-size:0.85rem; background:var(--wiki-surface); border-radius:var(--wiki-radius-sm); border:1px dashed var(--wiki-border);">
              <div class="row-inline" style="gap:0.55rem; align-items:center;">
                <span class="badge badge--ok">Active Project</span>
                <span class="mono"><strong>${escapeHtml(cfg.active_project || "(None selected)")}</strong></span>
              </div>
              <span class="muted">${cfg.whitelist.length} directories whitelisted &bull; Click "Expand" to manage directory grants</span>
            </div>`
          : `
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
      `
      }
    </section>

    <!-- Cloud & Providers ([[alt_provider]]) -->
    <section class="panel-card ${editingProv ? "is-editing" : ""}" id="edit-alt-provider-panel">
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

      <div class="row-between" style="margin-top:0.6rem; align-items:center;">
        <h3 id="edit-alt-provider-heading" style="margin:0; font-size:0.95rem; font-weight:600;">
          ${editingProv ? `Edit Alternate Provider (${escapeHtml(editingProv.name)})` : "Add Alternate Provider"}
        </h3>
        ${editingProv ? `<button type="button" id="cancel-edit-alt-provider-btn" class="btn btn--sm">Cancel Edit</button>` : ""}
      </div>

      <div class="alt-provider-form">
        <!-- Row 1: Preset, Name, Tier, Max Chars -->
        <div class="alt-provider-form__row">
          <div class="form-field">
            <label class="form-label" for="new-alt-preset">Preset</label>
            <select id="new-alt-preset" class="form-select">
              <option value="" ${!currentPreset ? "selected" : ""}>-- Choose Preset --</option>
              <option value="gemini" ${currentPreset === "gemini" ? "selected" : ""}>Gemini (OpenAI-compat)</option>
              <option value="lumo" ${currentPreset === "lumo" ? "selected" : ""}>Lumo (lumo-tamer bridge)</option>
              <option value="groq" ${currentPreset === "groq" ? "selected" : ""}>Groq</option>
              <option value="openrouter" ${currentPreset === "openrouter" ? "selected" : ""}>OpenRouter</option>
              <option value="ollama" ${currentPreset === "ollama" ? "selected" : ""}>Self-hosted Ollama</option>
            </select>
          </div>
          <div class="form-field">
            <label class="form-label" for="new-alt-name">Name</label>
            <input type="text" id="new-alt-name" class="form-input" value="${draft ? escapeHtml(draft.name) : ""}" placeholder="e.g. Gemini" />
          </div>
          <div class="form-field">
            <label class="form-label" for="new-alt-tier">Tier</label>
            <select id="new-alt-tier" class="form-select">
              <option value="low" ${draft?.tier === "low" ? "selected" : ""}>Low (Fast / Cheap)</option>
              <option value="medium" ${!draft?.tier || draft?.tier === "medium" ? "selected" : ""}>Medium (Standard)</option>
              <option value="high" ${draft?.tier === "high" ? "selected" : ""}>High (Premium / Deep)</option>
            </select>
          </div>
          <div class="form-field">
            <label class="form-label" for="new-alt-max-chars">Max Context Chars</label>
            <input type="number" id="new-alt-max-chars" class="form-input" value="${draft?.max_context_chars ? draft.max_context_chars : ""}" placeholder="Optional (e.g. 128000)" />
          </div>
        </div>

        <!-- Row 2: Base URL and API Key -->
        <div class="alt-provider-form__row--split">
          <div class="form-field">
            <label class="form-label" for="new-alt-url">Base URL</label>
            <input type="text" id="new-alt-url" class="form-input" value="${draft ? escapeHtml(draft.base_url) : ""}" placeholder="https://generativelanguage.googleapis.com/v1beta/openai/" />
          </div>
          <div class="form-field">
            <label class="form-label" for="new-alt-key">API Key</label>
            <input type="password" id="new-alt-key" class="form-input" value="${draft ? escapeHtml(draft.api_key) : ""}" placeholder="API key (stored locally)" />
          </div>
        </div>

        <!-- Row 3: Model Selection & Live Fetch -->
        <div class="form-field">
          <div class="row-between" style="align-items:center; margin-bottom:0.25rem;">
            <label class="form-label" for="new-alt-model" style="margin:0;">Model ID / Agent</label>
            <button
              type="button"
              id="fetch-live-models-btn"
              class="btn btn--sm"
              style="font-size:0.75rem; padding:0.2rem 0.65rem;"
              title="Query the provider API (Google ModelService.ListModels, Groq, Ollama, etc.) to list active live models"
              ${state.fetchingLiveModels ? "disabled" : ""}
            >
              ${state.fetchingLiveModels ? "Fetching live models..." : "↻ Fetch Live Models"}
            </button>
          </div>
          <div style="display:flex; gap:0.5rem; align-items:center;">
            <select id="new-alt-model-select" class="form-select" style="flex:1 1 50%;">
              ${
                modelOptions.length > 0
                  ? modelOptions
                      .map(
                        (m) =>
                          `<option value="${escapeHtml(m.id)}" ${
                            (draft ? draft.model : "") === m.id ? "selected" : ""
                          }>${escapeHtml(m.label)}</option>`
                      )
                      .join("") +
                    `<option value="__custom__" ${
                      draft && !modelOptions.some((m) => m.id === draft.model) ? "selected" : ""
                    }>Custom Model ID...</option>`
                  : `<option value="__custom__" selected>Custom / Enter ID below...</option>`
              }
            </select>
            <input
              type="text"
              id="new-alt-model"
              class="form-input"
              style="flex:1 1 50%; min-width:140px;"
              value="${draft ? escapeHtml(draft.model) : ""}"
              placeholder="e.g. gemini-3.0-flash"
            />
          </div>
          <div id="fetch-models-status-msg" style="font-size:0.78rem; margin-top:0.3rem;">
            ${state.fetchModelsStatus || ""}
          </div>
        </div>

        <!-- Row 4: Actions -->
        <div class="alt-provider-form__actions">
          ${
            editingProv
              ? `<button type="button" id="save-alt-provider-btn" class="btn btn--primary" style="min-width:140px;">Save Changes</button>
                 <button type="button" id="cancel-edit-alt-provider-btn" class="btn btn--neutral">Cancel</button>`
              : `<button type="button" id="add-alt-provider-btn" class="btn btn--primary" style="min-width:160px;">+ Add Provider</button>`
          }
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
                      <button type="button" class="btn btn--sm edit-alt-provider-btn" data-idx="${idx}">
                        Edit
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
              <br/>• <strong>Tier Low (Fast/Efficient):</strong> <code>gemini-3.0-flash</code>
              <br/>• <strong>Tier High (Deep Reasoning):</strong> <code>gemini-1.5-pro</code>
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

        ${renderHardwareCard(state.hardwareProfile)}

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
            <span class="badge badge--ok" style="font-size:0.85rem;">v1.0.0</span>
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
            <span>Kodiak Task Delegation System (v1.0.0)</span>
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

function renderQueueDrawer(): string {
  if (!state.queueDrawerOpen) return "";

  const items = state.queueItems;
  const runningItem = items.find(
    (i) => i.status === "interpreting" || i.status === "local_working" || i.status === "verifying"
  );

  const inbox = state.unifiedInbox;
  const inboxTotal = inbox?.total_pending ?? 0;
  const inboxConflicts = inbox?.conflict_count ?? 0;

  // Tab Navigation Header Pills (§2.2 Slice 2)
  const tabNavHtml = `
    <div class="queue-drawer__tabs">
      <button type="button" class="queue-tab-btn ${state.queueDrawerTab === "queue" ? "is-active" : ""}" id="queue-tab-btn-tasks">
        <span>📋 Tasks (${items.length})</span>
        ${runningItem ? `<span class="badge badge--ok pulse" style="font-size:0.65rem; padding:0.1rem 0.35rem;">Active</span>` : ""}
      </button>
      <button type="button" class="queue-tab-btn ${state.queueDrawerTab === "inbox" ? "is-active" : ""}" id="queue-tab-btn-inbox">
        <span>📥 Approval Inbox (${inboxTotal})</span>
        ${inboxConflicts > 0 ? `<span class="badge badge--bad" style="font-size:0.65rem; padding:0.1rem 0.35rem;">⚠️ ${inboxConflicts} conflict${inboxConflicts > 1 ? "s" : ""}</span>` : ""}
      </button>
    </div>
  `;

  let bodyContent = "";

  if (state.queueDrawerTab === "queue") {
    const itemsHtml = items.length === 0
      ? `<div style="text-align:center; padding:3rem 1rem; color:var(--md-default-fg-color--light);">
           <span style="font-size:2rem; display:block; margin-bottom:0.75rem;">📋</span>
           <strong>No tasks currently queued</strong>
           <p style="font-size:0.85rem; margin-top:0.35rem;">Enter a prompt in the workbench bar and hit Enter to queue tasks asynchronously without blocking!</p>
         </div>`
      : items.map((item, idx) => {
          let badgeHtml = "";
          let pulseClass = "";
          switch (item.status) {
            case "queued":
              badgeHtml = `<span class="badge badge--neutral">Queued #${idx + 1}</span>`;
              break;
            case "interpreting":
              badgeHtml = `<span class="badge badge--info" style="background:#0284c7; color:#fff;">Stage 1: Interpreting...</span>`;
              pulseClass = "is-active";
              break;
            case "waiting_for_dependency":
              badgeHtml = `<span class="badge badge--warn" title="Paused until earlier tasks in this repository are reviewed">⏳ Waiting for Dependency</span>`;
              break;
            case "local_working":
              badgeHtml = `<span class="badge badge--ok" style="background:#16a34a; color:#fff;" title="Active local model inference under VRAM lock">⚡ Local Working (VRAM)</span>`;
              pulseClass = "is-active";
              break;
            case "verifying":
              badgeHtml = `<span class="badge badge--info" style="background:#2563eb; color:#fff;">Stage 3: Verifying...</span>`;
              pulseClass = "is-active";
              break;
            case "waiting_approval":
              badgeHtml = `<span class="badge" style="background:#a855f7; color:#fff;">✋ Waiting Approval (${item.pending_cards_count})</span>`;
              break;
            case "completed":
              badgeHtml = `<span class="badge badge--ok">✓ Completed</span>`;
              break;
            case "failed":
              badgeHtml = `<span class="badge badge--bad">✕ Failed</span>`;
              break;
            case "cancelled":
              badgeHtml = `<span class="badge badge--neutral">Cancelled</span>`;
              break;
          }

          const projName = item.project_root.split("/").pop() || item.project_root;
          const canCancel = item.status === "queued" || item.status === "interpreting" || item.status === "waiting_for_dependency";
          const canRemove = item.status === "completed" || item.status === "failed" || item.status === "cancelled";
          const canReorder = item.status === "queued" || item.status === "waiting_for_dependency";

          return `
            <div class="queue-item-card ${pulseClass}">
              <div class="row-between" style="align-items:flex-start;">
                ${badgeHtml}
                <div class="queue-item-card__controls">
                  ${canReorder && idx > 0 ? `<button type="button" class="btn btn--sm queue-move-up-btn" data-queue-id="${escapeHtml(item.queue_id)}" data-new-idx="${idx - 1}" title="Move up">▲</button>` : ""}
                  ${canReorder && idx < items.length - 1 ? `<button type="button" class="btn btn--sm queue-move-down-btn" data-queue-id="${escapeHtml(item.queue_id)}" data-new-idx="${idx + 1}" title="Move down">▼</button>` : ""}
                  ${canCancel ? `<button type="button" class="btn btn--sm queue-cancel-btn" data-queue-id="${escapeHtml(item.queue_id)}" title="Cancel this queued task">Cancel</button>` : ""}
                  ${canRemove ? `<button type="button" class="btn btn--sm queue-remove-btn" data-queue-id="${escapeHtml(item.queue_id)}" title="Remove from queue list">✕</button>` : ""}
                </div>
              </div>
              <div class="queue-item-card__prompt">${escapeHtml(item.prompt)}</div>
              <div class="queue-item-card__meta">
                <span>📁 <strong>${escapeHtml(projName)}</strong></span>
                ${item.task_id ? `<span class="mono" style="font-size:0.72rem;">${escapeHtml(item.task_id)}</span>` : ""}
              </div>
              ${item.error ? `<div class="banner banner--error" style="padding:0.4rem 0.6rem; font-size:0.75rem; margin-top:0.25rem;">${escapeHtml(item.error)}</div>` : ""}
              ${item.status === "waiting_approval" && item.task_id ? `
                <div style="margin-top:0.25rem;">
                  <button type="button" class="btn btn--sm btn--primary queue-view-task-btn" data-task-id="${escapeHtml(item.task_id)}" style="width:100%;">
                    Review ${item.pending_cards_count} Pending Card(s) →
                  </button>
                </div>
              ` : ""}
            </div>
          `;
        }).join("");

    bodyContent = `
      <div style="padding:0.6rem 1.25rem; background:var(--wiki-surface); border-bottom:1px solid var(--wiki-border); font-size:0.82rem; display:flex; align-items:center; justify-content:space-between;">
        <label class="row-inline" style="gap:0.4rem; cursor:pointer; user-select:none;">
          <input type="checkbox" id="queue-wait-approval-toggle" ${state.queueWaitForApproval ? "checked" : ""} />
          <span>Wait for previous task approval in same repository</span>
        </label>
      </div>
      <div class="queue-drawer__body">
        ${itemsHtml}
      </div>
    `;
  } else {
    // UNIFIED INBOX TAB (§2.2 Slice 2)
    const allInboxItems = inbox?.items ?? [];
    const filteredItems = allInboxItems.filter((it) => {
      if (state.inboxFilter === "conflicts") return it.has_conflict;
      if (state.inboxFilter === "safe") return !it.has_conflict;
      return true;
    });

    const safeCount = allInboxItems.filter((i) => !i.has_conflict).length;
    const conflictCount = allInboxItems.filter((i) => i.has_conflict).length;
    const selectedCount = state.selectedInboxCardIds.length;

    // Test Hook Result Notification
    let testHookBanner = "";
    if (state.lastBatchApprovalResult && state.lastBatchApprovalResult.test_hook_results.length > 0) {
      testHookBanner = state.lastBatchApprovalResult.test_hook_results.map((hook) => `
        <div class="inbox-test-hook-panel">
          <div class="row-between" style="align-items:center;">
            <div class="row-inline" style="gap:0.4rem; align-items:center;">
              <span class="badge ${hook.success ? 'badge--ok' : 'badge--bad'}">${hook.success ? '✓ Tests Passed' : '✕ Tests Failed'}</span>
              <strong style="font-size:0.8rem;">${escapeHtml(hook.command)}</strong>
            </div>
            <span class="mono" style="font-size:0.72rem; color:var(--md-default-fg-color--light);">${escapeHtml(hook.project_dir.split('/').pop() || hook.project_dir)}</span>
          </div>
          ${hook.digested_summary ? `
            <div class="inbox-digest-box">
              <div style="font-weight:600; font-size:0.72rem; margin-bottom:0.25rem; text-transform:uppercase; letter-spacing:0.04em;">⚡ Local llama3.2:3b Compiler Digest:</div>
              <div style="white-space:pre-wrap; font-size:0.75rem; line-height:1.4;">${escapeHtml(hook.digested_summary)}</div>
            </div>
          ` : ""}
          <details style="font-size:0.73rem; margin-top:0.2rem;">
            <summary style="cursor:pointer; color:var(--md-default-fg-color--light);">View Raw Output (${hook.raw_output.length} bytes)</summary>
            <pre class="inbox-diff-preview" style="margin-top:0.3rem;">${escapeHtml(hook.raw_output)}</pre>
          </details>
        </div>
      `).join("");
    }

    // Toolbar
    const toolbarHtml = `
      <div class="inbox-toolbar">
        <div class="inbox-toolbar__row">
          <div class="inbox-filter-group">
            <button type="button" class="inbox-filter-btn ${state.inboxFilter === 'all' ? 'is-active' : ''}" data-inbox-filter="all">All (${allInboxItems.length})</button>
            <button type="button" class="inbox-filter-btn ${state.inboxFilter === 'safe' ? 'is-active' : ''}" data-inbox-filter="safe">Safe (${safeCount})</button>
            <button type="button" class="inbox-filter-btn ${state.inboxFilter === 'conflicts' ? 'is-active' : ''}" data-inbox-filter="conflicts">Conflicts (${conflictCount})</button>
          </div>
          <div class="row-inline" style="gap:0.35rem;">
            <button type="button" id="inbox-select-safe-btn" class="btn btn--sm" title="Select all safe, non-conflicting cards">Select Safe</button>
            <button type="button" id="inbox-select-all-btn" class="btn btn--sm" title="Select all visible cards">All</button>
            <button type="button" id="inbox-deselect-all-btn" class="btn btn--sm" title="Clear selection">Clear</button>
          </div>
        </div>
        <div class="inbox-toolbar__row" style="border-top:1px solid rgba(255,255,255,0.06); padding-top:0.5rem;">
          <label class="row-inline" style="gap:0.4rem; cursor:pointer; user-select:none; font-size:0.78rem;">
            <input type="checkbox" id="inbox-test-hook-checkbox" ${state.inboxRunTestHook ? "checked" : ""} />
            <span>Run local test hook (e.g. <code>cargo check</code>) after apply</span>
          </label>
          <div class="inbox-batch-actions">
            <button type="button" id="inbox-batch-reject-btn" class="btn btn--sm" ${selectedCount === 0 || state.batchApproving ? 'disabled' : ''} title="Reject selected cards with a reason">✕ Reject (${selectedCount})</button>
            <button type="button" id="inbox-batch-approve-btn" class="btn btn--sm btn--primary" ${selectedCount === 0 || state.batchApproving ? 'disabled' : ''} title="Approve and execute selected cards sequentially">
              ${state.batchApproving ? "Applying..." : `✓ Approve (${selectedCount})`}
            </button>
          </div>
        </div>
      </div>
    `;

    // Group items by project
    const projectGroups: Record<string, UnifiedInboxItem[]> = {};
    for (const item of filteredItems) {
      if (!projectGroups[item.project_dir]) {
        projectGroups[item.project_dir] = [];
      }
      projectGroups[item.project_dir].push(item);
    }

    const itemsHtml = filteredItems.length === 0
      ? `<div style="text-align:center; padding:3rem 1rem; color:var(--md-default-fg-color--light);">
           <span style="font-size:2rem; display:block; margin-bottom:0.75rem;">🎉</span>
           <strong>No cards match this filter</strong>
           <p style="font-size:0.85rem; margin-top:0.35rem;">All tasks are clear or pending cards are filtered out.</p>
         </div>`
      : Object.entries(projectGroups).map(([projDir, pItems]) => {
          const projName = projDir.split("/").pop() || projDir;
          const cardsHtml = pItems.map((item) => {
            const cardKey = `${item.task_id}:${item.card.card_id}`;
            const isSelected = state.selectedInboxCardIds.includes(cardKey);

            let rangesBadge = "";
            if (item.line_ranges.length > 0) {
              const rangesStr = item.line_ranges.map(([s, e]) => s === e ? `L${s}` : `L${s}-${e}`).join(", ");
              rangesBadge = `<span class="badge badge--neutral" style="font-size:0.7rem;">${rangesStr}</span>`;
            } else if (item.card.target_path) {
              rangesBadge = `<span class="badge badge--neutral" style="font-size:0.7rem;">Whole file</span>`;
            }

            return `
              <div class="inbox-card ${item.has_conflict ? 'has-conflict' : ''} ${isSelected ? 'is-selected' : ''}">
                <div class="row-between" style="align-items:flex-start;">
                  <label class="row-inline" style="gap:0.5rem; cursor:pointer; align-items:flex-start;">
                    <input type="checkbox" class="inbox-card-checkbox" data-card-key="${escapeHtml(cardKey)}" ${isSelected ? 'checked' : ''} style="margin-top:0.25rem;" />
                    <div>
                      <div class="row-inline" style="gap:0.35rem; align-items:center; flex-wrap:wrap;">
                        <span class="badge badge--warn" style="font-size:0.68rem; text-transform:uppercase;">${escapeHtml(item.card.action_class)}</span>
                        ${rangesBadge}
                        <code style="font-size:0.76rem; word-break:break-all;">${escapeHtml(item.card.target_path || item.card.title)}</code>
                      </div>
                      <div style="font-size:0.82rem; font-weight:500; margin-top:0.25rem;">${escapeHtml(item.card.title)}</div>
                    </div>
                  </label>
                  <button type="button" class="btn btn--sm inbox-jump-task-btn" data-task-id="${escapeHtml(item.task_id)}" title="Open task in workbench">View Task →</button>
                </div>

                ${item.has_conflict ? `
                  <div class="inbox-conflict-banner">
                    <div style="font-weight:600; display:flex; align-items:center; gap:0.3rem;">
                      <span>⚠️ Diff Conflict Collision Detected</span>
                    </div>
                    <div style="font-size:0.75rem;">${escapeHtml(item.conflict_description || "Overlaps with another queued task")}</div>
                    <div style="font-size:0.72rem; opacity:0.85;">Conflicting task: ${escapeHtml(item.conflicting_tasks.join(", "))}</div>
                  </div>
                ` : ""}

                ${item.card.diff_or_command ? `
                  <pre class="inbox-diff-preview">${escapeHtml(item.card.diff_or_command.slice(0, 600))}${item.card.diff_or_command.length > 600 ? '\n... (truncated)' : ''}</pre>
                ` : ""}

                <div class="row-between" style="font-size:0.73rem; color:var(--md-default-fg-color--light); align-items:center;">
                  <span>Task: <strong>${escapeHtml(item.task_title || item.task_id)}</strong></span>
                  <span class="mono">${escapeHtml(item.card.card_id)}</span>
                </div>
              </div>
            `;
          }).join("");

          return `
            <div class="inbox-group">
              <div class="inbox-group__title">
                <span>📁 ${escapeHtml(projName)}</span>
                <span class="mono" style="font-size:0.68rem; font-weight:normal; text-transform:none;">${escapeHtml(projDir)}</span>
              </div>
              ${cardsHtml}
            </div>
          `;
        }).join("");

    bodyContent = `
      ${toolbarHtml}
      <div class="queue-drawer__body">
        ${testHookBanner}
        ${itemsHtml}
      </div>
    `;
  }

  return `
    <div class="queue-drawer-backdrop" id="queue-drawer-backdrop">
      <div class="queue-drawer" onclick="event.stopPropagation();">
        <div class="queue-drawer__header">
          <div class="row-inline" style="gap:0.5rem; align-items:center;">
            <h2 class="panel-title" style="margin:0;">📋 Task Queue & Inbox</h2>
          </div>
          <div class="row-inline" style="gap:0.5rem; align-items:center;">
            ${state.queueDrawerTab === "queue"
              ? `<button type="button" id="clear-completed-queue-btn" class="btn btn--sm" title="Clear completed, failed, and cancelled tasks">Clear Finished</button>`
              : `<button type="button" id="refresh-inbox-btn" class="btn btn--sm" title="Refresh Unified Inbox and scan diffs">↻ Refresh</button>`}
            <button type="button" id="close-queue-drawer-btn" class="btn btn--sm" title="Close drawer">✕</button>
          </div>
        </div>
        ${tabNavHtml}
        ${bodyContent}
      </div>
    </div>
  `;
}

function renderApp(): void {
  const appEl = document.getElementById("app");
  if (!appEl || !state.config) return;

  if (state.tab === "wiki" && state.config.wiki_enabled === false) {
    state.tab = "workbench";
  }

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

  const prevMain = document.querySelector(".main-content") as HTMLElement | null;
  const prevMainScroll = prevMain ? prevMain.scrollTop : 0;
  const prevSidebar = document.querySelector(".sidebar__scroll") as HTMLElement | null;
  const prevSidebarScroll = prevSidebar ? prevSidebar.scrollTop : 0;
  const prevTicker = document.getElementById("ticker-body");
  const prevTop = prevTicker?.scrollTop ?? 0;

  const prevScrollMap = new Map<string, number>();
  document.querySelectorAll<HTMLElement>("[id]").forEach((el) => {
    if (el.id && el.scrollTop > 0 && el.id !== "ticker-body") {
      prevScrollMap.set(el.id, el.scrollTop);
    }
  });

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
    ${renderWikiSetupWizard()}
    ${renderQueueDrawer()}
  `;

  const restoreScrollPositions = () => {
    const newMain = document.querySelector(".main-content") as HTMLElement | null;
    if (newMain && prevMainScroll > 0) {
      newMain.scrollTop = prevMainScroll;
    }
    const newSidebar = document.querySelector(".sidebar__scroll") as HTMLElement | null;
    if (newSidebar && prevSidebarScroll > 0) {
      newSidebar.scrollTop = prevSidebarScroll;
    }
    for (const [id, top] of prevScrollMap.entries()) {
      const el = document.getElementById(id);
      if (el) el.scrollTop = top;
    }
    const tk = document.getElementById("ticker-body");
    if (tk) tk.scrollTop = state.tickerLocked || tickerHover ? prevTop : tk.scrollHeight;
  };

  // Immediate restore attempt
  restoreScrollPositions();
  // Double RAF to wait for browser layout reflow so scrollHeight calculation does not clamp to 0
  requestAnimationFrame(() => {
    restoreScrollPositions();
    requestAnimationFrame(restoreScrollPositions);
  });

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

async function openWikiWizard(): Promise<void> {
  try {
    const info = await invoke<WikiSetupInfo>("get_wiki_setup_info");
    state.wikiWizardInfo = info;
    if (info.projects_dir) {
      state.wikiWizardProjectsDir = info.projects_dir;
    }
    state.wikiWizardDetected = info.detected_projects || [];
    state.wikiWizardEnableService = info.service_enabled ?? true;
  } catch (err) {
    console.warn("Failed to get wiki setup info:", err);
    if (!state.wikiWizardProjectsDir) {
      state.wikiWizardProjectsDir = state.config?.projects_dir || "~/Projects";
    }
  }
  state.wikiWizardStep = 1;
  state.wikiWizardOpen = true;
  renderApp();
}

async function skipWikiSetup(): Promise<void> {
  try {
    const updated = await invoke<AppConfig>("set_wiki_enabled", { enabled: false });
    state.config = updated;
    state.wikiWizardOpen = false;
    if (state.tab === "wiki") {
      state.tab = "workbench";
    }
    state.uiBannerInfo = "Projects Wiki skipped. The navigation tab has been hidden. You can enable it anytime in Settings.";
    await refreshAllState();
  } catch (err) {
    state.uiBannerError = `Failed to skip wiki: ${String(err)}`;
    renderApp();
  }
}

async function scanWizardProjectsDir(dirPath: string): Promise<void> {
  state.wikiWizardScanning = true;
  renderApp();
  try {
    const detected = await invoke<string[]>("detect_projects_in_dir", { dirPath });
    state.wikiWizardDetected = detected;
  } catch (err) {
    state.uiBannerError = `Failed to scan directory: ${String(err)}`;
  } finally {
    state.wikiWizardScanning = false;
    renderApp();
  }
}

async function completeWikiWizard(): Promise<void> {
  state.wikiWizardSaving = true;
  renderApp();
  try {
    const payload: WikiSetupPayload = {
      enable_wiki: true,
      projects_dir: state.wikiWizardProjectsDir,
      add_to_whitelist: state.wikiWizardAddToWhitelist,
      enable_service: state.wikiWizardEnableService,
      start_now: state.wikiWizardStartNow,
    };
    const updated = await invoke<AppConfig>("complete_wiki_setup", { payload });
    state.config = updated;
    state.wikiWizardOpen = false;
    state.tab = "wiki";
    state.uiBannerInfo = "Projects Wiki setup complete! Repositories cataloged and wiki started.";
    await refreshAllState();
  } catch (err) {
    state.uiBannerError = `Wiki setup error: ${String(err)}`;
  } finally {
    state.wikiWizardSaving = false;
    renderApp();
  }
}

async function toggleWikiFeature(): Promise<void> {
  if (!state.config) return;
  const target = !(state.config.wiki_enabled !== false);
  try {
    const updated = await invoke<AppConfig>("set_wiki_enabled", { enabled: target });
    state.config = updated;
    if (!target && state.tab === "wiki") {
      state.tab = "workbench";
    }
    state.uiBannerInfo = target
      ? "Projects Wiki enabled. The navigation tab is now restored."
      : "Projects Wiki disabled. The navigation tab has been hidden.";
    await refreshAllState();
  } catch (err) {
    state.uiBannerError = `Failed to toggle wiki: ${String(err)}`;
    renderApp();
  }
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
  document.getElementById("sidebar-projects-toggle")?.addEventListener("click", () => {
    state.sidebarProjectsCollapsed = !state.sidebarProjectsCollapsed;
    renderApp();
  });
  document.getElementById("sidebar-memories-toggle")?.addEventListener("click", () => {
    state.sidebarMemoriesCollapsed = !state.sidebarMemoriesCollapsed;
    renderApp();
  });
  document.getElementById("sidebar-tasks-toggle")?.addEventListener("click", () => {
    state.sidebarTasksCollapsed = !state.sidebarTasksCollapsed;
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

  promptEl?.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      document.getElementById("run-pipeline-btn")?.click();
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

  // Run pipeline / enqueue task asynchronously without blocking prompt input (§2.1 Slice 2)
  document.getElementById("run-pipeline-btn")?.addEventListener("click", async () => {
    const prompt = state.promptText.trim();
    if (!prompt) {
      state.uiBannerError = "Please enter a task prompt before submitting.";
      renderApp();
      return;
    }
    state.readOnlyHistorical = false;
    state.uiBannerError = null;
    state.uiBannerInfo = null;

    try {
      const item = await invoke<QueueItem>("enqueue_task", {
        prompt,
        projectRoot: state.config?.active_project ?? null,
        waitForPreviousApproval: state.queueWaitForApproval ?? true,
      });
      state.promptText = "";
      state.uiBannerInfo = `Task queued (#${item.queue_id}) — running asynchronously via multi-task queue.`;
      const qItems = await invoke<QueueItem[]>("get_queue_items");
      state.queueItems = qItems;
      renderApp();
    } catch (err) {
      state.uiBannerError = `Failed to queue task: ${String(err)}`;
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

  // Wiki tab controls & Service operations
  const handleStartWiki = async () => {
    state.startingWiki = true;
    state.uiBannerError = null;
    renderApp();
    try {
      state.health = await invoke<HealthStatus>("start_wiki_server");
      await refreshAllState();
    } catch (err) {
      state.uiBannerError = String(err);
    } finally {
      state.startingWiki = false;
      renderApp();
    }
  };
  document.getElementById("start-wiki-btn")?.addEventListener("click", handleStartWiki);
  document.getElementById("settings-start-wiki-btn")?.addEventListener("click", handleStartWiki);

  const handleStopWiki = async () => {
    state.stoppingWiki = true;
    state.uiBannerError = null;
    renderApp();
    try {
      state.health = await invoke<HealthStatus>("stop_wiki_server");
      await refreshAllState();
    } catch (err) {
      state.uiBannerError = String(err);
    } finally {
      state.stoppingWiki = false;
      renderApp();
    }
  };
  document.getElementById("stop-wiki-btn")?.addEventListener("click", handleStopWiki);
  document.getElementById("settings-stop-wiki-btn")?.addEventListener("click", handleStopWiki);

  const handleSyncWiki = async () => {
    state.syncingWiki = true;
    state.uiBannerError = null;
    renderApp();
    try {
      const msg = await invoke<string>("sync_wiki_catalog");
      state.uiBannerInfo = msg || "Projects Wiki catalog synced.";
      await refreshAllState();
      const iframe = document.getElementById("projects-wiki-iframe") as HTMLIFrameElement | null;
      if (iframe && state.health?.wiki_url) {
        iframe.src = state.health.wiki_url;
      }
    } catch (err) {
      state.uiBannerError = String(err);
    } finally {
      state.syncingWiki = false;
      renderApp();
    }
  };
  document.getElementById("sync-wiki-btn")?.addEventListener("click", handleSyncWiki);
  document.getElementById("settings-sync-wiki-btn")?.addEventListener("click", handleSyncWiki);

  const handleRebuildWikiAssets = async () => {
    state.rebuildingWikiAssets = true;
    state.uiBannerError = null;
    renderApp();
    try {
      const msg = await invoke<string>("rebuild_wiki_assets");
      state.uiBannerInfo = msg || "Assets indexed and catalog synced.";
      await refreshAllState();
      const iframe = document.getElementById("projects-wiki-iframe") as HTMLIFrameElement | null;
      if (iframe && state.health?.wiki_url) {
        iframe.src = state.health.wiki_url;
      }
    } catch (err) {
      state.uiBannerError = String(err);
    } finally {
      state.rebuildingWikiAssets = false;
      renderApp();
    }
  };
  document.getElementById("rebuild-wiki-assets-btn")?.addEventListener("click", handleRebuildWikiAssets);
  document.getElementById("settings-rebuild-assets-btn")?.addEventListener("click", handleRebuildWikiAssets);

  document.getElementById("recheck-wiki-btn")?.addEventListener("click", async () => {
    await refreshAllState();
  });

  document.getElementById("reload-wiki-iframe-btn")?.addEventListener("click", () => {
    const iframe = document.getElementById("projects-wiki-iframe") as HTMLIFrameElement | null;
    if (iframe && state.health?.wiki_url) {
      iframe.src = state.health.wiki_url;
    }
  });

  // Wiki Setup Wizard & Feature Toggle Listeners
  document.getElementById("banner-open-wizard-btn")?.addEventListener("click", openWikiWizard);
  document.getElementById("settings-open-wizard-btn")?.addEventListener("click", openWikiWizard);
  document.getElementById("banner-skip-wizard-btn")?.addEventListener("click", skipWikiSetup);
  document.getElementById("toggle-wiki-feature-btn")?.addEventListener("click", toggleWikiFeature);

  document.getElementById("wizard-close-x-btn")?.addEventListener("click", () => {
    state.wikiWizardOpen = false;
    renderApp();
  });

  document.getElementById("wizard-skip-step1-btn")?.addEventListener("click", () => {
    skipWikiSetup();
  });

  document.getElementById("wizard-next-step1-btn")?.addEventListener("click", () => {
    state.wikiWizardStep = 2;
    if (state.wikiWizardDetected.length === 0 && state.wikiWizardProjectsDir) {
      scanWizardProjectsDir(state.wikiWizardProjectsDir);
    } else {
      renderApp();
    }
  });

  document.getElementById("wizard-back-step2-btn")?.addEventListener("click", () => {
    state.wikiWizardStep = 1;
    renderApp();
  });

  document.getElementById("wizard-scan-dir-btn")?.addEventListener("click", () => {
    const input = (document.getElementById("wizard-projects-dir-input") as HTMLInputElement)?.value.trim();
    if (input) {
      state.wikiWizardProjectsDir = input;
      scanWizardProjectsDir(input);
    }
  });

  document.getElementById("wizard-next-step2-btn")?.addEventListener("click", () => {
    const input = (document.getElementById("wizard-projects-dir-input") as HTMLInputElement)?.value.trim();
    if (input) {
      state.wikiWizardProjectsDir = input;
    }
    const addWhitelist = (document.getElementById("wizard-add-to-whitelist-cb") as HTMLInputElement)?.checked ?? true;
    state.wikiWizardAddToWhitelist = addWhitelist;
    state.wikiWizardStep = 3;
    renderApp();
  });

  document.getElementById("wizard-back-step3-btn")?.addEventListener("click", () => {
    state.wikiWizardStep = 2;
    renderApp();
  });

  document.getElementById("wizard-next-step3-btn")?.addEventListener("click", () => {
    const enableSvc = (document.getElementById("wizard-enable-service-cb") as HTMLInputElement)?.checked ?? true;
    const startNow = (document.getElementById("wizard-start-now-cb") as HTMLInputElement)?.checked ?? true;
    state.wikiWizardEnableService = enableSvc;
    state.wikiWizardStartNow = startNow;
    state.wikiWizardStep = 4;
    renderApp();
  });

  document.getElementById("wizard-back-step4-btn")?.addEventListener("click", () => {
    state.wikiWizardStep = 3;
    renderApp();
  });

  document.getElementById("wizard-finish-btn")?.addEventListener("click", () => {
    completeWikiWizard();
  });

  // AI Documentation Generator Modal
  const handleOpenAiModal = () => {
    state.wikiAiModalOpen = true;
    state.wikiAiStatusMessage = null;
    state.wikiAiOutput = null;
    if (!state.wikiAiSelectedProject && state.config?.active_project) {
      state.wikiAiSelectedProject = state.config.active_project.split("/").pop() || "";
    }
    renderApp();
  };
  document.querySelectorAll<HTMLElement>("#open-wiki-ai-modal-btn").forEach((btn) => {
    btn.addEventListener("click", handleOpenAiModal);
  });
  document.getElementById("settings-open-ai-modal-btn")?.addEventListener("click", handleOpenAiModal);

  document.getElementById("close-wiki-ai-modal-btn")?.addEventListener("click", () => {
    state.wikiAiModalOpen = false;
    renderApp();
  });

  document.getElementById("close-wiki-ai-modal-cancel-btn")?.addEventListener("click", () => {
    state.wikiAiModalOpen = false;
    renderApp();
  });

  document.getElementById("run-wiki-ai-build-btn")?.addEventListener("click", async () => {
    const projSel = (document.getElementById("wiki-ai-project-select") as HTMLSelectElement)?.value;
    const modelSel = (document.getElementById("wiki-ai-model-select") as HTMLSelectElement)?.value;
    const overwrite = (document.getElementById("wiki-ai-overwrite-checkbox") as HTMLInputElement)?.checked ?? true;

    if (!projSel) {
      state.wikiAiStatusMessage = "Please select a target project.";
      renderApp();
      return;
    }

    state.buildingWikiAi = true;
    state.wikiAiSelectedProject = projSel;
    state.wikiAiSelectedModel = modelSel || "qwen2.5-coder:7b";
    state.wikiAiOverwrite = overwrite;
    state.wikiAiStatusMessage = `Analyzing ${projSel} and synthesizing documentation with local model ${state.wikiAiSelectedModel}...`;
    renderApp();

    try {
      const res = await invoke<string>("ai_build_project_wiki", {
        projectName: projSel,
        model: state.wikiAiSelectedModel,
        overwrite,
      });
      state.wikiAiOutput = res;
      state.wikiAiStatusMessage = `Documentation generated and catalog updated for ${projSel}!`;
      await refreshAllState();
      const iframe = document.getElementById("projects-wiki-iframe") as HTMLIFrameElement | null;
      if (iframe && state.health?.wiki_url) {
        const cleanSlug = projSel.replace(/ /g, "_");
        iframe.src = `${state.health.wiki_url}/${cleanSlug}/`;
      }
    } catch (err) {
      state.wikiAiStatusMessage = `Error building documentation: ${String(err)}`;
    } finally {
      state.buildingWikiAi = false;
      renderApp();
    }
  });

  // Workbench Quick Wiki link
  document.querySelectorAll<HTMLElement>(".quick-wiki-btn").forEach((btn) => {
    btn.addEventListener("click", () => {
      state.tab = "wiki";
      renderApp();
      if (state.config?.active_project && state.health?.wiki_url) {
        const clean = state.config.active_project.split("/").pop()?.replace(/ /g, "_");
        const iframe = document.getElementById("projects-wiki-iframe") as HTMLIFrameElement | null;
        if (iframe && clean) {
          iframe.src = `${state.health.wiki_url}/${clean}/`;
        }
      }
    });
  });

  // Settings: Projects whitelist toggle collapse/expand
  document.getElementById("settings-projects-toggle")?.addEventListener("click", () => {
    state.settingsProjectsCollapsed = !state.settingsProjectsCollapsed;
    renderApp();
  });

  // Settings: Local Decision Journal toggle collapse/expand
  document.getElementById("journal-toggle-btn")?.addEventListener("click", () => {
    state.journalCollapsed = !state.journalCollapsed;
    renderApp();
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
          cloudModelInput.value = tier === "high" ? (found.model.includes("pro") ? found.model : "gemini-2.5-pro") : "gemini-3.0-flash";
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
    state.config.wiki_dir = wikiDir || "./ProjectsWiki";
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

  // Settings: Apply hardware-recommended models
  document.getElementById("btn-apply-hardware-models")?.addEventListener("click", async () => {
    if (!state.hardwareProfile) return;
    const rec = state.hardwareProfile.recommended_models;
    const fastInput = document.getElementById("cfg-fast-model") as HTMLInputElement | null;
    const coderInput = document.getElementById("cfg-coder-model") as HTMLInputElement | null;
    const heavyInput = document.getElementById("cfg-heavy-model") as HTMLInputElement | null;
    if (fastInput) fastInput.value = rec.fast_model;
    if (coderInput) coderInput.value = rec.coder_model;
    if (heavyInput) heavyInput.value = rec.heavy_model;
    try {
      state.config = await invoke<AppConfig>("apply_hardware_recommended_models");
      state.uiBannerInfo = `Adopted hardware-recommended models (${rec.fast_model}, ${rec.coder_model}, ${rec.heavy_model}) for ${state.hardwareProfile.tier_label}.`;
      await refreshAllState();
    } catch (err) {
      state.uiBannerError = `Failed to apply recommended models: ${err}`;
      renderApp();
    }
  });

  // Settings: Preset dropdown & Tier change in Add/Edit Provider form
  const presetEl = document.getElementById("new-alt-preset") as HTMLSelectElement | null;
  const newAltNameEl = document.getElementById("new-alt-name") as HTMLInputElement | null;
  const newAltUrlEl = document.getElementById("new-alt-url") as HTMLInputElement | null;
  const newAltKeyEl = document.getElementById("new-alt-key") as HTMLInputElement | null;
  const newAltTierEl = document.getElementById("new-alt-tier") as HTMLSelectElement | null;
  const newAltModelEl = document.getElementById("new-alt-model") as HTMLInputElement | null;
  const newAltModelSelect = document.getElementById("new-alt-model-select") as HTMLSelectElement | null;
  const newAltMaxCharsEl = document.getElementById("new-alt-max-chars") as HTMLInputElement | null;

  const syncFormDraft = () => {
    const chosenModel = (newAltModelSelect?.value && newAltModelSelect.value !== "__custom__")
      ? newAltModelSelect.value
      : (newAltModelEl?.value.trim() || "");
    state.altProviderFormDraft = {
      preset: presetEl?.value || "",
      name: newAltNameEl?.value.trim() || "",
      base_url: newAltUrlEl?.value.trim() || "",
      api_key: newAltKeyEl?.value.trim() || "",
      tier: newAltTierEl?.value || "medium",
      model: chosenModel,
      max_context_chars: newAltMaxCharsEl?.value ? Number(newAltMaxCharsEl.value) : undefined,
    };
  };

  newAltNameEl?.addEventListener("input", syncFormDraft);
  newAltUrlEl?.addEventListener("input", syncFormDraft);
  newAltKeyEl?.addEventListener("input", syncFormDraft);
  newAltTierEl?.addEventListener("change", syncFormDraft);
  newAltMaxCharsEl?.addEventListener("input", syncFormDraft);

  const populateModelOptionsForPreset = (preset: string, selectedModel?: string) => {
    if (!newAltModelSelect) return;
    const live = state.fetchedModelsForPreset[preset];
    const opts = live ? live.map((m) => ({ id: m, label: m })) : (PRESET_MODELS[preset] || []);
    let html = "";
    if (opts.length > 0) {
      html = opts.map((m) => `<option value="${escapeHtml(m.id)}">${escapeHtml(m.label)}</option>`).join("");
      html += `<option value="__custom__">Custom Model ID...</option>`;
    } else {
      html = `<option value="__custom__">Custom / Enter ID below...</option>`;
    }
    newAltModelSelect.innerHTML = html;
    if (selectedModel && (opts.some((m) => m.id === selectedModel) || selectedModel === "__custom__")) {
      newAltModelSelect.value = selectedModel;
    } else if (opts.length > 0) {
      newAltModelSelect.value = opts[0].id;
    } else {
      newAltModelSelect.value = "__custom__";
    }
  };

  const fetchLiveModelsForCurrentForm = async () => {
    syncFormDraft();
    const urlEl = document.getElementById("new-alt-url") as HTMLInputElement | null;
    const keyEl = document.getElementById("new-alt-key") as HTMLInputElement | null;
    const nameEl = document.getElementById("new-alt-name") as HTMLInputElement | null;
    const presetEl = document.getElementById("new-alt-preset") as HTMLSelectElement | null;
    const modelSelect = document.getElementById("new-alt-model-select") as HTMLSelectElement | null;
    const modelInput = document.getElementById("new-alt-model") as HTMLInputElement | null;
    const tierEl = document.getElementById("new-alt-tier") as HTMLSelectElement | null;
    const fetchBtn = document.getElementById("fetch-live-models-btn") as HTMLButtonElement | null;
    const statusEl = document.getElementById("fetch-models-status-msg");

    const baseUrl = urlEl?.value.trim() ?? "";
    const apiKey = keyEl?.value.trim() ?? "";
    const providerName = nameEl?.value.trim() || presetEl?.value || "";

    if (!baseUrl) {
      const msg = `<span class="badge badge--bad">Enter Base URL first</span>`;
      state.fetchModelsStatus = msg;
      if (statusEl) statusEl.innerHTML = msg;
      return;
    }

    state.fetchingLiveModels = true;
    if (fetchBtn) {
      fetchBtn.disabled = true;
      fetchBtn.textContent = "Fetching...";
    }
    const pendingMsg = `<span class="badge badge--neutral">Querying provider for live models...</span>`;
    state.fetchModelsStatus = pendingMsg;
    if (statusEl) statusEl.innerHTML = pendingMsg;

    try {
      const models = await invoke<string[]>("list_provider_models", {
        baseUrl,
        apiKey,
        providerName: providerName || null,
      });

      state.fetchingLiveModels = false;
      if (!models || models.length === 0) {
        const warnMsg = `<span class="badge badge--warn">No text generation models returned</span>`;
        state.fetchModelsStatus = warnMsg;
        if (statusEl) statusEl.innerHTML = warnMsg;
        return;
      }

      const cacheKey = presetEl?.value || baseUrl;
      state.fetchedModelsForPreset[cacheKey] = models;
      const okMsg = `<span class="badge badge--ok">✓ Fetched ${models.length} live models from provider</span>`;
      state.fetchModelsStatus = okMsg;
      if (statusEl) statusEl.innerHTML = okMsg;

      let html = models.map((m) => `<option value="${escapeHtml(m)}">${escapeHtml(m)}</option>`).join("");
      html += `<option value="__custom__">Custom Model ID...</option>`;
      if (modelSelect) {
        modelSelect.innerHTML = html;
        const current = modelInput?.value.trim() || "";
        if (models.includes(current)) {
          modelSelect.value = current;
        } else {
          const tier = tierEl?.value || "medium";
          let best = models[0];
          if (tier === "high") {
            best = models.find((m) => m.includes("pro") || m.includes("70b") || m.includes("max")) || models[0];
          } else {
            best = models.find((m) => m.includes("flash") || m.includes("8b") || m.includes("lite") || m.includes("7b")) || models[0];
          }
          modelSelect.value = best;
          if (modelInput) modelInput.value = best;
        }
      }
      syncFormDraft();
    } catch (err) {
      state.fetchingLiveModels = false;
      const errMsg = `<span class="badge badge--bad">Fetch failed: ${escapeHtml(String(err))}</span>`;
      state.fetchModelsStatus = errMsg;
      if (statusEl) statusEl.innerHTML = errMsg;
    } finally {
      if (fetchBtn) {
        fetchBtn.disabled = false;
        fetchBtn.textContent = "↻ Fetch Live Models";
      }
    }
  };

  document.getElementById("fetch-live-models-btn")?.addEventListener("click", fetchLiveModelsForCurrentForm);

  newAltModelSelect?.addEventListener("change", () => {
    if (!newAltModelEl) return;
    const val = newAltModelSelect.value;
    if (val !== "__custom__") {
      newAltModelEl.value = val;
    } else {
      newAltModelEl.focus();
    }
    syncFormDraft();
  });

  newAltModelEl?.addEventListener("input", () => {
    if (!newAltModelSelect) return;
    const val = newAltModelEl.value.trim();
    let found = false;
    for (let i = 0; i < newAltModelSelect.options.length; i++) {
      if (newAltModelSelect.options[i].value === val) {
        newAltModelSelect.selectedIndex = i;
        found = true;
        break;
      }
    }
    if (!found) {
      newAltModelSelect.value = "__custom__";
    }
    syncFormDraft();
  });

  presetEl?.addEventListener("change", () => {
    const val = presetEl.value;
    populateModelOptionsForPreset(val);
    if (val === "lumo") {
      if (newAltNameEl) newAltNameEl.value = "Lumo-Alt";
      if (newAltUrlEl) newAltUrlEl.value = "http://127.0.0.1:3003/v1";
      if (newAltKeyEl) newAltKeyEl.value = "lumo";
      if (newAltTierEl) newAltTierEl.value = "medium";
      if (newAltModelSelect) newAltModelSelect.value = "lumo";
      if (newAltModelEl) newAltModelEl.value = "lumo";
    } else if (val === "gemini") {
      if (newAltNameEl) newAltNameEl.value = "Gemini";
      if (newAltUrlEl) newAltUrlEl.value = "https://generativelanguage.googleapis.com/v1beta/openai/";
      if (newAltKeyEl && !newAltKeyEl.value) newAltKeyEl.placeholder = "Google AI Studio API key";
      if (newAltTierEl) newAltTierEl.value = "low";
      if (newAltModelSelect) newAltModelSelect.value = "gemini-3.0-flash";
      if (newAltModelEl) newAltModelEl.value = "gemini-3.0-flash";
    } else if (val === "groq") {
      if (newAltNameEl) newAltNameEl.value = "Groq";
      if (newAltUrlEl) newAltUrlEl.value = "https://api.groq.com/openai/v1";
      if (newAltKeyEl && !newAltKeyEl.value) newAltKeyEl.placeholder = "Groq API key";
      if (newAltTierEl) newAltTierEl.value = "high";
      if (newAltModelSelect) newAltModelSelect.value = "llama-3.3-70b-versatile";
      if (newAltModelEl) newAltModelEl.value = "llama-3.3-70b-versatile";
    } else if (val === "openrouter") {
      if (newAltNameEl) newAltNameEl.value = "OpenRouter";
      if (newAltUrlEl) newAltUrlEl.value = "https://openrouter.ai/api/v1";
      if (newAltKeyEl && !newAltKeyEl.value) newAltKeyEl.placeholder = "OpenRouter API key";
      if (newAltTierEl) newAltTierEl.value = "high";
      if (newAltModelSelect) newAltModelSelect.value = "anthropic/claude-3.5-sonnet";
      if (newAltModelEl) newAltModelEl.value = "anthropic/claude-3.5-sonnet";
    } else if (val === "ollama") {
      if (newAltNameEl) newAltNameEl.value = "Self-Hosted-Ollama";
      if (newAltUrlEl) newAltUrlEl.value = "http://127.0.0.1:11434/v1";
      if (newAltKeyEl) newAltKeyEl.value = "ollama";
      if (newAltTierEl) newAltTierEl.value = "medium";
      if (newAltModelSelect) newAltModelSelect.value = "qwen2.5-coder:7b";
      if (newAltModelEl) newAltModelEl.value = "qwen2.5-coder:7b";
    }
    syncFormDraft();
  });

  newAltTierEl?.addEventListener("change", () => {
    const preset = presetEl?.value;
    const tier = newAltTierEl.value;
    if (preset === "gemini") {
      const model = tier === "high" ? "gemini-2.5-pro" : "gemini-3.0-flash";
      if (newAltModelSelect) newAltModelSelect.value = model;
      if (newAltModelEl) newAltModelEl.value = model;
    } else if (preset === "lumo") {
      const model = tier === "low" ? "lumo-lite" : tier === "high" ? "lumo-max" : "lumo";
      if (newAltModelSelect) newAltModelSelect.value = model;
      if (newAltModelEl) newAltModelEl.value = model;
    }
    syncFormDraft();
  });

  // Settings: Edit alternate provider
  document.querySelectorAll<HTMLElement>(".edit-alt-provider-btn").forEach((btn) => {
    btn.addEventListener("click", () => {
      const idx = Number(btn.getAttribute("data-idx"));
      if (!Number.isNaN(idx)) {
        state.editingAltProviderIdx = idx;
        renderApp();
        requestAnimationFrame(() => {
          const editPanel = document.getElementById("edit-alt-provider-panel");
          if (editPanel) {
            editPanel.scrollIntoView({ behavior: "smooth", block: "start" });
          }
        });
      }
    });
  });

  // Settings: Cancel edit alternate provider
  document.getElementById("cancel-edit-alt-provider-btn")?.addEventListener("click", () => {
    state.editingAltProviderIdx = null;
    state.altProviderFormDraft = null;
    state.fetchModelsStatus = null;
    renderApp();
  });

  // Settings: Save edited alternate provider
  document.getElementById("save-alt-provider-btn")?.addEventListener("click", async () => {
    if (!state.config || !state.config.alt_provider) return;
    const editingIdx = state.editingAltProviderIdx;
    if (editingIdx === null || !state.config.alt_provider[editingIdx]) return;

    const nameEl = document.getElementById("new-alt-name") as HTMLInputElement | null;
    const urlEl = document.getElementById("new-alt-url") as HTMLInputElement | null;
    const keyEl = document.getElementById("new-alt-key") as HTMLInputElement | null;
    const tierEl = document.getElementById("new-alt-tier") as HTMLSelectElement | null;
    const modelEl = document.getElementById("new-alt-model") as HTMLInputElement | null;
    const modelSelectEl = document.getElementById("new-alt-model-select") as HTMLSelectElement | null;
    const maxCharsEl = document.getElementById("new-alt-max-chars") as HTMLInputElement | null;

    const name = nameEl?.value.trim() || state.altProviderFormDraft?.name || "";
    const base_url = urlEl?.value.trim() || state.altProviderFormDraft?.base_url || "";
    const api_key = keyEl?.value.trim() || state.altProviderFormDraft?.api_key || "";
    const tier = tierEl?.value || state.altProviderFormDraft?.tier || "medium";
    let model = modelEl?.value.trim() || "";
    if (!model && modelSelectEl?.value && modelSelectEl.value !== "__custom__") {
      model = modelSelectEl.value;
    }
    if (!model && state.altProviderFormDraft?.model) {
      model = state.altProviderFormDraft.model;
    }
    const max_chars_raw = (maxCharsEl?.value || "").trim();
    const max_context_chars = max_chars_raw ? Number(max_chars_raw) : state.altProviderFormDraft?.max_context_chars;

    if (!name || !base_url || !api_key || !model) {
      const missing: string[] = [];
      if (!name) missing.push("Name");
      if (!base_url) missing.push("Base URL");
      if (!api_key) missing.push("API Key");
      if (!model) missing.push("Model");
      state.uiBannerError = `Required fields missing: ${missing.join(", ")}.`;
      renderApp();
      return;
    }

    const oldName = state.config.alt_provider[editingIdx].name;
    state.config.alt_provider[editingIdx] = {
      name,
      base_url,
      api_key,
      tier,
      model,
      max_context_chars,
    };

    if (state.config.cascade?.order && oldName !== name) {
      state.config.cascade.order = state.config.cascade.order.map((n) => (n === oldName ? name : n));
    }

    state.editingAltProviderIdx = null;
    state.altProviderFormDraft = null;
    state.fetchModelsStatus = null;

    try {
      state.config = await invoke<AppConfig>("update_config", { newConfig: state.config });
      state.uiBannerInfo = `Updated alternate provider '${name}'.`;
      await refreshAllState();
    } catch (err) {
      state.uiBannerError = String(err);
      renderApp();
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
    const modelSelectEl = document.getElementById("new-alt-model-select") as HTMLSelectElement | null;
    const maxCharsEl = document.getElementById("new-alt-max-chars") as HTMLInputElement | null;

    const name = nameEl?.value.trim() || state.altProviderFormDraft?.name || "";
    const base_url = urlEl?.value.trim() || state.altProviderFormDraft?.base_url || "";
    const api_key = keyEl?.value.trim() || state.altProviderFormDraft?.api_key || "";
    const tier = tierEl?.value || state.altProviderFormDraft?.tier || "medium";
    let model = modelEl?.value.trim() || "";
    if (!model && modelSelectEl?.value && modelSelectEl.value !== "__custom__") {
      model = modelSelectEl.value;
    }
    if (!model && state.altProviderFormDraft?.model) {
      model = state.altProviderFormDraft.model;
    }
    const max_chars_raw = (maxCharsEl?.value || "").trim();
    const max_context_chars = max_chars_raw ? Number(max_chars_raw) : state.altProviderFormDraft?.max_context_chars;

    if (!name || !base_url || !api_key || !model) {
      const missing: string[] = [];
      if (!name) missing.push("Name");
      if (!base_url) missing.push("Base URL");
      if (!api_key) missing.push("API Key");
      if (!model) missing.push("Model");
      state.uiBannerError = `Required fields missing: ${missing.join(", ")}.`;
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

    state.altProviderFormDraft = null;
    state.fetchModelsStatus = null;

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

  // Queue Drawer events (§2.1 Slice 2)
  document.getElementById("queue-drawer-toggle-btn")?.addEventListener("click", () => {
    state.queueDrawerOpen = !state.queueDrawerOpen;
    renderApp();
  });

  document.getElementById("close-queue-drawer-btn")?.addEventListener("click", () => {
    state.queueDrawerOpen = false;
    renderApp();
  });

  document.getElementById("queue-drawer-backdrop")?.addEventListener("click", (e) => {
    if ((e.target as HTMLElement)?.id === "queue-drawer-backdrop") {
      state.queueDrawerOpen = false;
      renderApp();
    }
  });

  document.getElementById("queue-wait-approval-toggle")?.addEventListener("change", (e) => {
    state.queueWaitForApproval = (e.target as HTMLInputElement).checked;
  });

  document.getElementById("clear-completed-queue-btn")?.addEventListener("click", async () => {
    try {
      const items = await invoke<QueueItem[]>("clear_completed_queue_items");
      state.queueItems = items;
      renderApp();
    } catch (err) {
      state.uiBannerError = `Cannot clear queue: ${err}`;
      renderApp();
    }
  });

  document.querySelectorAll<HTMLElement>(".queue-cancel-btn").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const queueId = btn.getAttribute("data-queue-id");
      if (!queueId) return;
      try {
        await invoke<QueueItem>("cancel_queue_item", { queueId });
        state.queueItems = await invoke<QueueItem[]>("get_queue_items");
        renderApp();
      } catch (err) {
        state.uiBannerError = `Cannot cancel queue item: ${err}`;
        renderApp();
      }
    });
  });

  document.querySelectorAll<HTMLElement>(".queue-remove-btn").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const queueId = btn.getAttribute("data-queue-id");
      if (!queueId) return;
      try {
        await invoke<QueueItem>("remove_queue_item", { queueId });
        state.queueItems = await invoke<QueueItem[]>("get_queue_items");
        renderApp();
      } catch (err) {
        state.uiBannerError = `Cannot remove queue item: ${err}`;
        renderApp();
      }
    });
  });

  document.querySelectorAll<HTMLElement>(".queue-move-up-btn, .queue-move-down-btn").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const queueId = btn.getAttribute("data-queue-id");
      const newIndexStr = btn.getAttribute("data-new-idx");
      if (!queueId || newIndexStr === null) return;
      const newIndex = parseInt(newIndexStr, 10);
      try {
        const updated = await invoke<QueueItem[]>("reorder_queue_item", { queueId, newIndex });
        state.queueItems = updated;
        renderApp();
      } catch (err) {
        state.uiBannerError = `Cannot reorder queue item: ${err}`;
        renderApp();
      }
    });
  });

  document.querySelectorAll<HTMLElement>(".queue-view-task-btn").forEach((btn) => {
    btn.addEventListener("click", () => {
      const taskId = btn.getAttribute("data-task-id");
      if (!taskId) return;
      const found = state.history.find((t) => t.task_id === taskId);
      if (found) {
        state.currentTask = found;
        state.readOnlyHistorical = false;
        state.tab = "workbench";
        state.queueDrawerOpen = false;
        renderApp();
      }
    });
  });

  // Unified Inbox events (§2.2 Slice 2 Phase B)
  document.getElementById("queue-tab-btn-tasks")?.addEventListener("click", () => {
    state.queueDrawerTab = "queue";
    renderApp();
  });

  document.getElementById("queue-tab-btn-inbox")?.addEventListener("click", () => {
    state.queueDrawerTab = "inbox";
    renderApp();
  });

  document.getElementById("refresh-inbox-btn")?.addEventListener("click", async () => {
    try {
      state.unifiedInbox = await invoke<UnifiedInboxResponse>("get_unified_inbox");
      renderApp();
    } catch (err) {
      state.uiBannerError = `Cannot refresh inbox: ${err}`;
      renderApp();
    }
  });

  document.querySelectorAll<HTMLElement>(".inbox-filter-btn").forEach((btn) => {
    btn.addEventListener("click", () => {
      const filter = btn.getAttribute("data-inbox-filter") as "all" | "conflicts" | "safe" | null;
      if (filter) {
        state.inboxFilter = filter;
        renderApp();
      }
    });
  });

  document.querySelectorAll<HTMLInputElement>(".inbox-card-checkbox").forEach((chk) => {
    chk.addEventListener("change", () => {
      const cardKey = chk.getAttribute("data-card-key");
      if (!cardKey) return;
      if (chk.checked) {
        if (!state.selectedInboxCardIds.includes(cardKey)) {
          state.selectedInboxCardIds.push(cardKey);
        }
      } else {
        state.selectedInboxCardIds = state.selectedInboxCardIds.filter((k) => k !== cardKey);
      }
      renderApp();
    });
  });

  document.getElementById("inbox-select-safe-btn")?.addEventListener("click", () => {
    if (!state.unifiedInbox) return;
    const safeKeys = state.unifiedInbox.items
      .filter((i) => !i.has_conflict)
      .map((i) => `${i.task_id}:${i.card.card_id}`);
    state.selectedInboxCardIds = safeKeys;
    renderApp();
  });

  document.getElementById("inbox-select-all-btn")?.addEventListener("click", () => {
    if (!state.unifiedInbox) return;
    const allKeys = state.unifiedInbox.items
      .filter((it) => {
        if (state.inboxFilter === "conflicts") return it.has_conflict;
        if (state.inboxFilter === "safe") return !it.has_conflict;
        return true;
      })
      .map((i) => `${i.task_id}:${i.card.card_id}`);
    state.selectedInboxCardIds = allKeys;
    renderApp();
  });

  document.getElementById("inbox-deselect-all-btn")?.addEventListener("click", () => {
    state.selectedInboxCardIds = [];
    renderApp();
  });

  document.getElementById("inbox-test-hook-checkbox")?.addEventListener("change", (e) => {
    state.inboxRunTestHook = (e.target as HTMLInputElement).checked;
  });

  document.getElementById("inbox-batch-approve-btn")?.addEventListener("click", async () => {
    if (state.selectedInboxCardIds.length === 0 || state.batchApproving) return;
    const targets: BatchCardTarget[] = state.selectedInboxCardIds.map((k) => {
      const idx = k.indexOf(":");
      return {
        task_id: k.slice(0, idx),
        card_id: k.slice(idx + 1),
      };
    });

    state.batchApproving = true;
    state.uiBannerError = null;
    renderApp();

    try {
      const res = await invoke<BatchApprovalResult>("batch_approve_cards", {
        targets,
        runTestHook: state.inboxRunTestHook,
      });
      state.lastBatchApprovalResult = res;
      state.selectedInboxCardIds = [];
      state.uiBannerInfo = `Batch approved ${res.approved_count} card(s) across ${res.affected_projects.length} project(s).`;
      await refreshAllState();
    } catch (err) {
      state.uiBannerError = `Batch approval error: ${err}`;
      renderApp();
    } finally {
      state.batchApproving = false;
      renderApp();
    }
  });

  document.getElementById("inbox-batch-reject-btn")?.addEventListener("click", async () => {
    if (state.selectedInboxCardIds.length === 0 || state.batchApproving) return;
    const reasonPrompt = window.prompt("Reason for rejecting selected card(s):", "Rejected in batch review");
    if (reasonPrompt === null) return; // user cancelled prompt

    const cleanReason = reasonPrompt.trim() || "Rejected by user in batch review";
    const targets: BatchCardTarget[] = state.selectedInboxCardIds.map((k) => {
      const idx = k.indexOf(":");
      return {
        task_id: k.slice(0, idx),
        card_id: k.slice(idx + 1),
      };
    });

    state.batchApproving = true;
    renderApp();

    try {
      const rejectedCount = await invoke<number>("batch_reject_cards", {
        targets,
        reason: cleanReason,
      });
      state.selectedInboxCardIds = [];
      state.uiBannerInfo = `Batch rejected ${rejectedCount} card(s).`;
      await refreshAllState();
    } catch (err) {
      state.uiBannerError = `Batch rejection error: ${err}`;
      renderApp();
    } finally {
      state.batchApproving = false;
      renderApp();
    }
  });

  document.querySelectorAll<HTMLElement>(".inbox-jump-task-btn").forEach((btn) => {
    btn.addEventListener("click", () => {
      const taskId = btn.getAttribute("data-task-id");
      if (!taskId) return;
      const found = state.history.find((t) => t.task_id === taskId);
      if (found) {
        state.currentTask = found;
        state.readOnlyHistorical = false;
        state.tab = "workbench";
        state.queueDrawerOpen = false;
        renderApp();
      }
    });
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
listen<QueueEvent>("kodiak://queue-event", (e) => {
  if (e.payload.kind === "queue_updated" && e.payload.items) {
    state.queueItems = e.payload.items;
    renderApp();
  }
});

refreshAllState();
