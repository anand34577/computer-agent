import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import {
  Archive, Bot, Check, CircleAlert, CircleCheck, DollarSign, Download, FolderLock, FolderOpen, FolderPlus, Gauge, Info, Keyboard, Lock, Monitor, MousePointer2,
  Network, Palette, RotateCcw, Scale, Search, ShieldAlert, ShieldCheck, SlidersHorizontal, Trash2, Upload, X, Zap,
} from "lucide-react";
import { api, errText, type PlatformInfo, type Settings } from "../api";
import { Callout, Card, Checkbox, ColorField, ConfirmDialog, Field, Modal, NumberField, Seg, Select, SettingRow, Slider, TextField, ToggleRow, useApp } from "../components/common";
import { ACCENTS } from "../theme";
import * as v from "../validate";

type Section = { id: string; label: string; icon: typeof Palette; keywords: string };
const SECTIONS: Section[] = [
  { id: "general", label: "General", icon: SlidersHorizontal, keywords: "profile name assistant send enter suggestions reasoning thinking effort" },
  { id: "appearance", label: "Appearance", icon: Palette, keywords: "theme dark light contrast accent colour color radius density text size sidebar" },
  { id: "agent", label: "Agent behaviour", icon: Bot, keywords: "autonomy full access permission plan draft pointer steps minutes retries screenshot settle yield corner minimize" },
  { id: "ai", label: "Documents and sub-agents", icon: Network, keywords: "embedding documents rag pdf search subagents sub-agents delegate hybrid local models parallel" },
  { id: "cost", label: "Cost limits", icon: DollarSign, keywords: "cap budget money daily task" },
  { id: "permissions", label: "Permissions", icon: FolderLock, keywords: "folders protected allowed blocked apps sites undo" },
  { id: "privacy", label: "Privacy and data", icon: Lock, keywords: "local only mask telemetry screenshots retention wipe instructions audit log" },
  { id: "shortcuts", label: "Shortcuts", icon: Keyboard, keywords: "keyboard stop emergency summon hotkey" },
  { id: "backup", label: "Backup and reset", icon: Archive, keywords: "export import reset defaults" },
  { id: "system", label: "System", icon: Monitor, keywords: "platform permissions diagnostics version monitors" },
];

export default function SettingsView({ initialSection }: { initialSection?: string }) {
  const [section, setSection] = useState(initialSection ?? "general");
  const [q, setQ] = useState("");
  useEffect(() => {
    if (initialSection) setSection(initialSection);
  }, [initialSection]);
  const shown = useMemo(() => SECTIONS.filter((s) => !q || `${s.label} ${s.keywords}`.toLowerCase().includes(q.toLowerCase())), [q]);
  useEffect(() => {
    if (q && shown.length && !shown.some((s) => s.id === section)) setSection(shown[0].id);
  }, [q, shown]);

  return (
    <div className="page">
      <div className="page-inner">
        <div className="page-head">
          <div className="grow">
            <h1>Settings</h1>
            <p>Changes save automatically and are checked before they apply.</p>
          </div>
        </div>
        <div className="settings-layout">
          <nav className="settings-nav" aria-label="Settings sections">
            <div className="input-group" style={{ marginBottom: 8 }}><Search size={15} className="lead" /><input className="input" placeholder="Search settings" value={q} onChange={(e) => setQ(e.target.value)} aria-label="Search settings" /></div>
            {shown.map((s) => <button key={s.id} className={section === s.id ? "on" : ""} onClick={() => setSection(s.id)}><s.icon size={16} /> {s.label}</button>)}
            {shown.length === 0 && <div className="small muted" style={{ padding: 8 }}>No settings match</div>}
          </nav>
          <div>
            {section === "general" && <General />}
            {section === "appearance" && <Appearance />}
            {section === "agent" && <AgentSection />}
            {section === "ai" && <AiSection />}
            {section === "cost" && <Cost />}
            {section === "permissions" && <Permissions />}
            {section === "privacy" && <Privacy />}
            {section === "shortcuts" && <Shortcuts />}
            {section === "backup" && <Backup />}
            {section === "system" && <System />}
          </div>
        </div>
      </div>
    </div>
  );
}

/** Saves a patch on top of the latest settings. */
function usePatch() {
  const { settings, setSettings } = useApp();
  return (patch: Partial<Settings>) => setSettings({ ...settings, ...patch });
}

