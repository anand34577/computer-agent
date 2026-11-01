import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type Profile = {
  vision: boolean;
  tools: boolean;
  reasoning: boolean;
  audio: boolean;
  embedding: boolean;
  context: number;
  tokens_per_sec: number;
  latency_ms: number;
  local: boolean;
  tested: boolean;
  tested_at: number;
  notes: string[];
};

export type Overrides = { vision?: boolean | null; tools?: boolean | null; reasoning?: boolean | null; audio?: boolean | null; embedding?: boolean | null };

export type Model = {
  id: string;
  provider_id: string;
  provider_name: string;
  provider_kind: string;
  name: string;
  label: string;
  profile: Profile;
  overrides: Overrides;
  effective: Profile;
  price_in: number;
  price_out: number;
  local: boolean;
};

export type Provider = { id: string; kind: string; name: string; base_url: string; local: boolean; has_key: boolean };

export type SandboxCfg = { kind: string; distro: string; image: string; network: boolean; allow_host: boolean; timeout_secs: number; vm_memory_mb: number; vm_idle_minutes: number };
export type McpServer = { id: string; name: string; transport: "stdio" | "http"; command: string; args: string[]; env: Record<string, string>; url: string; enabled: boolean; permission: string };

export type Settings = {
  theme: string;
  accent: string;
  app_name: string;
  assistant_name: string;
  radius: string;
  sidebar_collapsed: boolean;
  show_suggestions: boolean;
  send_on_enter: boolean;
  sandbox: SandboxCfg;
  llama: LlamaCfg;
  decision: DecisionCfg;
  mcp_servers: McpServer[];
  plugins: Record<string, { enabled: boolean; permission: string }>;
  web_search: boolean;
  text_size: number;
  density: string;
  layer: string;
  default_model: string | null;
  roles: { planner: string | null; doer: string | null; eyes: string | null; locator: string | null; locator_coords: string };
  fallback_model: string | null;
  local_only: boolean;
  system_prompt: string;
  chat_tools: boolean;
  reasoning: string;
  show_reasoning: boolean;
  tool_permissions: Record<string, string>;
  embed_model: string | null;
  doc_mode: "ask" | "embed" | "direct";
  subagents: { enabled: boolean; models: string[]; max_parallel: number; local_only: boolean };
  show_prices: boolean;
  autonomy: string;
  plan_first: boolean;
  draft_only: boolean;
  pointer_delay_ms: number;
  max_steps: number;
  max_task_minutes: number;
  max_retries: number;
  screenshot_width: number;
  settle_ms: number;
  yield_to_user: boolean;
  corner_pause: boolean;
  minimize_on_run: boolean;
  task_cost_cap: number;
  daily_cost_cap: number;
  allowed_folders: string[];
  protected_folders: string[];
  blocked_apps: string[];
  blocked_sites: string[];
  undo_hours: number;
  mask_secrets: boolean;
  keep_screenshots_days: number;
  audit_retention_days: number;
  telemetry: boolean;
  stop_shortcut: string;
  summon_shortcut: string;
  onboarded: boolean;
};

export type Attachment = { media_type: string; data: string; name?: string };

export type Message = {
  id: string;
  role: "user" | "assistant";
  content: string;
  images: Attachment[];
  reasoning: string;
  meta: { tools?: { name: string; args: unknown; result: string; permission?: string; ms?: number }[]; cost?: number; model?: string; task_id?: string; status?: string; stopped?: boolean; input_tokens?: number; output_tokens?: number; rating?: number; tps?: number; ttft_ms?: number; gen_ms?: number; docs?: DocRef[] };
  model_id: string | null;
  created: number;
};

export type Conversation = { id: string; title: string; folder: string; pinned: boolean; model_id: string | null; system_prompt: string; params: { temperature?: number | null; max_tokens?: number | null }; messages: Message[] };
export type DocRef = { id: string; name: string; mode: "embed" | "direct"; chars: number; chunks?: number; embed_model?: string };
export type ConvSummary = { id: string; title: string; folder: string; pinned: boolean; model_id: string | null; updated: number };

