import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  BarChart3, Blocks, Bot, CheckSquare, Cloud, Cpu, Download, Folder, GitFork, History, Lock, MessageSquare, Moon, MoreHorizontal, Palette, Pencil, Pin, PinOff, Play, Plus,
  ScrollText, Search, Settings as SettingsIcon, ShieldCheck, Square, Sun, Trash2, X,
} from "lucide-react";
import { api, errText, money, on, type AgentState, type ConvSummary, type Model, type Prompt, type Settings } from "./api";
import { Callout, Checkbox, ConfirmDialog, Ctx, Menu, Modal, PromptDialog, Spinner, Toasts, type Toast, type ToastAction } from "./components/common";

const UNDO_MS = 6500;
import { saveExport, toMarkdown } from "./exporters";
import { live, useLiveBusy } from "./live";
import CommandPalette, { type Command } from "./components/CommandPalette";
import ErrorBoundary from "./components/ErrorBoundary";
import TitleBar from "./components/TitleBar";
import { applyTheme } from "./theme";
import { parseEngineErrors } from "./validate";
import ChatView from "./views/ChatView";
// Pages other than chat load on first visit, keeping startup light.
const ModelsView = lazy(() => import("./views/ModelsView"));
const ExtensionsView = lazy(() => import("./views/ExtensionsView"));
const ActivityView = lazy(() => import("./views/ActivityView"));
const StatsView = lazy(() => import("./views/StatsView"));
const AuditView = lazy(() => import("./views/AuditView"));
const SettingsView = lazy(() => import("./views/SettingsView"));
import Onboarding from "./views/Onboarding";

export type View = "chat" | "models" | "extensions" | "activity" | "stats" | "audit" | "settings";

const NAV: { id: View; label: string; icon: typeof MessageSquare; key: string }[] = [
  { id: "chat", label: "Chat", icon: MessageSquare, key: "1" },
  { id: "models", label: "Models", icon: Cpu, key: "2" },
  { id: "extensions", label: "Extensions", icon: Blocks, key: "3" },
  { id: "activity", label: "Activity", icon: History, key: "4" },
  { id: "stats", label: "Stats", icon: BarChart3, key: "5" },
  { id: "audit", label: "Audit log", icon: ScrollText, key: "6" },
  { id: "settings", label: "Settings", icon: SettingsIcon, key: "7" },
];

const CHAT_PAGE = 40;

/** Today / Yesterday / Previous 7 days / Previous 30 days / Month Year, like other chat apps. */
function dateGroup(ms: number): string {
  const d = new Date(ms);
  const start = new Date();
  start.setHours(0, 0, 0, 0);
  const day = 86_400_000;
  if (ms >= start.getTime()) return "Today";
  if (ms >= start.getTime() - day) return "Yesterday";
  if (ms >= start.getTime() - 7 * day) return "Previous 7 days";
  if (ms >= start.getTime() - 30 * day) return "Previous 30 days";
  return d.toLocaleDateString([], { month: "long", year: "numeric" });
}