function General() {
  const { settings, toast } = useApp();
  const patch = usePatch();
  const [name, setName] = useState(settings.app_name);
  const [assistant, setAssistant] = useState(settings.assistant_name);
  const nameErr = v.required(name, "A name") ?? v.maxLen(name, 40);
  const asstErr = v.required(assistant, "A name") ?? v.maxLen(assistant, 30);
  const profiles = [
    { id: "local", icon: Lock, t: "Private and local", d: "Blocks every cloud call. Local models only." },
    { id: "balanced", icon: Scale, t: "Balanced assistant", d: "Plans first, asks before risky steps, draft-only sending." },
    { id: "power", icon: Zap, t: "Maximum power", d: "Autonomous inside allowed apps and folders. Fixed gates still apply." },
  ];
  return (
    <>
      <Card title="Quick profiles" subtitle="One click sets a group of options. You can fine-tune anything afterwards.">
        <div className="grid3">
          {profiles.map((p) => (
            <button key={p.id} className="choice" onClick={async () => { await api.applyProfile(p.id); toast(`Applied “${p.t}”`, "ok"); }}>
              <div className="choice-icon"><p.icon size={17} /></div>
              <b>{p.t}</b>
              <small>{p.d}</small>
            </button>
          ))}
        </div>
      </Card>
      <Card title="Make it yours">
        <div className="grid2">
          <TextField label="App name" hint="Shown in the title bar." value={name} onChange={setName} error={nameErr} onBlur={() => !nameErr && name.trim() !== settings.app_name && patch({ app_name: name.trim() })} />
          <TextField label="Assistant name" hint="How the assistant is named in chats." value={assistant} onChange={setAssistant} error={asstErr} onBlur={() => !asstErr && assistant.trim() !== settings.assistant_name && patch({ assistant_name: assistant.trim() })} />
        </div>
      </Card>
      <Card title="Chat">
        <ToggleRow label="Send with Enter" hint="Off: Ctrl+Enter sends and Enter adds a new line." checked={settings.send_on_enter} onChange={(x) => patch({ send_on_enter: x })} />
        <ToggleRow label="Show suggestions on new chats" checked={settings.show_suggestions} onChange={(x) => patch({ show_suggestions: x })} />
        <ToggleRow label="Show the model's reasoning" hint="While a reasoning model thinks, its thoughts stream into a fold-out block above the answer." checked={settings.show_reasoning} onChange={(x) => patch({ show_reasoning: x })} />
        <SettingRow label="Reasoning effort" hint="For models that can reason (o-series, Claude, DeepSeek-R1, Qwen3…). Automatic leaves the provider's default.">
          <Select width={190} label="Reasoning effort" value={settings.reasoning} onChange={(x) => patch({ reasoning: x })}
            options={[{ value: "auto", label: "Automatic" }, { value: "off", label: "Off (fastest)" }, { value: "low", label: "Low" }, { value: "medium", label: "Medium" }, { value: "high", label: "High (slowest)" }]} />
        </SettingRow>
        <InstructionsField />
      </Card>
    </>
  );
}

function InstructionsField() {
  const { settings } = useApp();
  const patch = usePatch();
  const [text, setText] = useState(settings.system_prompt);
  const err = text.length > 8000 ? "Keep instructions under 8,000 characters" : undefined;
  return (
    <div style={{ paddingTop: 14, borderTop: "1px solid var(--line)" }}>
      <Field label="Instructions for every chat" hint={`${text.length.toLocaleString()} / 8,000 characters`} error={err}>
        <textarea className={"input" + (err ? " invalid" : "")} rows={4} value={text} onChange={(e) => setText(e.target.value)} onBlur={() => !err && text !== settings.system_prompt && patch({ system_prompt: text })}
          placeholder="e.g. Keep answers short. I use Windows 11 and Excel." />
      </Field>
    </div>
  );
}