export type Task = {
  id: string;
  goal: string;
  status: string;
  cost: number;
  steps: number;
  started: number;
  ended: number | null;
  summary: string;
  models: { doer?: string; planner?: string; eyes?: string | null; locator?: string | null; mode?: string };
  undoable: number;
};

export type Step = {
  id: string;
  idx: number;
  kind: string;
  title: string;
  detail: string;
  action: Record<string, unknown>;
  result: string;
  screenshot: string;
  risk: string;
  model: string;
  cost: number;
  created: number;
};

export type DecisionCfg = {
  enabled: boolean; backend: "gguf" | "python"; url: string; model_repo: string; device: string; autostart: boolean; threshold: number; budget_ms: number;
  check_loading: boolean; check_result: boolean; check_done: boolean; check_plan: boolean; check_signin: boolean; log: boolean;
};
export type DecisionStatus = {
  backend: string; installed: boolean; engine_installed: boolean; setup_running: boolean; running: boolean; ready: boolean; local: boolean; dir: string;
  info: { model?: string; device?: string; dtype?: string }; last_ms: number | null; log: string[];
  files: { name: string; size: number; present: boolean; source: string }[]; model_dir: string;
};
export type DecisionAnswer = { type: "noul" | "choice"; decision: string; confidence: number; probabilities: Record<string, number> };
/** Latest fast-check answers during a task. */
export type FastChecks = { ms?: number; error?: string; paused?: boolean; items?: { name: string; decision: string; confidence: number; sure: boolean }[] } | null;

export type AgentState = {
  task_id: string;
  goal: string;
  status: string;
  message: string;
  step: number;
  cost: number;
  cost_cap: number;
  mode: string;
  doer: string;
  local: boolean;
  conversation_id?: string | null;
  /** The approved plan and the step the agent says it is on (1-based, 0 = unknown). */
  plan?: string[];
  plan_step?: number;
  /** Its latest note on what it sees and why, and the action it is doing now. */
  thought?: string;
  doing?: string;
  locator?: string | null;
  fast?: FastChecks;
};

/** A tool call in progress or finished, from a chat reply or an agent task. */
export type ToolEvent = { call_id: string; name: string; title?: string; args: unknown; status: "running" | "done"; result?: string; ms?: number; task_id?: string; conversation_id?: string | null };

export type Prompt = {
  id: string;
  kind: "plan" | "approval" | "handoff" | "question" | "limit" | "tool";
  task_id?: string;
  steps?: string[];
  goal?: string;
  description?: string;
  reason?: string;
  level?: string;
  draftable?: boolean;
  content?: string;
  tool?: string;
  args?: unknown;
  question?: string;
  message?: string;
};