export default function App() {
  const [settings, setSettingsState] = useState<Settings | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [models, setModels] = useState<Model[]>([]);
  const [view, setView] = useState<View>("chat");
  const [settingsSection, setSettingsSection] = useState<string | undefined>();
  const [convs, setConvs] = useState<ConvSummary[]>([]);
  const [convId, setConvId] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [agent, setAgent] = useState<AgentState | null>(null);
  const [agentRunning, setAgentRunning] = useState(false);
  const [toolPrompt, setToolPrompt] = useState<Prompt | null>(null);
  const [usage, setUsage] = useState({ today: 0, total: 0 });
  const [selecting, setSelecting] = useState(false);
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [anchor, setAnchor] = useState<string | null>(null);
  const [bulkDel, setBulkDel] = useState(false);
  const [bulkFolder, setBulkFolder] = useState(false);
  useLiveBusy(); // re-render the sidebar when a chat starts or stops replying
  const [palette, setPalette] = useState(false);
  const [playbooks, setPlaybooks] = useState<{ id: string; name: string; goal: string }[]>([]);
  const [shownChats, setShownChats] = useState(CHAT_PAGE);
  const [looseTotal, setLooseTotal] = useState(0);
  const [renaming, setRenaming] = useState<ConvSummary | null>(null);
  const [convsLoaded, setConvsLoaded] = useState(false);

  const toast = useCallback((text: string, kind: Toast["kind"] = "info", action?: ToastAction) => {
    const id = Date.now() + Math.random();
    const dismiss = () => setToasts((t) => t.filter((x) => x.id !== id));
    const act = action && { label: action.label, run: () => { dismiss(); action.run(); } };
    setToasts((t) => [...t.slice(-3), { id, text, kind, action: act }]);
    setTimeout(dismiss, action ? UNDO_MS : kind === "bad" ? 7000 : 4000);
  }, []);

  const refreshModels = useCallback(async () => setModels(await api.listModels()), []);
  // Chats deleted moments ago stay hidden until their Undo window ends, then are removed for good.
  const pendingDelete = useRef(new Map<string, number>());
  // Only the chats on screen are loaded; "Show more" asks the engine for the next page.
  const refreshConvs = useCallback(async () => {
    const r = await api.listConversations(query, shownChats);
    setConvs(r.items.filter((c) => !pendingDelete.current.has(c.id)));
    setLooseTotal(Math.max(0, r.loose_total - pendingDelete.current.size));
    setConvsLoaded(true);
  }, [query, shownChats]);
  const deleteChat = useCallback((c: { id: string; title: string }) => {
    setConvs((cs) => cs.filter((x) => x.id !== c.id));
    setConvId((cur) => (cur === c.id ? null : cur));
    const timer = window.setTimeout(async () => {
      pendingDelete.current.delete(c.id);
      try { await api.deleteConversation(c.id); } catch (e) { toast(errText(e), "bad"); }
      refreshConvs();
    }, UNDO_MS);
    pendingDelete.current.set(c.id, timer);
    toast(`Deleted “${c.title}”`, "ok", { label: "Undo", run: () => { clearTimeout(timer); pendingDelete.current.delete(c.id); refreshConvs(); } });
  }, [toast, refreshConvs]);
  // Closing the window during an Undo window still completes the deletion.
  useEffect(() => {
    const flush = () => pendingDelete.current.forEach((t, id) => { clearTimeout(t); api.deleteConversation(id); });
    window.addEventListener("beforeunload", flush);
    return () => window.removeEventListener("beforeunload", flush);
  }, []);

  const setSettings = useCallback(
    async (s: Settings) => {
      try {
        setSettingsState(await api.saveSettings(s));
        return true;
      } catch (e) {
        const { fields, message } = parseEngineErrors(e);
        toast(message ?? Object.entries(fields).map(([f, m]) => `${f}: ${m}`).join(" · "), "bad");
        return false;
      }
    },
    [toast],
  );

  const loadSettings = useCallback(() => {
    setLoadError(null);
    api.getSettings().then(setSettingsState).catch((e) => setLoadError(errText(e)));
  }, []);

  useEffect(() => {
    loadSettings();
    refreshModels().catch(() => {});
    api.usage().then(setUsage).catch(() => {});
    api.listPlaybooks().then(setPlaybooks).catch(() => {});
    api.agentStatus().then((s) => {
      setAgentRunning(s.running);
      if (s.running && s.status) setAgent(s.status);
    }).catch(() => {});
    const offs = [
      on<AgentState>("agent-state", (s) => {
        setAgent(s);
        setAgentRunning(!["done", "failed", "stopped"].includes(s.status));
      }),
      on<{ status: string; summary: string }>("agent-finished", (f) => {
        setAgentRunning(false);
        toast(`${f.status === "done" ? "Task finished" : f.status === "stopped" ? "Task stopped" : "Task could not finish"}: ${f.summary}`, f.status === "done" ? "ok" : "bad");
        api.usage().then(setUsage);
      }),
      on<string>("agent-error", (m) => toast(m, "bad")),
      on<Prompt>("chat-permission", (p) => setToolPrompt(p)),
      on("chat-done", () => api.usage().then(setUsage)),
      on<Settings>("settings-changed", (s) => setSettingsState(s)),
    ];
    return () => offs.forEach((f) => f());
  }, [refreshModels, toast, loadSettings]);

  useEffect(() => {
    const t = setTimeout(() => refreshConvs().catch(() => {}), 150);
    return () => clearTimeout(t);
  }, [refreshConvs]);
  // A new filter starts again from the first page.
  useEffect(() => setShownChats(CHAT_PAGE), [query]);

  useEffect(() => {
    if (!settings) return;
    applyTheme(settings);
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const h = () => applyTheme(settings);
    mq.addEventListener("change", h);
    return () => mq.removeEventListener("change", h);
  }, [settings]);

  const newChat = useCallback(async () => {
    const id = await api.createConversation(settings?.default_model ?? null);
    await refreshConvs();
    setConvId(id);
    setView("chat");
  }, [settings?.default_model, refreshConvs]);

  const go = useCallback((v: View, section?: string) => {
    setSettingsSection(section);
    setView(v);
  }, []);

  const toggleSidebar = useCallback(() => {
    if (settings) setSettings({ ...settings, sidebar_collapsed: !settings.sidebar_collapsed });
  }, [settings, setSettings]);

  // Global keyboard shortcuts.
  useEffect(() => {
    const h = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey)) return;
      const k = e.key.toLowerCase();
      if (k === "k") { e.preventDefault(); setPalette((p) => !p); }
      else if (k === "n" && !e.shiftKey) { e.preventDefault(); newChat(); }
      else if (k === "b") { e.preventDefault(); toggleSidebar(); }
      else if (k === ",") { e.preventDefault(); go("settings"); }
      else {
        const n = NAV.find((x) => x.key === e.key);
        if (n && e.altKey) { e.preventDefault(); go(n.id); }
      }
    };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  }, [newChat, toggleSidebar, go]);

  const privacy = useMemo(() => {
    if (!settings) return { local: true, names: [] as string[] };
    const ids = [settings.default_model, settings.roles.doer, settings.roles.planner, settings.roles.eyes].filter(Boolean) as string[];
    const cloud = models.filter((m) => ids.includes(m.id) && !m.local);
    return { local: settings.local_only || cloud.length === 0, names: [...new Set(cloud.map((m) => m.provider_name))] };
  }, [settings, models]);

  const commands = useMemo<Command[]>(() => {
    if (!settings) return [];
    const c: Command[] = [
      { id: "new", label: "New chat", group: "Actions", icon: Plus, hint: "Ctrl N", run: newChat },
      ...NAV.map((n) => ({ id: "go-" + n.id, label: `Go to ${n.label}`, group: "Pages", icon: n.icon, hint: `Ctrl Alt ${n.key}`, run: () => go(n.id) })),
      { id: "theme", label: settings.theme === "dark" ? "Switch to light theme" : "Switch to dark theme", group: "Actions", icon: settings.theme === "dark" ? Sun : Moon, keywords: "appearance mode", run: () => setSettings({ ...settings, theme: settings.theme === "dark" ? "light" : "dark" }) },
      { id: "local", label: settings.local_only ? "Turn off local-only mode" : "Turn on local-only mode", group: "Actions", icon: Lock, keywords: "privacy offline", run: () => setSettings({ ...settings, local_only: !settings.local_only }) },
      { id: "sidebar", label: "Toggle sidebar", group: "Actions", icon: Folder, hint: "Ctrl B", run: toggleSidebar },
      { id: "add-model", label: "Add a model provider", group: "Actions", icon: Cpu, keywords: "openai anthropic ollama key", run: () => go("models") },
      { id: "add-mcp", label: "Add an MCP server", group: "Actions", icon: Blocks, keywords: "tools integration", run: () => go("extensions") },
      { id: "audit", label: "Open the audit log", group: "Pages", icon: ScrollText, keywords: "history record trail compliance", run: () => go("audit") },
      ...[["agent", "Agent behaviour", ShieldCheck], ["appearance", "Appearance and personalization", Palette], ["permissions", "Permissions", Lock], ["privacy", "Privacy and data", Lock], ["shortcuts", "Shortcuts", SettingsIcon]].map(([id, label, icon]) => ({
        id: "set-" + id, label: `Settings: ${label}`, group: "Settings", icon: icon as typeof Lock, run: () => go("settings", id as string),
      })),
      ...playbooks.map((p) => ({ id: "pb-" + p.id, label: `Run playbook: ${p.name}`, group: "Playbooks", icon: Play, keywords: p.goal, run: () => api.startTask(p.goal, null).then(() => toast("Task started", "ok")).catch((e) => toast(errText(e), "bad")) })),
      ...convs.slice(0, 40).map((cv) => ({ id: "c-" + cv.id, label: cv.title, group: "Chats", icon: MessageSquare, run: () => { setConvId(cv.id); setView("chat"); } })),
    ];
    if (agentRunning) c.unshift({ id: "stop", label: "Stop the agent now", group: "Actions", icon: Square, hint: settings.stop_shortcut, run: () => api.agentStop() });
    return c;
  }, [settings, convs, playbooks, agentRunning, newChat, go, toggleSidebar, setSettings, toast]);

  if (loadError) {
    return (
      <div className="error-screen card">
        <h2>Could not start</h2>
        <pre className="code-block mono">{loadError}</pre>
        <div className="row end" style={{ marginTop: 12 }}><button className="btn primary" onClick={loadSettings}>Retry</button></div>
      </div>
    );
  }
  if (!settings) return <div className="empty" style={{ height: "100%" }}><Spinner size={22} /> Starting…</div>;

  const ctx = { settings, setSettings, models, refreshModels, toast };

  if (!settings.onboarded) {
    return (
      <Ctx.Provider value={ctx}>
        <div className="frame">
          <TitleBar name={settings.app_name} onSearch={() => {}} onToggleSidebar={() => {}} />
          <div className="page">
            <Onboarding onDone={async () => { const fresh = await api.getSettings(); await setSettings({ ...fresh, onboarded: true }); }} />
          </div>
        </div>
        <Toasts toasts={toasts} />
      </Ctx.Provider>
    );
  }

  const pinned = convs.filter((c) => c.pinned);
  const folders = [...new Set(convs.filter((c) => !c.pinned && c.folder).map((c) => c.folder))].sort();
  const loose = convs.filter((c) => !c.pinned && !c.folder);
  const looseShown = loose.slice(0, shownChats);
  const groups: [string, ConvSummary[]][] = [];
  for (const c of looseShown) {
    const g = dateGroup(c.updated);
    const last = groups[groups.length - 1];
    if (last && last[0] === g) last[1].push(c);
    else groups.push([g, [c]]);
  }
  const open = (id: string) => { setConvId(id); setView("chat"); };
  // Order the sidebar shows, for Shift-click ranges.
  const ordered = [...pinned, ...folders.flatMap((f) => convs.filter((c) => !c.pinned && c.folder === f)), ...looseShown];
  const pick = (c: ConvSummary, e: { shiftKey: boolean }) => {
    setPicked((s) => {
      const n = new Set(s);
      if (e.shiftKey && anchor) {
        const a = ordered.findIndex((x) => x.id === anchor);
        const b = ordered.findIndex((x) => x.id === c.id);
        if (a >= 0 && b >= 0) ordered.slice(Math.min(a, b), Math.max(a, b) + 1).forEach((x) => n.add(x.id));
      } else if (n.has(c.id)) n.delete(c.id);
      else n.add(c.id);
      return n;
    });
    setAnchor(c.id);
  };
  const endSelect = () => { setSelecting(false); setPicked(new Set()); setAnchor(null); };
  const bulk = async (fn: (id: string) => Promise<unknown>, done: string) => {
    try {
      for (const id of picked) await fn(id);
      await refreshConvs();
      toast(done, "ok");
    } catch (e) {
      toast(errText(e), "bad");
    }
  };
  const exportPicked = async () => {
    try {
      const parts = [];
      for (const id of picked) parts.push(toMarkdown(await api.getConversation(id), settings.assistant_name));
      const path = await saveExport(`${picked.size} chats`, "md", parts.join("\n\n---\n\n"));
      if (path) toast(`Saved to ${path}`, "ok");
    } catch (e) {
      toast(errText(e), "bad");
    }
  };
  const forkChat = async (c: ConvSummary) => {
    try { const id = await api.forkConversation(c.id); await refreshConvs(); open(id); toast("Forked into a new chat", "ok"); } catch (e) { toast(errText(e), "bad"); }
  };

  const item = (c: ConvSummary) => (
    <div key={c.id} className={"conv-item " + (c.id === convId && view === "chat" && !selecting ? "active" : "") + (picked.has(c.id) ? " picked" : "")} title={c.title}
      onClick={(e) => {
        if (selecting || e.ctrlKey || e.metaKey || e.shiftKey) { e.preventDefault(); if (!selecting) setSelecting(true); pick(c, e); }
        else open(c.id);
      }}
      role="button" tabIndex={0} aria-pressed={selecting ? picked.has(c.id) : undefined} onKeyDown={(e) => { if (e.key === "Enter" || (selecting && e.key === " ")) { e.preventDefault(); if (selecting) pick(c, e); else open(c.id); } }}>
      {selecting && <span onClick={(e) => e.stopPropagation()}><Checkbox checked={picked.has(c.id)} onChange={() => pick(c, { shiftKey: false })} /></span>}
      <span className="t">{c.title}</span>
      {live.busy(c.id) && <span className="busy-dot" title="Replying…"><Spinner size={12} /></span>}
      <span className="acts" style={selecting ? { display: "none" } : undefined}>
        <Menu label={`Actions for ${c.title}`} trigger={() => <button className="icon-btn" aria-label="Chat actions" title="More"><MoreHorizontal size={15} /></button>}
          items={[
            { label: "Rename", icon: Pencil, onClick: () => setRenaming(c) },
            { label: c.pinned ? "Unpin" : "Pin to top", icon: c.pinned ? PinOff : Pin, onClick: () => api.updateConversation(c.id, { pinned: !c.pinned }).then(refreshConvs) },
            { label: "Fork", icon: GitFork, onClick: () => forkChat(c) },
            "sep",
            { label: "Delete", icon: Trash2, danger: true, onClick: () => deleteChat(c) },
          ]} />
      </span>
    </div>
  );

  return (
    <Ctx.Provider value={ctx}>
      <div className="frame">
        <TitleBar name={settings.app_name} onSearch={() => setPalette(true)} onToggleSidebar={toggleSidebar} />
        <div className={"app" + (settings.sidebar_collapsed ? " collapsed" : "")}>
          <aside className="sidebar">
            <div className="side-top">
              <div style={{ padding: "12px 12px 8px" }}>
                <button className="btn primary block new-chat" onClick={newChat} title="New chat (Ctrl+N)"><Plus size={16} /><span className="label"> New chat</span></button>
              </div>
              <nav className="nav" aria-label="Main">
                {NAV.map((n) => (
                  <button key={n.id} className={"nav-item " + (view === n.id ? "active" : "")} onClick={() => go(n.id)} aria-current={view === n.id ? "page" : undefined} title={n.label}>
                    <n.icon size={17} className="icon" /> <span className="label">{n.label}</span>
                    {n.id === "models" && models.length > 0 && <span className="count">{models.length}</span>}
                  </button>
                ))}
              </nav>
              <div className="side-section row" style={{ justifyContent: "space-between" }}>
                <span>Chats</span>
                {convs.length > 0 && (
                  <button className={"icon-btn" + (selecting ? " active" : "")} style={{ width: 24, height: 24 }} onClick={() => (selecting ? endSelect() : setSelecting(true))}
                    title={selecting ? "Done selecting" : "Select chats (or Ctrl+click, Shift+click)"} aria-label={selecting ? "Done selecting" : "Select chats"}>
                    {selecting ? <X size={14} /> : <CheckSquare size={14} />}
                  </button>
                )}
              </div>
              {selecting && (
                <div className="side-bulk">
                  <div className="row" style={{ justifyContent: "space-between" }}>
                    <b>{picked.size} selected</b>
                    <span className="row" style={{ gap: 4 }}>
                      <button className="btn sm subtle" onClick={() => setPicked(new Set(ordered.map((c) => c.id)))}>All</button>
                      <button className="btn sm subtle" onClick={() => setPicked(new Set())} disabled={!picked.size}>None</button>
                    </span>
                  </div>
                  <div className="row" style={{ gap: 4 }}>
                    <button className="icon-btn" title="Pin" aria-label="Pin selected" disabled={!picked.size} onClick={() => bulk((id) => api.updateConversation(id, { pinned: true }), "Pinned")}><Pin size={15} /></button>
                    <button className="icon-btn" title="Unpin" aria-label="Unpin selected" disabled={!picked.size} onClick={() => bulk((id) => api.updateConversation(id, { pinned: false }), "Unpinned")}><PinOff size={15} /></button>
                    <button className="icon-btn" title="Move to folder" aria-label="Move selected to a folder" disabled={!picked.size} onClick={() => setBulkFolder(true)}><Folder size={15} /></button>
                    <button className="icon-btn" title="Export as Markdown" aria-label="Export selected" disabled={!picked.size} onClick={exportPicked}><Download size={15} /></button>
                    <span className="grow" />
                    <button className="icon-btn danger" title="Delete" aria-label="Delete selected" disabled={!picked.size} onClick={() => setBulkDel(true)}><Trash2 size={15} /></button>
                  </div>
                </div>
              )}
              <div className="conv-search" style={{ padding: "0 12px 6px" }}>
                <div className="input-group">
                  <Search size={15} className="lead" />
                  <input className="input" placeholder="Filter chats" value={query} onChange={(e) => setQuery(e.target.value)} aria-label="Filter chats" />
                </div>
              </div>
              <div className="conv-list">
                {!convsLoaded && [0, 1, 2, 3].map((i) => <div key={i} className="sk-line" style={{ margin: "10px 8px", width: `${80 - i * 12}%` }} />)}
                {pinned.length > 0 && <div className="conv-group"><Pin size={12} /> Pinned</div>}
                {pinned.map(item)}
                {folders.map((f) => (
                  <div key={f}>
                    <div className="conv-group"><Folder size={12} /> {f}</div>
                    {convs.filter((c) => !c.pinned && c.folder === f).map(item)}
                  </div>
                ))}
                {groups.map(([g, list]) => (
                  <div key={g}>
                    <div className="conv-group">{g}</div>
                    {list.map(item)}
                  </div>
                ))}
                {looseTotal > loose.length && (
                  <button className="btn sm subtle block" style={{ margin: "8px 0" }} onClick={() => setShownChats((n) => n + CHAT_PAGE)}>Show {Math.min(CHAT_PAGE, looseTotal - loose.length)} more · {looseTotal - loose.length} left</button>
                )}
                {convsLoaded && convs.length === 0 && <div className="small muted" style={{ padding: "8px 10px" }}>{query ? "No chats match" : "Your chats will appear here"}</div>}
              </div>
            </div>
            <div className="side-foot">
              <div className="row" title={privacy.local ? "No text or screenshots leave this computer" : `Data is sent to: ${privacy.names.join(", ")}`}>
                <span className={"badge " + (privacy.local ? "ok" : "warn")}>
                  {privacy.local ? <Lock size={12} /> : <Cloud size={12} />}
                  <span className="hide-collapsed">{settings.local_only ? "Local only" : privacy.local ? "Local" : "Cloud active"}</span>
                </span>
                {!privacy.local && <span className="ellipsis grow hide-collapsed" style={{ textAlign: "right" }}>{privacy.names.join(", ")}</span>}
              </div>
              <div className="row hide-collapsed"><span>Spent today</span><span>{money(usage.today)} of {money(settings.daily_cost_cap)}</span></div>
              <div className="row hide-collapsed"><span>Emergency stop</span><span className="kbd">{settings.stop_shortcut}</span></div>
            </div>
          </aside>
          <main className="main">
            {!convsLoaded && <div className="topline" aria-hidden />}
        {agentRunning && agent && (
              <div className="agent-strip" role="status">
                <span className="dot pulse" />
                <Bot size={15} />
                <b>Agent</b>
                <span className="grow ellipsis">Step {agent.step} · {agent.message}</span>
                <span className="badge">{money(agent.cost)}</span>
                <button className="btn sm danger" onClick={() => api.agentStop()}><Square size={11} fill="currentColor" /> Stop</button>
              </div>
            )}
            <ErrorBoundary resetKey={view}>
              <Suspense fallback={<div className="page-loading" role="status" aria-live="polite"><Spinner size={18} /> Loading…</div>}>
              {view === "chat" && <ChatView deleteChat={deleteChat} convId={convId} setConvId={setConvId} refreshConvs={refreshConvs} agentRunning={agentRunning} goModels={() => go("models")} playbooks={playbooks} />}
              {view === "models" && <ModelsView />}
              {view === "extensions" && <ExtensionsView />}
              {view === "activity" && <ActivityView onPlaybooksChanged={() => api.listPlaybooks().then(setPlaybooks)} />}
              {view === "stats" && <StatsView />}
              {view === "audit" && <AuditView openChat={(id) => open(id)} />}
              {view === "settings" && <SettingsView initialSection={settingsSection} />}
              </Suspense>
            </ErrorBoundary>
          </main>
        </div>
      </div>
      {palette && <CommandPalette commands={commands} onClose={() => setPalette(false)} />}
      {toolPrompt && (
        <Modal
          title="Allow tool use?"
          subtitle={`The assistant wants to use “${toolPrompt.tool}”.`}
          icon={ShieldCheck}
          onClose={() => { api.agentRespond(toolPrompt.id, { decision: "deny" }); setToolPrompt(null); }}
          footer={
            <>
              <button className="btn" onClick={() => { api.agentRespond(toolPrompt.id, { decision: "deny" }); setToolPrompt(null); }}>Deny</button>
              <button className="btn primary" onClick={() => { api.agentRespond(toolPrompt.id, { decision: "approve" }); setToolPrompt(null); }}>Allow once</button>
            </>
          }
        >
          {toolPrompt.reason && <Callout kind="warn">{toolPrompt.reason}</Callout>}
          <pre className="code-block mono">{JSON.stringify(toolPrompt.args, null, 2)}</pre>
          <p className="small muted" style={{ marginBottom: 0 }}>Change the default in Extensions → Tools.</p>
        </Modal>
      )}
      {renaming && (
        <PromptDialog title="Rename chat" label="Title" initial={renaming.title} validate={(x) => (!x.trim() ? "Enter a title" : x.length > 120 ? "Up to 120 characters" : undefined)}
          onSubmit={async (x) => { await api.updateConversation(renaming.id, { title: x }); await refreshConvs(); }} onClose={() => setRenaming(null)} />
      )}
      {bulkDel && (
        <ConfirmDialog title={`Delete ${picked.size} chat${picked.size === 1 ? "" : "s"}?`} onClose={() => setBulkDel(false)} onConfirm={async () => {
          const n = picked.size;
          await bulk((id) => api.deleteConversation(id), `${n} chat${n === 1 ? "" : "s"} deleted`);
          if (convId && picked.has(convId)) setConvId(null);
          endSelect();
        }}>
          The selected chats, their messages and documents will be removed from this computer.
        </ConfirmDialog>
      )}
      {bulkFolder && (
        <PromptDialog title={`Move ${picked.size} chat${picked.size === 1 ? "" : "s"} to a folder`} label="Folder name" hint="Leave empty to take them out of folders" initial=""
          validate={(x) => (x.length > 40 ? "Up to 40 characters" : /[\\/]/.test(x) ? "No slashes" : undefined)}
          onSubmit={async (x) => { await bulk((id) => api.updateConversation(id, { folder: x }), x ? `Moved to ${x}` : "Removed from folders"); }}
          onClose={() => setBulkFolder(false)} confirmLabel="Move" />
      )}
      <Toasts toasts={toasts} />
    </Ctx.Provider>
  );
}