function Appearance() {
  const { settings } = useApp();
  const patch = usePatch();
  const [custom, setCustom] = useState(settings.accent);
  const customErr = v.hexColor(custom);
  return (
    <Card title="Appearance">
      <SettingRow label="Theme">
        <Seg value={settings.theme} onChange={(x) => patch({ theme: x })} options={[{ value: "system", label: "System" }, { value: "light", label: "Light" }, { value: "dark", label: "Dark" }, { value: "contrast", label: "High contrast" }]} />
      </SettingRow>
      <SettingRow label="Accent colour" hint="Used for buttons, highlights and focus rings.">
        <div className="swatches">
          {ACCENTS.map((c) => (
            <button key={c} className={"swatch " + (settings.accent.toLowerCase() === c ? "on" : "")} style={{ background: c }} aria-label={`Accent ${c}`} onClick={() => { setCustom(c); patch({ accent: c }); }}>
              {settings.accent.toLowerCase() === c && <Check size={14} />}
            </button>
          ))}
          <ColorField label="Custom accent colour" value={customErr ? settings.accent : custom} onChange={(c) => { setCustom(c); patch({ accent: c }); }} />
        </div>
      </SettingRow>
      <SettingRow label="Corners">
        <Seg value={settings.radius} onChange={(x) => patch({ radius: x })} options={[{ value: "sharp", label: "Sharp" }, { value: "rounded", label: "Rounded" }, { value: "round", label: "Round" }]} />
      </SettingRow>
      <SettingRow label="Density">
        <Seg value={settings.density} onChange={(x) => patch({ density: x })} options={[{ value: "comfortable", label: "Comfortable" }, { value: "compact", label: "Compact" }]} />
      </SettingRow>
      <SettingRow label="Text size" hint="Applies everywhere in the app.">
        <Slider label="Text size" min={12} max={20} value={settings.text_size} onChange={(n) => n !== settings.text_size && patch({ text_size: n })} format={(n) => `${n}px`} />
      </SettingRow>
      <ToggleRow label="Collapse the sidebar" hint="Toggle any time with Ctrl+B." checked={settings.sidebar_collapsed} onChange={(x) => patch({ sidebar_collapsed: x })} />
    </Card>
  );
}

function AgentSection() {
  const { settings } = useApp();
  const patch = usePatch();
  const level = settings.layer;
  const [confirmFull, setConfirmFull] = useState(false);
  return (
    <>
      <Card title="Autonomy" subtitle="How much the agent and chat tools may do without asking.">
        <Seg value={settings.autonomy} onChange={(x) => (x === "full" && settings.autonomy !== "full" ? setConfirmFull(true) : patch({ autonomy: x }))}
          options={[{ value: "supervised", label: "Supervised" }, { value: "balanced", label: "Balanced" }, { value: "autonomous", label: "Autonomous" }, { value: "full", label: "Full access" }]} />
        <div style={{ marginTop: 12 }}>
          {settings.autonomy === "supervised" && <Callout kind="info">You approve every action.</Callout>}
          {settings.autonomy === "balanced" && <Callout kind="info">Routine steps run; risky ones ask first.</Callout>}
          {settings.autonomy === "autonomous" && <Callout kind="warn">Runs end to end inside allowed apps and folders. It still stops at the fixed gates: sending, paying, deleting outside undo, system settings, terminals, installing and passwords.</Callout>}
          {settings.autonomy === "full" && (
            <Callout kind="bad" action={<button className="btn sm" onClick={() => patch({ autonomy: "balanced" })}>Turn off</button>}>
              <b>Full access is on.</b> The agent skips plan approval and every confirmation, including sending, deleting, installing, system settings and shell commands; chat tools run without asking. Passwords, sign-in screens, CAPTCHAs and off-limits apps still hand control back to you. Stop everything with <span className="kbd">{settings.stop_shortcut}</span>.
            </Callout>
          )}
        </div>
      </Card>
      {confirmFull && (
        <ConfirmDialog title="Give the assistant full access?" confirmLabel="Turn on full access" onClose={() => setConfirmFull(false)} onConfirm={async () => { await patch({ autonomy: "full" }); }}>
          <p style={{ marginTop: 0 }}><ShieldAlert size={15} style={{ verticalAlign: -2, color: "var(--danger)" }} /> With full access, the agent and chat tools act on their own: no plan approval, no confirmation before clicking send, deleting, installing, changing settings, running shell commands or stopping programs.</p>
          <p>A wrong or manipulated instruction (for example, hidden text on a web page) can then do real damage. Only use it with a model you trust, for tasks you can watch. Everything is still recorded in the Audit log.</p>
        </ConfirmDialog>
      )}
      <Card title="Behaviour">
        <ToggleRow label="Show the plan before starting" hint="You can edit or cancel it." checked={settings.plan_first} onChange={(x) => patch({ plan_first: x })} />
        <ToggleRow label="Draft-only for send, post, submit and pay" hint="The agent prepares everything; you press the final button." checked={settings.draft_only} onChange={(x) => patch({ draft_only: x })} />
        <ToggleRow label="Shrink to a floating bar while working" checked={settings.minimize_on_run} onChange={(x) => patch({ minimize_on_run: x })} />
        <ToggleRow label="Pause when I move the mouse" hint="Resume with one click on the bar." checked={settings.yield_to_user} onChange={(x) => patch({ yield_to_user: x })} />
        <ToggleRow label="Pause when the pointer hits the top-left corner" checked={settings.corner_pause} onChange={(x) => patch({ corner_pause: x })} />
      </Card>
      <Card title="Limits and timing" actions={<Seg value={level} onChange={(x) => patch({ layer: x })} options={[{ value: "basic", label: "Basic" }, { value: "advanced", label: "Advanced" }, { value: "expert", label: "Expert" }]} />}>
        <div className="grid3">
          <NumberField label="Max steps per task" value={settings.max_steps} onCommit={(n) => patch({ max_steps: n })} min={1} max={500} />
          <NumberField label="Max minutes per task" value={settings.max_task_minutes} onCommit={(n) => patch({ max_task_minutes: n })} min={1} max={240} />
          <NumberField label="Pointer highlight (ms)" value={settings.pointer_delay_ms} onCommit={(n) => patch({ pointer_delay_ms: n })} min={0} max={3000} hint="Pause before each click" />
          {level !== "basic" && <NumberField label="Retries before an alternative" value={settings.max_retries} onCommit={(n) => patch({ max_retries: n })} min={0} max={10} />}
          {level === "expert" && <NumberField label="Screenshot width (px)" value={settings.screenshot_width} onCommit={(n) => patch({ screenshot_width: n })} min={640} max={3840} hint="Smaller is cheaper and faster" />}
          {level === "expert" && <NumberField label="Wait for screen to settle (ms)" value={settings.settle_ms} onCommit={(n) => patch({ settle_ms: n })} min={0} max={10000} />}
        </div>
        {level === "basic" && <div className="field-hint" style={{ marginTop: 10 }}>Switch to Advanced or Expert for more options.</div>}
      </Card>
    </>
  );
}