export const api = {
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<Settings>("save_settings", { settings }),
  applyProfile: (profile: string) => invoke<Settings>("apply_profile", { profile }),
  exportSettings: () => invoke<string>("export_settings"),
  importSettings: (jsonText: string) => invoke<Settings>("import_settings", { jsonText }),
  resetSettings: () => invoke<Settings>("reset_settings"),
  testProvider: (kind: string, baseUrl: string, key?: string, providerId?: string) =>
    invoke<{ base_url: string; models: { name: string; size: number }[] }>("test_provider", { kind, baseUrl, key: key || null, providerId: providerId ?? null }),

  listProviders: () => invoke<Provider[]>("list_providers"),
  addProvider: (kind: string, name: string, baseUrl: string, key?: string) => invoke<string>("add_provider", { kind, name, baseUrl, key: key || null }),
  updateProvider: (id: string, name: string, baseUrl: string, key?: string) => invoke<void>("update_provider", { id, name, baseUrl, key: key || null }),
  deleteProvider: (id: string) => invoke<void>("delete_provider", { id }),
  remoteModels: (id: string) => invoke<{ name: string; size: number }[]>("provider_remote_models", { id }),
  detectLocal: () => invoke<{ name: string; kind: string; base_url: string; running: boolean }[]>("detect_local"),

  listModels: () => invoke<Model[]>("list_models"),
  addModel: (providerId: string, name: string) => invoke<string>("add_model", { providerId, name }),
  updateModel: (id: string, label: string, overrides: Overrides, priceIn: number, priceOut: number) =>
    invoke<void>("update_model", { id, label, overrides, priceIn, priceOut }),
  deleteModel: (id: string) => invoke<void>("delete_model", { id }),
  testModel: (id: string) => invoke<Profile>("test_model", { id }),
  hardware: () => invoke<{ ram_gb: number; ram_free_gb: number; cpu: string; cores: number; gpus: string[]; disk_free_gb: number }>("hardware_info"),
  catalog: () => invoke<CatalogItem[]>("model_catalog"),
  ollamaPull: (baseUrl: string, name: string) => invoke<void>("ollama_pull", { baseUrl, name }),
  ollamaDelete: (baseUrl: string, name: string) => invoke<void>("ollama_delete", { baseUrl, name }),

  /** Pinned and foldered chats, plus the newest `limit` others; `loose_total` counts all of those others. */
  listConversations: (query = "", limit = 40) => invoke<{ items: ConvSummary[]; loose_total: number }>("list_conversations", { query, limit }),
  createConversation: (modelId: string | null, folder = "") => invoke<string>("create_conversation", { modelId, folder }),
  getConversation: (id: string) => invoke<Conversation>("get_conversation", { id }),
  updateConversation: (id: string, patch: Record<string, unknown>) => invoke<void>("update_conversation", { id, patch }),
  deleteConversation: (id: string) => invoke<void>("delete_conversation", { id }),
  branchConversation: (id: string, messageId: string) => invoke<string>("branch_conversation", { id, messageId }),
  forkConversation: (id: string) => invoke<string>("fork_conversation", { id }),
  rateMessage: (conversationId: string, messageId: string, rating: number) => invoke<void>("rate_message", { conversationId, messageId, rating }),
  saveTextFile: (path: string, content: string) => invoke<void>("save_text_file", { path, content }),
  sendMessage: (conversationId: string, content: string, images: Attachment[], modelId: string, think: boolean | null = null, docs: DocRef[] = []) =>
    invoke<string>("send_message", { conversationId, content, images, modelId, think, docs }),
  docsAdd: (conversationId: string, name: string, mode: "embed" | "direct", src: { data: string }) =>
    invoke<DocRef>("docs_add", { conversationId, name, mode, data: src.data }),
  /** Native file dialog. Only paths picked here are accepted by the save and import commands. */
  pickPath: (mode: "open" | "save" | "folder", opts: { title?: string; filterName?: string; extensions?: string[]; defaultName?: string } = {}) =>
    invoke<string | null>("pick_path", { mode, title: opts.title ?? null, filterName: opts.filterName ?? null, extensions: opts.extensions ?? null, defaultName: opts.defaultName ?? null }),
  readTextFile: (path: string) => invoke<string>("read_text_file", { path }),
  docsList: (conversationId: string) => invoke<(DocRef & { created: number })[]>("docs_list", { conversationId }),
  docsRemove: (id: string) => invoke<void>("docs_remove", { id }),
  stats: (days: number) => invoke<Stats>("stats", { days }),
  regenerate: (conversationId: string, messageId: string, modelId: string, think: boolean | null = null) => invoke<void>("regenerate", { conversationId, messageId, modelId, think }),
  editAndResend: (conversationId: string, messageId: string, content: string, modelId: string, think: boolean | null = null) =>
    invoke<void>("edit_and_resend", { conversationId, messageId, content, modelId, think }),
  stopChat: (conversationId: string) => invoke<void>("stop_chat", { conversationId }),
  exportConversation: (id: string, format: string) => invoke<string>("export_conversation", { id, format }),
  captureScreenshot: () => invoke<string>("capture_screenshot"),

  startTask: (goal: string, conversationId: string | null, modelId: string | null = null) => invoke<string>("start_task", { goal, conversationId, modelId }),
  resumeTask: (taskId: string) => invoke<string>("resume_task", { taskId }),
  agentStop: () => invoke<void>("agent_stop"),
  /** Cancels one running tool call; the reply or task carries on. False if it already finished. */
  cancelTool: (callId: string) => invoke<boolean>("cancel_tool", { callId }),
  agentPause: () => invoke<void>("agent_pause"),
  agentResume: () => invoke<void>("agent_resume"),
  agentRespond: (id: string, value: Record<string, unknown>) => invoke<void>("agent_respond", { id, value }),
  agentStatus: () => invoke<{ running: boolean; paused: boolean; status: AgentState | null; prompts: Prompt[]; queue: { id: string; goal: string }[] }>("agent_status"),
  cancelQueued: (id: string) => invoke<void>("cancel_queued", { id }),

  listTasks: () => invoke<Task[]>("list_tasks"),
  taskSteps: (id: string, query?: string) => invoke<Step[]>("get_task_steps", { id, query: query ?? null }),
  readScreenshot: (path: string) => invoke<string>("read_screenshot", { path }),
  undoTask: (id: string) => invoke<{ restored: number; problems: string[] }>("undo_task", { id }),
  deleteTask: (id: string) => invoke<void>("delete_task", { id }),
  exportTask: (id: string) => invoke<string>("export_task", { id }),

  listPlaybooks: () => invoke<{ id: string; name: string; goal: string }[]>("list_playbooks"),
  savePlaybook: (name: string, goal: string) => invoke<void>("save_playbook", { name, goal }),
  deletePlaybook: (id: string) => invoke<void>("delete_playbook", { id }),

  wipeAll: (includeAudit: boolean) => invoke<void>("wipe_all_data", { includeAudit }),
  platformInfo: () => invoke<PlatformInfo>("platform_info"),
  diagnostics: () => invoke<string>("diagnostics_report"),
  usage: () => invoke<{ today: number; total: number }>("usage_summary"),
  recordConsent: (subject: string) => invoke<void>("record_consent", { subject }),
  hasConsent: (subject: string) => invoke<boolean>("has_consent", { subject }),
  reveal: (path: string) => invoke<void>("reveal_path", { path }),
  openLink: (url: string) => invoke<void>("open_link", { url }),
  setBarSize: (height: number) => invoke<void>("set_bar_size", { height }),

  listPlugins: () => invoke<PluginInfo[]>("list_plugins"),
  createPlugin: (name: string) => invoke<string>("create_plugin", { name }),
  importPlugin: (path: string) => invoke<string>("import_plugin", { path }),
  deletePlugin: (id: string) => invoke<void>("delete_plugin", { id }),
  setPluginPref: (id: string, enabled: boolean, permission: string) => invoke<void>("set_plugin_pref", { id, enabled, permission }),
  pluginsDir: () => invoke<string>("plugins_dir"),
  testPluginTool: (id: string, tool: string, args: unknown) => invoke<string>("test_plugin_tool", { id, tool, args }),
  saveMcpServer: (server: McpServer, secret?: string) => invoke<McpTest>("save_mcp_server", { server, secret: secret || null }),
  testMcpServer: (id: string) => invoke<McpTest>("test_mcp_server", { id }),
  deleteMcpServer: (id: string) => invoke<void>("delete_mcp_server", { id }),
  mcpStatus: () => invoke<{ id: string; connected: boolean; error: string | null; tools: number; has_secret: boolean }[]>("mcp_status"),
  sandboxDetect: () => invoke<{ wsl: boolean; wsl_distros: string[]; docker: boolean; docker_version: string }>("sandbox_detect"),
  sandboxTest: (cfg: SandboxCfg) => invoke<{ code: number; stdout: string; stderr: string; target: string; timed_out: boolean }>("sandbox_test", { cfg }),
  listAllTools: () => invoke<{ builtin: { name: string; description: string; scope: string; permission?: string }[]; extensions: { name: string; description: string; origin: string; permission: string }[] }>("list_all_tools"),
  dataDir: () => invoke<string>("data_dir"),
  sandboxFolder: () => invoke<string>("sandbox_folder"),
  sandboxExec: (command: string) => invoke<{ code: number; stdout: string; stderr: string; target: string; timed_out: boolean }>("sandbox_exec", { command }),

  vmStatus: () => invoke<VmStatus>("vm_status"),
  vmInstall: () => invoke<VmStatus>("vm_install"),
  vmRemove: () => invoke<void>("vm_remove"),
  vmStart: () => invoke<VmStatus>("vm_start"),
  vmStop: () => invoke<void>("vm_stop"),

  llamaStatus: () => invoke<LlamaStatus>("llama_status"),
  decisionStatus: () => invoke<DecisionStatus>("decision_status"),
  decisionSetup: (gpu: boolean) => invoke<void>("decision_setup", { gpu }),
  decisionStart: () => invoke<DecisionStatus>("decision_start"),
  decisionStop: () => invoke<DecisionStatus>("decision_stop"),
  decisionDeleteModel: () => invoke<DecisionStatus>("decision_delete_model"),
  decisionTry: (questions: { name: string; instructions: string; options: string[] | null }[], context: string, delaySecs: number) =>
    invoke<{ ms: number; answers: Record<string, DecisionAnswer>; image: string }>("decision_try", { questions, context, delaySecs }),
  decisionTest: () => invoke<{ ms: number; server_ms?: number; model?: string; answers: Record<string, { decision: string; confidence: number }> }>("decision_test"),
  llamaVariants: () => invoke<{ id: string; label: string; about: string; size: number; recommended?: boolean }[]>("llama_variants"),
  llamaInstall: (variant: string) => invoke<LlamaStatus>("llama_install", { variant }),
  llamaUninstall: () => invoke<void>("llama_uninstall"),
  llamaInspect: (path: string) => invoke<{ info: GgufInfo; projector: string | null }>("llama_inspect", { path }),
  llamaImport: (path: string, mmproj: string, ctx: number) => invoke<string>("llama_import", { path, mmproj, ctx }),
  llamaImportFolder: (dir: string) => invoke<{ added: string[]; skipped: string[] }>("llama_import_folder", { dir }),
  llamaUpdateModel: (id: string, mmproj: string, ctx: number) => invoke<void>("llama_update_model", { id, mmproj, ctx }),
  llamaRemoveModel: (id: string) => invoke<void>("llama_remove_model", { id }),
  llamaLoad: (id: string) => invoke<LlamaStatus>("llama_load", { id }),
  llamaUnload: () => invoke<void>("llama_unload"),
  llamaLog: () => invoke<string>("llama_log"),
  llamaDevices: (refresh = false) => invoke<LlamaDevice[]>("llama_devices", { refresh }),
  llamaEmbedCatalog: () => invoke<{ id: string; label: string; note: string; size: number; installed: boolean }[]>("llama_embed_catalog"),
  llamaFetchEmbed: (id: string) => invoke<string>("llama_fetch_embed", { id }),

  auditQuery: (query: AuditQuery) => invoke<AuditPage>("audit_query", { query }),
  auditExport: (query: AuditQuery, format: "csv" | "json") => invoke<string>("audit_export", { query, format }),
  auditVerify: () => invoke<{ ok: boolean; checked: number; broken_at?: number; reason?: string }>("audit_verify"),
};

export type LlamaStatus = { installed: boolean; variant: string; tag: string; installing: boolean; state: "stopped" | "loading" | "ready" | "error"; current: string | null; port: number; error: string; embed_current: string | null; embed_state: string };
export type GgufInfo = { arch: string; name: string; context_length: number; is_projector: boolean; is_embedding: boolean; size: number };
export type LlamaModel = { id: string; path: string; mmproj: string; ctx: number; size: number; arch: string; train_ctx: number; embedding: boolean };
export type LlamaDevice = { id: string; name: string; total_mb: number; free_mb: number; integrated: boolean };
export type LlamaCfg = {
  port: number; ctx_size: number; gpu_layers: number; threads: number; idle_minutes: number; extra_args: string; models: LlamaModel[];
  devices: string; split_mode: string; tensor_split: string; main_gpu: number; flash_attn: string; batch: number; ubatch: number;
  cache_k: string; cache_v: string; mlock: boolean; no_mmap: boolean; parallel: number; cpu_moe: number;
  temperature: number | null; top_k: number | null; top_p: number | null; min_p: number | null; repeat_penalty: number | null; embed_port: number;
};
export type ModelStat = { model_id: string; model: string; provider: string; replies: number; input: number; output: number; cost: number; avg_tps: number; max_tps: number; avg_ttft_ms: number; last: number; errors: number };
export type Stats = {
  totals: { replies: number; input: number; output: number; cost: number; avg_tps: number; avg_ttft_ms: number; errors: number; stopped: number };
  by_model: ModelStat[];
  daily: { day: string; replies: number; input: number; output: number; cost: number }[];
  hours: number[];
  tools: { name: string; calls: number; failed: number }[];
  tasks: Record<string, number>;
  chats: number; messages: number; documents: number; agent_steps: number;
};