function Cost() {
  const { settings } = useApp();
  const patch = usePatch();
  return (
    <Card title="Cost limits" subtitle="Hitting a cap pauses the task and asks you; it never fails silently. Local models cost nothing. 0 means no cap.">
      <div className="grid2">
        <NumberField label="Per-task cap (USD)" step={0.1} value={settings.task_cost_cap} onCommit={(n) => patch({ task_cost_cap: n })} min={0} max={10000}
          validate={(n) => (settings.daily_cost_cap > 0 && n > settings.daily_cost_cap ? "Cannot exceed the daily cap" : undefined)} />
        <NumberField label="Daily cap (USD)" step={0.5} value={settings.daily_cost_cap} onCommit={(n) => patch({ daily_cost_cap: n })} min={0} max={10000}
          validate={(n) => (n > 0 && n < settings.task_cost_cap ? "Must be at least the per-task cap" : undefined)} />
      </div>
    </Card>
  );
}

function FolderList({ label, hint, values, onChange }: { label: string; hint: string; values: string[]; onChange: (v: string[]) => void }) {
  const { toast } = useApp();
  const [typed, setTyped] = useState("");
  const typedErr = typed ? v.absolutePath(typed) ?? (values.some((x) => x.toLowerCase() === typed.trim().toLowerCase()) ? "Already in the list" : undefined) : undefined;
  const add = (p: string) => {
    const clean = p.trim();
    if (!clean || v.absolutePath(clean)) return;
    if (values.some((x) => x.toLowerCase() === clean.toLowerCase())) return toast("Already in the list", "info");
    onChange([...values, clean]);
  };
  return (
    <Field label={label} hint={hint}>
      <div style={{ border: "1px solid var(--line)", borderRadius: "var(--r)", overflow: "hidden" }}>
        {values.length === 0 && <div className="small muted" style={{ padding: 12 }}>Nothing added</div>}
        {values.map((p) => (
          <div key={p} className="row" style={{ padding: "6px 6px 6px 12px", borderBottom: "1px solid var(--line)" }}>
            <span className="grow mono ellipsis" title={p}>{p}</span>
            <button className="icon-btn danger" aria-label={`Remove ${p}`} onClick={() => onChange(values.filter((x) => x !== p))}><X size={14} /></button>
          </div>
        ))}
        <form className="row" style={{ padding: 6 }} onSubmit={(e) => { e.preventDefault(); if (!typedErr) { add(typed); setTyped(""); } }}>
          <input className={"input mono" + (typedErr ? " invalid" : "")} value={typed} onChange={(e) => setTyped(e.target.value)} placeholder="Type a full path and press Enter" aria-label={`Add to ${label}`} />
          <button type="button" className="btn sm" onClick={async () => { const d = await open({ directory: true }); if (typeof d === "string") add(d); }}><FolderPlus size={14} /> Browse</button>
        </form>
      </div>
      {typedErr && <div className="field-error">{typedErr}</div>}
    </Field>
  );
}

function TextList({ label, hint, values, onChange, placeholder }: { label: string; hint: string; values: string[]; onChange: (v: string[]) => void; placeholder: string }) {
  const [typed, setTyped] = useState("");
  const err = typed.trim().length > 80 ? "Too long" : values.some((x) => x.toLowerCase() === typed.trim().toLowerCase()) && typed ? "Already in the list" : undefined;
  return (
    <Field label={label} hint={hint}>
      <div className="row wrap" style={{ gap: 6, padding: 8, border: "1px solid var(--line)", borderRadius: "var(--r)" }}>
        {values.map((x) => <span key={x} className="badge" style={{ height: 26 }}>{x}<button className="icon-btn" style={{ width: 18, height: 18 }} aria-label={`Remove ${x}`} onClick={() => onChange(values.filter((y) => y !== x))}><X size={12} /></button></span>)}
        <form onSubmit={(e) => { e.preventDefault(); if (typed.trim() && !err) { onChange([...values, typed.trim()]); setTyped(""); } }} style={{ flex: 1, minWidth: 160 }}>
          <input className={"input" + (err ? " invalid" : "")} style={{ height: 28 }} value={typed} onChange={(e) => setTyped(e.target.value)} placeholder={placeholder} aria-label={`Add to ${label}`} />
        </form>
      </div>
      {err && <div className="field-error">{err}</div>}
    </Field>
  );
}

function Permissions() {
  const { settings } = useApp();
  const patch = usePatch();
  return (
    <>
      <Card title="Folders" subtitle="Where the agent's file tools may make changes. Protected folders always win.">
        <div className="stack">
          <FolderList label="Allowed folders" hint="The agent can move, rename and write files only inside these." values={settings.allowed_folders} onChange={(x) => patch({ allowed_folders: x })} />
          <FolderList label="Protected folders" hint="Never changed by the agent." values={settings.protected_folders} onChange={(x) => patch({ protected_folders: x })} />
          <NumberField label="Undo window for file changes" suffix="hours" value={settings.undo_hours} onCommit={(n) => patch({ undo_hours: n })} min={1} max={720} />
        </div>
      </Card>
      <Card title="Blocked apps and sites" subtitle="When one of these is in front, the agent stops and hands control to you.">
        <div className="stack">
          <TextList label="Blocked apps" hint="Matched against window and app names. Press Enter to add." values={settings.blocked_apps} onChange={(x) => patch({ blocked_apps: x })} placeholder="e.g. keepass" />
          <TextList label="Blocked sites" hint="Matched against links and browser window titles." values={settings.blocked_sites} onChange={(x) => patch({ blocked_sites: x })} placeholder="e.g. mybank.com" />
        </div>
      </Card>
    </>
  );
}