export type VmStatus = { installed: boolean; installing: boolean; state: "off" | "starting" | "ready" | "error"; error: string; size_mb: number; boot_ms: number; memory_mb: number };

export type AuditQuery = { search?: string; category?: string; actor?: string; outcome?: string; session?: string; from?: number | null; to?: number | null; page: number; page_size: number };
export type AuditEntry = { id: number; ts: number; category: string; action: string; actor: "user" | "ai" | "system"; subject: string; session: string; outcome: string; details: Record<string, unknown> };
export type AuditPage = { items: AuditEntry[]; total: number; page: number; page_size: number; categories: string[] };

export type PluginInfo = {
  id: string;
  dir: string;
  manifest: { name: string; version: string; description: string; author: string; tools: { name: string; description: string; risk: string; parameters: Record<string, unknown> }[] } | null;
  error: string | null;
  enabled: boolean;
  permission: string;
};
export type McpTest = { id: string; ok: boolean; error?: string; tools: { name: string; description: string }[] };

export type CatalogItem = { name: string; size_gb: number; vision: boolean; tools: boolean; license: string; role: string; about: string };

export type PlatformInfo = {
  os: string;
  arch: string;
  version: string;
  wayland: boolean;
  capture: boolean;
  capture_error: string | null;
  accessibility: boolean;
  accessibility_backend: string;
  input: boolean;
  monitors: { name: string; width: number; height: number; scale: number; primary: boolean }[];
};

export function on<T>(event: string, cb: (payload: T) => void): () => void {
  let un: UnlistenFn | null = null;
  let cancelled = false;
  listen<T>(event, (e) => cb(e.payload)).then((u) => {
    if (cancelled) u();
    else un = u;
  });
  return () => {
    cancelled = true;
    un?.();
  };
}

export function errText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e);
}

export const money = (n: number) => (n < 0.01 && n > 0 ? `$${n.toFixed(4)}` : `$${n.toFixed(2)}`);

export function fileToAttachment(f: File): Promise<Attachment> {
  return new Promise((res, rej) => {
    const r = new FileReader();
    r.onload = () => {
      const s = String(r.result);
      res({ media_type: f.type || "image/png", data: s.slice(s.indexOf(",") + 1), name: f.name });
    };
    r.onerror = rej;
    r.readAsDataURL(f);
  });
}