function Privacy() {
  const { settings, toast } = useApp();
  const patch = usePatch();
  const [diag, setDiag] = useState<string | null>(null);
  const [wipe, setWipe] = useState(false);
  return (
    <>
      <Card title="Privacy">
        <ToggleRow label="Local-only mode" hint="Blocks every model and network call to non-local addresses, including web search and remote MCP servers. For proof, pair it with your OS firewall." checked={settings.local_only} onChange={(x) => patch({ local_only: x })} />
        <ToggleRow label="Mask password fields in screenshots" hint="Best effort: unusual layouts can be missed. Use local models for sensitive work." checked={settings.mask_secrets} onChange={(x) => patch({ mask_secrets: x })} />
        <ToggleRow label="Share anonymous usage data" hint="Off by default. This build sends none." checked={settings.telemetry} onChange={(x) => patch({ telemetry: x })} />
        <div style={{ paddingTop: 14, borderTop: "1px solid var(--line)" }}>
          <div className="grid2">
            <NumberField label="Keep agent screenshots for" suffix="days" value={settings.keep_screenshots_days} onCommit={(n) => patch({ keep_screenshots_days: n })} min={1} max={365} />
            <NumberField label="Keep audit records for" hint="0 keeps them forever. Older records are removed at startup." value={settings.audit_retention_days} onCommit={(n) => patch({ audit_retention_days: n })} min={0} max={3650} />
          </div>
        </div>
      </Card>
      <Card title="Your data">
        <div className="row wrap">
          <button className="btn" onClick={async () => setDiag(await api.diagnostics())}><Info size={15} /> Diagnostics report</button>
          <button className="btn" onClick={async () => api.reveal(await api.dataDir())}><FolderOpen size={15} /> Open data folder</button>
          <button className="btn danger" onClick={() => setWipe(true)}><Trash2 size={15} /> Wipe all data</button>
        </div>
      </Card>
      {diag && (
        <Modal title="Diagnostics report" subtitle="Read it before sharing. It never contains screenshots, keys or chats." icon={Info} onClose={() => setDiag(null)} wide
          footer={<><button className="btn" onClick={() => { navigator.clipboard.writeText(diag); toast("Copied", "ok"); }}>Copy</button><button className="btn primary" onClick={() => setDiag(null)}>Close</button></>}>
          <pre className="code-block mono">{diag}</pre>
        </Modal>
      )}
      {wipe && (
        <ConfirmDialog title="Wipe all data?" confirmLabel="Wipe everything" onClose={() => setWipe(false)} onConfirm={async () => { try { await api.wipeAll(); toast("All data wiped", "ok"); } catch (e) { toast(errText(e), "bad"); } }}>
          This deletes every chat, task log, screenshot and the undo area. Models, providers and settings are kept. This cannot be undone.
        </ConfirmDialog>
      )}
    </>
  );
}

function ShortcutField({ label, hint, value, required, other, onSave }: { label: string; hint: string; value: string; required?: boolean; other: string; onSave: (v: string) => void }) {
  const [val, setVal] = useState(value);
  const [recording, setRecording] = useState(false);
  useEffect(() => setVal(value), [value]);
  const err = v.shortcut(val, required) ?? (val && val.toLowerCase() === other.toLowerCase() ? "Already used by another shortcut" : undefined);
  return (
    <Field label={label} hint={recording ? "Press the key combination now…" : hint} error={err}>
      <div className="row">
        <input className={"input mono" + (err ? " invalid" : "")} value={val} readOnly={recording} onChange={(e) => setVal(e.target.value)} onBlur={() => { setRecording(false); if (!err && val !== value) onSave(val); }}
          onKeyDown={(e) => {
            if (!recording) return;
            e.preventDefault();
            if (["Control", "Shift", "Alt", "Meta"].includes(e.key)) return;
            const parts = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Super"].filter(Boolean) as string[];
            const key = e.key.length === 1 ? e.key.toUpperCase() : e.key === " " ? "Space" : e.key;
            const combo = [...parts, key].join("+");
            setVal(combo);
            setRecording(false);
            if (!v.shortcut(combo, required) && combo.toLowerCase() !== other.toLowerCase()) onSave(combo);
          }} aria-label={label} />
        <button className={"btn " + (recording ? "primary" : "")} onClick={(e) => { setRecording(!recording); ((e.currentTarget.previousSibling as HTMLInputElement) ?? null)?.focus(); }}><Keyboard size={15} /> {recording ? "Listening" : "Record"}</button>
      </div>
    </Field>
  );
}

function Shortcuts() {
  const { settings } = useApp();
  const patch = usePatch();
  return (
    <>
      <Card title="Global shortcuts" subtitle="These work even when the app is in the background.">
        <div className="stack">
          <ShortcutField label="Emergency stop" required hint="Stops the agent within half a second." value={settings.stop_shortcut} other={settings.summon_shortcut} onSave={(x) => patch({ stop_shortcut: x })} />
          <ShortcutField label="Show the app" hint="Brings the window to the front. Leave empty to turn off." value={settings.summon_shortcut} other={settings.stop_shortcut} onSave={(x) => patch({ summon_shortcut: x })} />
        </div>
      </Card>
      <Card title="In-app shortcuts">
        <dl className="kv">
          <dt><span className="kbd">Ctrl K</span></dt><dd>Command palette</dd>
          <dt><span className="kbd">Ctrl N</span></dt><dd>New chat</dd>
          <dt><span className="kbd">Ctrl B</span></dt><dd>Toggle sidebar</dd>
          <dt><span className="kbd">Ctrl ,</span></dt><dd>Settings</dd>
          <dt><span className="kbd">Ctrl Alt 1–5</span></dt><dd>Switch pages</dd>
          <dt><span className="kbd">/</span></dt><dd>Slash commands in the message box</dd>
        </dl>
      </Card>
    </>
  );
}

function Backup() {
  const { toast } = useApp();
  const [reset, setReset] = useState(false);
  const importFile = async () => {
    const f = await open({ filters: [{ name: "Settings", extensions: ["json"] }], title: "Choose a settings export" });
    if (typeof f !== "string") return;
    try {
      const content = await readTextFile(f);
      await api.importSettings(content);
      toast("Settings imported", "ok");
    } catch (e) {
      const p = v.parseEngineErrors(e);
      toast(p.message ?? Object.entries(p.fields).map(([k, m]) => `${k}: ${m}`).join(" · "), "bad");
    }
  };
  return (
    <>
      <Card title="Backup" subtitle="Move your setup to another computer. API keys and secrets are never exported.">
        <div className="row wrap">
          <button className="btn" onClick={async () => { try { const p = await api.exportSettings(); toast(`Exported to ${p}`, "ok"); api.reveal(p); } catch (e) { toast(errText(e), "bad"); } }}><Download size={15} /> Export settings</button>
          <button className="btn" onClick={importFile}><Upload size={15} /> Import settings</button>
        </div>
      </Card>
      <Card title="Reset" subtitle="Restores every setting to its default. Models, roles, chats and extensions data are kept.">
        <button className="btn" onClick={() => setReset(true)}><RotateCcw size={15} /> Reset to defaults</button>
      </Card>
      {reset && <ConfirmDialog title="Reset all settings?" confirmLabel="Reset" onClose={() => setReset(false)} onConfirm={async () => { await api.resetSettings(); toast("Settings reset", "ok"); }}>Appearance, agent behaviour, permissions, shortcuts, MCP servers and the sandbox go back to their defaults.</ConfirmDialog>}
    </>
  );
}

/** Reads a text file the user picked through the dialog. */
function readTextFile(path: string): Promise<string> {
  return invoke<string>("read_text_file", { path });
}

function System() {
  const [p, setP] = useState<PlatformInfo | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const load = () => { setErr(null); setP(null); api.platformInfo().then(setP).catch((e) => setErr(errText(e))); };
  useEffect(load, []);
  return (
    <Card title="This computer" subtitle="What works here. Reduced features are labelled, never hidden." actions={<button className="btn" onClick={load}><RotateCcw size={15} /> Re-check</button>}>
      {err && <Callout kind="bad">{err}</Callout>}
      {p ? <PlatformStatus p={p} /> : !err && <div className="muted">Checking…</div>}
    </Card>
  );
}

export function PlatformStatus({ p }: { p: PlatformInfo }) {
  const items: [string, boolean, string, typeof Monitor][] = [
    ["Screen capture", p.capture, p.capture ? `${p.monitors.length} display(s): ${p.monitors.map((m) => `${m.width}×${m.height} at ${Math.round(m.scale * 100)}%`).join(", ")}` : p.capture_error ?? "Not available. On macOS allow Screen Recording.", Monitor],
    ["Mouse and keyboard", p.input, p.input ? "Works. Admin-level windows need the app to run as administrator." : "Not available. On macOS allow Accessibility.", MousePointer2],
    ["Accessibility view", p.accessibility, p.accessibility ? `${p.accessibility_backend}: numbered tags and text maps` : "Not available on this OS yet; the agent uses vision only", ShieldCheck],
  ];
  return (
    <>
      <div className="status-list">
        {items.map(([name, ok, detail, I]) => (
          <div className="status-item" key={name}>
            <div className="si-icon"><I size={16} /></div>
            <div className="grow"><b>{name}</b><div className="small muted">{detail}</div></div>
            <span className={"badge " + (ok ? "ok" : "bad")}>{ok ? <CircleCheck size={12} /> : <CircleAlert size={12} />} {ok ? "Ready" : "Limited"}</span>
          </div>
        ))}
      </div>
      {p.wayland && <Callout kind="warn">Linux Wayland: screen capture and input control are restricted and vary by desktop. Supported with limits; X11 sessions work fully.</Callout>}
      <div className="small muted" style={{ marginTop: 10 }}><Gauge size={12} /> {p.os} {p.arch} · version {p.version}</div>
    </>
  );
}

function AiSection() {
  const { settings, models } = useApp();
  const patch = usePatch();
  const embedModels = models.filter((m) => m.effective.embedding);
  const chatModels = models.filter((m) => !m.effective.embedding);
  const sa = settings.subagents;
  const pool = sa.models.length ? chatModels.filter((m) => sa.models.includes(m.id)) : chatModels.filter((m) => m.local);
  const usable = sa.local_only ? pool.filter((m) => m.local) : pool;
  const toggleModel = (id: string, on: boolean) => patch({ subagents: { ...sa, models: on ? [...sa.models, id] : sa.models.filter((x) => x !== id) } });
  return (
    <>
      <Card title="Documents" subtitle="What happens when you attach a PDF, Word file, spreadsheet, text or code file to a chat.">
        <SettingRow label="When you attach a document" hint="Search sends only the relevant passages (needs an embedding model). Full text sends everything with your message.">
          <Select width={230} label="Document mode" value={settings.doc_mode} onChange={(x) => patch({ doc_mode: x as Settings["doc_mode"] })}
            options={[{ value: "ask", label: "Ask me each time" }, { value: "embed", label: "Search it (embedding)" }, { value: "direct", label: "Send the full text" }]} />
        </SettingRow>
        <SettingRow label="Embedding model" hint={embedModels.length ? "Turns documents into searchable vectors. Runs locally on the built-in engine, or any embedding model you added." : "None yet: download one in Models → Built-in engine → Embeddings."}>
          <Select width={230} label="Embedding model" value={settings.embed_model ?? ""} onChange={(x) => patch({ embed_model: x || null })}
            options={[{ value: "", label: "None" }, ...embedModels.map((m) => ({ value: m.id, label: m.label, hint: m.provider_name }))]} />
        </SettingRow>
        {settings.doc_mode === "embed" && !settings.embed_model && <Callout kind="warn">Search mode needs an embedding model; until one is set you are asked each time.</Callout>}
      </Card>

      <Card title="Sub-agents" subtitle="Hybrid mode: the chat model can hand self-contained work to other models, run several at once, and search your documents with the embedding model.">
        <ToggleRow label="Let the chat model use sub-agents" hint="Adds run_subagents and list_models tools for models with tool calling. Off by default." checked={sa.enabled} onChange={(x) => patch({ subagents: { ...sa, enabled: x } })} />
        {sa.enabled && (
          <>
            <ToggleRow label="Local models only" hint="Sub-agents never call a cloud model, even if the main model is in the cloud." checked={sa.local_only} onChange={(x) => patch({ subagents: { ...sa, local_only: x } })} />
            <SettingRow label="Run at the same time" hint="More is faster but loads your computer; the built-in engine runs one chat model at a time.">
              <Select width={120} label="Parallel sub-agents" value={String(sa.max_parallel)} onChange={(x) => patch({ subagents: { ...sa, max_parallel: Number(x) } })}
                options={[1, 2, 3, 4, 6, 8].map((n) => ({ value: String(n), label: String(n) }))} />
            </SettingRow>
            <div style={{ paddingTop: 12, borderTop: "1px solid var(--line)" }}>
              <div className="field-label">Models sub-agents may use</div>
              <div className="field-hint" style={{ marginBottom: 8 }}>{sa.models.length ? "Only the models ticked below." : "None ticked: every local chat model may be used."}</div>
              <div className="pick-grid">
                {chatModels.map((m) => (
                  <Checkbox key={m.id} checked={sa.models.includes(m.id)} onChange={(x) => toggleModel(m.id, x)} disabled={sa.local_only && !m.local}>
                    <span>{m.label} <span className="muted small">· {m.local ? "local" : "cloud"}{m.effective.tokens_per_sec ? ` · ${m.effective.tokens_per_sec} tok/s` : ""}</span></span>
                  </Checkbox>
                ))}
              </div>
              {usable.length === 0 && <Callout kind="warn">No model is available to sub-agents with these settings.</Callout>}
            </div>
          </>
        )}
      </Card>
    </>
  );
}
