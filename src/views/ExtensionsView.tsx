import { useEffect, useMemo, useRef, useState } from "react";
import {
  Blocks, Box, CheckCircle2, CircleAlert, Cpu, Download, FlaskConical, FolderOpen, Globe, Loader2, Package, Pencil, Play, Plug, Plus, Puzzle, RefreshCw, Search,
  Server, Square, Terminal, Trash2, Upload, Wrench,
} from "lucide-react";
import { api, errText, on, type McpServer, type McpTest, type PluginInfo, type SandboxCfg, type VmStatus } from "../api";
import { Callout, Card, ConfirmDialog, Empty, Field, Modal, NumberField, Pagination, Progress, PromptDialog, Seg, Select, SelectField, SettingRow, Skeleton, Spinner, Switch, TextField, usePaged, useApp } from "../components/common";
import * as v from "../validate";

type Tab = "plugins" | "mcp" | "sandbox" | "tools";

export default function ExtensionsView() {
  const [tab, setTab] = useState<Tab>("mcp");
  const tabs: [Tab, string, typeof Plug][] = [["mcp", "MCP servers", Server], ["plugins", "Plugins", Puzzle], ["sandbox", "Linux sandbox", Box], ["tools", "Tools & permissions", Wrench]];
  return (
    <div className="page">
      <div className="page-inner">
        <div className="page-head">
          <div className="grow">
            <h1>Extensions</h1>
            <p>Give the assistant and agent more abilities: connect MCP servers, add plugins, and run commands in an isolated Linux sandbox.</p>
          </div>
        </div>
        <div className="tabs" role="tablist">
          {tabs.map(([k, l, I]) => <button key={k} role="tab" aria-selected={tab === k} className={tab === k ? "on" : ""} onClick={() => setTab(k)}><I size={15} /> {l}</button>)}
        </div>
        {tab === "mcp" && <McpTab />}
        {tab === "plugins" && <PluginsTab />}
        {tab === "sandbox" && <SandboxTab />}
        {tab === "tools" && <ToolsTab />}
      </div>
    </div>
  );
}

/* ---------------- MCP ---------------- */

const MCP_PRESETS: { label: string; server: Partial<McpServer>; secretHint?: string }[] = [
  { label: "Filesystem (official)", server: { name: "filesystem", transport: "stdio", command: "npx", args: ["-y", "@modelcontextprotocol/server-filesystem", "C:\\Users"] } },
  { label: "Fetch web pages", server: { name: "fetch", transport: "stdio", command: "uvx", args: ["mcp-server-fetch"] } },
  { label: "Memory (knowledge graph)", server: { name: "memory", transport: "stdio", command: "npx", args: ["-y", "@modelcontextprotocol/server-memory"] } },
  { label: "GitHub (remote)", server: { name: "github", transport: "http", url: "https://api.githubcopilot.com/mcp/" }, secretHint: "Authorization: Bearer <your GitHub token>" },
  { label: "Custom local program", server: { name: "", transport: "stdio", command: "" } },
  { label: "Custom web address", server: { name: "", transport: "http", url: "https://" } },
];

const blankServer = (): McpServer => ({ id: "", name: "", transport: "stdio", command: "", args: [], env: {}, url: "", enabled: true, permission: "ask" });

function McpTab() {
  const { settings, toast } = useApp();
  const [status, setStatus] = useState<Record<string, { connected: boolean; error: string | null; tools: number; has_secret: boolean }>>({});
  const [editing, setEditing] = useState<{ server: McpServer; secretHint?: string } | null>(null);
  const [testing, setTesting] = useState<string | null>(null);
  const [del, setDel] = useState<McpServer | null>(null);
  const [lastTest, setLastTest] = useState<Record<string, McpTest>>({});
  const refresh = () => api.mcpStatus().then((l) => setStatus(Object.fromEntries(l.map((s) => [s.id, s]))));
  useEffect(() => {
    refresh();
  }, [settings.mcp_servers.length]);

  const test = async (id: string) => {
    setTesting(id);
    try {
      const r = await api.testMcpServer(id);
      setLastTest((t) => ({ ...t, [id]: r }));
      toast(r.ok ? `Connected · ${r.tools.length} tool(s)` : r.error ?? "Failed", r.ok ? "ok" : "bad");
    } catch (e) {
      toast(errText(e), "bad");
    }
    setTesting(null);
    refresh();
  };

  return (
    <>
      <Card title="Add an MCP server" subtitle="The Model Context Protocol connects tools and data sources. Start from a preset or add your own." icon={Plus}>
        <div className="row wrap">
          {MCP_PRESETS.map((p) => (
            <button key={p.label} className="chip" onClick={() => setEditing({ server: { ...blankServer(), ...p.server, args: p.server.args ?? [] }, secretHint: p.secretHint })}>
              {p.server.transport === "http" ? <Globe size={13} /> : <Terminal size={13} />} {p.label}
            </button>
          ))}
        </div>
        <div className="field-hint" style={{ marginTop: 10 }}>Local programs usually need Node.js (npx) or Python (uvx) installed. Tool descriptions and results from servers are treated as untrusted data.</div>
      </Card>
      <Card title="Your servers" icon={Server} flush={settings.mcp_servers.length > 0}>
        {settings.mcp_servers.length === 0 ? (
          <Empty icon={Plug} title="No MCP servers yet">Pick a preset above to connect your first server.</Empty>
        ) : (
          <div style={{ padding: "8px 20px 16px" }}>
            {settings.mcp_servers.map((m) => {
              const st = status[m.id];
              const lt = lastTest[m.id];
              return (
                <div className="list-row" key={m.id} style={{ alignItems: "flex-start" }}>
                  <div className="card-icon">{m.transport === "http" ? <Globe size={16} /> : <Terminal size={16} />}</div>
                  <div className="grow">
                    <div className="row wrap">
                      <b>{m.name}</b>
                      {!m.enabled ? <span className="badge">Disabled</span> : st?.connected ? <span className="badge ok"><CheckCircle2 size={12} /> {st.tools} tools</span> : st?.error ? <span className="badge bad"><CircleAlert size={12} /> Error</span> : <span className="badge">Not connected</span>}
                      <span className="badge">{m.permission === "allow" ? "Always allow" : m.permission === "ask" ? "Asks first" : "Off"}</span>
                    </div>
                    <div className="small muted mono ellipsis" style={{ maxWidth: 560 }}>{m.transport === "http" ? m.url : [m.command, ...m.args].join(" ")}</div>
                    {st?.error && <div className="field-error" style={{ marginTop: 4 }}>{st.error}</div>}
                    {lt?.ok && lt.tools.length > 0 && (
                      <details className="drawer" style={{ marginTop: 8 }}>
                        <summary><Wrench size={13} /> {lt.tools.length} tools</summary>
                        <div>{lt.tools.map((t) => `${t.name} — ${t.description}`).join("\n")}</div>
                      </details>
                    )}
                  </div>
                  <Switch label={`Enable ${m.name}`} checked={m.enabled} onChange={async (en) => { try { await api.saveMcpServer({ ...m, enabled: en }); refresh(); } catch (e) { toast(errText(e), "bad"); } }} />
                  <button className="btn sm" onClick={() => test(m.id)} disabled={testing === m.id || !m.enabled}>{testing === m.id ? <Loader2 size={14} className="spin" /> : <RefreshCw size={14} />} Test</button>
                  <button className="icon-btn" aria-label={`Edit ${m.name}`} onClick={() => setEditing({ server: m })}><Pencil size={15} /></button>
                  <button className="icon-btn danger" aria-label={`Remove ${m.name}`} onClick={() => setDel(m)}><Trash2 size={15} /></button>
                </div>
              );
            })}
          </div>
        )}
      </Card>
      {editing && <McpForm initial={editing.server} secretHint={editing.secretHint} hasSecret={!!status[editing.server.id]?.has_secret} onClose={() => setEditing(null)} onSaved={(r) => { setLastTest((t) => ({ ...t, [r.id]: r })); refresh(); }} />}
      {del && <ConfirmDialog title={`Remove ${del.name}?`} confirmLabel="Remove" onClose={() => setDel(null)} onConfirm={async () => { await api.deleteMcpServer(del.id); toast("Server removed", "ok"); }}>Its saved secrets are deleted from the credential store too.</ConfirmDialog>}
    </>
  );
}

function McpForm({ initial, secretHint, hasSecret, onClose, onSaved }: { initial: McpServer; secretHint?: string; hasSecret: boolean; onClose: () => void; onSaved: (r: McpTest) => void }) {
  const { toast } = useApp();
  const [s, setS] = useState<McpServer>(initial);
  const [argsText, setArgsText] = useState(initial.args.join("\n"));
  const [envText, setEnvText] = useState(Object.entries(initial.env).map(([k, val]) => `${k}=${val}`).join("\n"));
  const [secret, setSecret] = useState("");
  const [touched, setTouched] = useState(false);
  const [saving, setSaving] = useState(false);
  const [result, setResult] = useState<McpTest | null>(null);
  const [serverErr, setServerErr] = useState<v.Errors>({});

  const envParsed = useMemo(() => {
    const out: Record<string, string> = {};
    const bad: string[] = [];
    for (const line of envText.split("\n").map((l) => l.trim()).filter(Boolean)) {
      const i = line.indexOf("=");
      if (i <= 0 || /\s/.test(line.slice(0, i))) bad.push(line);
      else out[line.slice(0, i)] = line.slice(i + 1);
    }
    return { out, bad };
  }, [envText]);
  const secretBad = secret.trim() && s.transport === "http" ? secret.split("\n").filter((l) => l.trim() && !/^[A-Za-z0-9-]+\s*:/.test(l)) : secret.trim() ? secret.split("\n").filter((l) => l.trim() && !/^[A-Za-z_][A-Za-z0-9_]*=/.test(l)) : [];
  const errors: v.Errors = {
    name: serverErr.Name ?? (v.required(s.name, "A name") ?? v.maxLen(s.name, 40)),
    command: s.transport === "stdio" ? serverErr.Command ?? v.required(s.command, "A command") : undefined,
    url: s.transport === "http" ? serverErr.Address ?? v.url(s.url) : undefined,
    env: envParsed.bad.length ? `Use KEY=value lines (check: ${envParsed.bad[0]})` : undefined,
    secret: secretBad.length ? (s.transport === "http" ? "Use Header-Name: value lines" : "Use KEY=value lines") : undefined,
  };
  const show = (f: string) => (touched ? errors[f] : undefined);

  const save = async () => {
    setTouched(true);
    if (v.hasErrors(errors)) return;
    setSaving(true);
    setResult(null);
    try {
      const server = { ...s, args: argsText.split("\n").map((a) => a.trim()).filter(Boolean), env: envParsed.out };
      const r = await api.saveMcpServer(server, secret);
      setResult(r);
      setS({ ...server, id: r.id });
      onSaved(r);
      if (r.ok) toast(`${server.name} connected with ${r.tools.length} tool(s)`, "ok");
    } catch (e) {
      const p = v.parseEngineErrors(e);
      setServerErr(p.fields);
      if (p.message) toast(p.message, "bad");
    }
    setSaving(false);
  };

  return (
    <Modal title={initial.id ? `Edit ${initial.name}` : "Add MCP server"} subtitle="Saving connects to the server and lists its tools, so you know it works." icon={Server} wide onClose={onClose}
      footer={
        <>
          {result?.ok && <span className="saved-flag" style={{ marginRight: "auto" }}><CheckCircle2 size={14} /> Connected · {result.tools.length} tools</span>}
          <button className="btn" onClick={onClose}>{result?.ok ? "Done" : "Cancel"}</button>
          <button className="btn primary" onClick={save} disabled={saving}>{saving ? <><Loader2 size={15} className="spin" /> Connecting…</> : result && !result.ok ? "Save and retry" : "Save and connect"}</button>
        </>
      }>
      <div className="stack">
        <div className="grid2">
          <TextField label="Name" required value={s.name} onChange={(x) => { setS({ ...s, name: x }); setServerErr({}); }} error={show("name")} placeholder="e.g. filesystem" />
          <Field label="Type">
            <Seg value={s.transport} onChange={(t) => setS({ ...s, transport: t })} options={[{ value: "stdio", label: "Local program", icon: Terminal }, { value: "http", label: "Web address", icon: Globe }]} />
          </Field>
        </div>
        {s.transport === "stdio" ? (
          <>
            <TextField label="Command" required mono value={s.command} onChange={(x) => { setS({ ...s, command: x }); setServerErr({}); }} error={show("command")} placeholder="npx, uvx, python, node or a full path" />
            <Field label="Arguments" hint="One per line">
              <textarea className="input mono" rows={3} value={argsText} onChange={(e) => setArgsText(e.target.value)} placeholder={"-y\n@modelcontextprotocol/server-filesystem\nC:\\Users\\you\\Documents"} />
            </Field>
            <Field label="Environment variables" hint="KEY=value per line. Not secret; stored in settings." error={show("env")}>
              <textarea className={"input mono" + (show("env") ? " invalid" : "")} rows={2} value={envText} onChange={(e) => setEnvText(e.target.value)} placeholder="LOG_LEVEL=info" />
            </Field>
            <Field label="Secret environment variables" hint={hasSecret ? "Saved securely. Leave empty to keep them." : "KEY=value per line, e.g. API tokens. Stored in the OS credential store."} error={show("secret")}>
              <textarea className={"input mono" + (show("secret") ? " invalid" : "")} rows={2} value={secret} onChange={(e) => setSecret(e.target.value)} placeholder="API_TOKEN=…" />
            </Field>
          </>
        ) : (
          <>
            <TextField label="Server address" required mono value={s.url} onChange={(x) => { setS({ ...s, url: x }); setServerErr({}); }} error={show("url")} placeholder="https://example.com/mcp" />
            <Field label="Secret headers" hint={hasSecret ? "Saved securely. Leave empty to keep them." : secretHint ?? "Header-Name: value per line, e.g. Authorization: Bearer …"} error={show("secret")}>
              <textarea className={"input mono" + (show("secret") ? " invalid" : "")} rows={2} value={secret} onChange={(e) => setSecret(e.target.value)} placeholder="Authorization: Bearer …" />
            </Field>
          </>
        )}
        <SettingRow label="Permission" hint="Applies to every tool from this server, in chat and for the agent.">
          <Seg value={s.permission} onChange={(p) => setS({ ...s, permission: p })} options={[{ value: "allow", label: "Always allow" }, { value: "ask", label: "Ask first" }, { value: "off", label: "Off" }]} />
        </SettingRow>
        {result && !result.ok && <Callout kind="bad">{result.error}</Callout>}
        {result?.ok && (
          <Callout kind="ok">
            Connected. Tools: {result.tools.slice(0, 12).map((t) => t.name).join(", ")}{result.tools.length > 12 ? ` and ${result.tools.length - 12} more` : ""}
          </Callout>
        )}
      </div>
    </Modal>
  );
}

/* ---------------- Plugins ---------------- */

function PluginsTab() {
  const { toast } = useApp();
  const [plugins, setPlugins] = useState<PluginInfo[] | null>(null);
  const [creating, setCreating] = useState(false);
  const [testing, setTesting] = useState<{ p: PluginInfo; tool: string } | null>(null);
  const [del, setDel] = useState<PluginInfo | null>(null);
  const refresh = () => api.listPlugins().then(setPlugins).catch((e) => toast(errText(e), "bad"));
  useEffect(() => {
    refresh();
    // Pick up edits made in the plugins folder while the app was in the background.
    window.addEventListener("focus", refresh);
    return () => window.removeEventListener("focus", refresh);
  }, []);

  const importFolder = async () => {
    const dir = await api.pickPath("folder", { title: "Choose a plugin folder (with plugin.json)" });
    if (typeof dir !== "string") return;
    try {
      const name = await api.importPlugin(dir);
      toast(`Installed ${name}. Enable it to use its tools.`, "ok");
      refresh();
    } catch (e) {
      toast(errText(e), "bad");
    }
  };
  const openDir = async () => api.reveal(await api.pluginsDir());

  return (
    <>
      <Card title="Plugins" subtitle="A plugin is a folder with a plugin.json that declares tools. Each tool runs a program you choose and returns its output." icon={Puzzle}
        actions={
          <div className="row">
            <button className="btn" onClick={openDir}><FolderOpen size={15} /> Open folder</button>
            <button className="btn" onClick={importFolder}><Upload size={15} /> Import</button>
            <button className="btn primary" onClick={() => setCreating(true)}><Plus size={15} /> New plugin</button>
          </div>
        }>
        {!plugins ? <div className="row muted"><Loader2 size={15} className="spin" /> Loading…</div> : plugins.length === 0 ? (
          <Empty icon={Package} title="No plugins installed" action={<button className="btn primary" onClick={() => setCreating(true)}><Plus size={15} /> Create from template</button>}>
            Create one from the template; it includes a working example tool you can edit.
          </Empty>
        ) : (
          plugins.map((p) => (
            <div className="list-row" key={p.id} style={{ alignItems: "flex-start" }}>
              <div className="card-icon"><Blocks size={16} /></div>
              <div className="grow">
                <div className="row wrap"><b>{p.manifest?.name ?? p.id}</b>{p.manifest?.version && <span className="badge">v{p.manifest.version}</span>}{p.error && <span className="badge bad"><CircleAlert size={12} /> Invalid</span>}</div>
                {p.manifest?.description && <div className="small muted">{p.manifest.description}</div>}
                {p.error && <div className="field-error">{p.error}</div>}
                {p.manifest && (
                  <div className="row wrap" style={{ marginTop: 6 }}>
                    {p.manifest.tools.map((t) => (
                      <button key={t.name} className="chip" title={`${t.description}\nRisk: ${t.risk}. Click to test.`} onClick={() => setTesting({ p, tool: t.name })}><FlaskConical size={12} /> {t.name}</button>
                    ))}
                  </div>
                )}
              </div>
              <Select width={160} value={p.permission} disabled={!p.manifest} label="Permission" onChange={async (x) => { await api.setPluginPref(p.id, p.enabled, x); refresh(); }}
                options={[{ value: "auto", label: "Use risk level" }, { value: "allow", label: "Always allow" }, { value: "ask", label: "Ask first" }, { value: "off", label: "Off" }]} />
              <Switch label={`Enable ${p.id}`} checked={p.enabled} disabled={!p.manifest} onChange={async (en) => { await api.setPluginPref(p.id, en, p.permission); refresh(); toast(en ? "Plugin enabled" : "Plugin disabled", "ok"); }} />
              <button className="icon-btn" aria-label="Open folder" title="Open folder" onClick={() => api.reveal(p.dir)}><FolderOpen size={15} /></button>
              <button className="icon-btn danger" aria-label={`Delete ${p.id}`} title="Delete" onClick={() => setDel(p)}><Trash2 size={15} /></button>
            </div>
          ))
        )}
      </Card>
      <Callout kind="info">Tools with risk “gate” always ask. Plugins run with your user's permissions, so only install plugins you trust.</Callout>
      {creating && (
        <PromptDialog title="New plugin" label="Plugin name" hint="Letters, digits, - and _ only" confirmLabel="Create"
          validate={(x) => (!x.trim() ? "Enter a name" : !/^[A-Za-z0-9_-]{1,48}$/.test(x.trim()) ? "Use 1–48 letters, digits, - or _" : plugins?.some((p) => p.id === x.trim()) ? "That name is taken" : undefined)}
          onSubmit={async (name) => { const dir = await api.createPlugin(name); await refresh(); toast("Plugin created. Edit its files, then enable it.", "ok"); setTimeout(() => api.reveal(dir), 300); }}
          onClose={() => setCreating(false)} />
      )}
      {testing && <PluginTest p={testing.p} tool={testing.tool} onClose={() => setTesting(null)} />}
      {del && <ConfirmDialog title={`Delete ${del.id}?`} onClose={() => setDel(null)} onConfirm={async () => { await api.deletePlugin(del.id); refresh(); toast("Plugin deleted", "ok"); }}>The plugin folder and all its files are removed.</ConfirmDialog>}
    </>
  );
}

function PluginTest({ p, tool, onClose }: { p: PluginInfo; tool: string; onClose: () => void }) {
  const t = p.manifest?.tools.find((x) => x.name === tool);
  const props = (t?.parameters as { properties?: Record<string, unknown> })?.properties ?? {};
  const [args, setArgs] = useState(JSON.stringify(Object.fromEntries(Object.keys(props).map((k) => [k, "example"])), null, 2));
  const [out, setOut] = useState<{ ok: boolean; text: string } | null>(null);
  const [running, setRunning] = useState(false);
  let parseErr: string | undefined;
  try {
    const parsed = JSON.parse(args);
    if (typeof parsed !== "object" || Array.isArray(parsed) || parsed === null) parseErr = "Arguments must be a JSON object";
  } catch {
    parseErr = "Not valid JSON";
  }
  return (
    <Modal title={`Test ${tool}`} subtitle={t?.description} icon={FlaskConical} onClose={onClose}
      footer={<><button className="btn" onClick={onClose}>Close</button><button className="btn primary" disabled={!!parseErr || running} onClick={async () => {
        setRunning(true);
        try { setOut({ ok: true, text: await api.testPluginTool(p.id, tool, JSON.parse(args)) }); } catch (e) { setOut({ ok: false, text: errText(e) }); }
        setRunning(false);
      }}>{running ? <Loader2 size={15} className="spin" /> : <FlaskConical size={15} />} Run</button></>}>
      <div className="stack">
        <Field label="Arguments (JSON)" error={parseErr}>
          <textarea className={"input mono" + (parseErr ? " invalid" : "")} rows={6} value={args} onChange={(e) => setArgs(e.target.value)} />
        </Field>
        {out && <Field label={out.ok ? "Output" : "Error"}><pre className="code-block mono" style={{ color: out.ok ? undefined : "var(--danger)" }}>{out.text || "(no output)"}</pre></Field>}
      </div>
    </Modal>
  );
}

/* ---------------- Sandbox ---------------- */

function SandboxTab() {
  const { settings, setSettings, toast } = useApp();
  const [cfg, setCfg] = useState<SandboxCfg>(settings.sandbox);
  const [avail, setAvail] = useState<Awaited<ReturnType<typeof api.sandboxDetect>> | null>(null);
  const [detecting, setDetecting] = useState(false);
  const [testOut, setTestOut] = useState<{ ok: boolean; text: string } | null>(null);
  const [testing, setTesting] = useState(false);
  useEffect(() => setCfg(settings.sandbox), [settings.sandbox]);
  const detect = async () => {
    setDetecting(true);
    setAvail(await api.sandboxDetect());
    setDetecting(false);
  };
  useEffect(() => {
    detect();
  }, []);
  const errors: v.Errors = {
    distro: cfg.kind === "wsl" && !cfg.distro ? "Choose a distribution" : undefined,
    image: cfg.kind === "docker" ? v.required(cfg.image, "An image") ?? (/\s/.test(cfg.image) ? "No spaces in image names" : undefined) : undefined,
  };
  const dirty = JSON.stringify(cfg) !== JSON.stringify(settings.sandbox);
  const save = async () => {
    if (v.hasErrors(errors)) return;
    if (await setSettings({ ...settings, sandbox: cfg })) toast("Sandbox settings saved", "ok");
  };

  return (
    <>
      <MicroVmCard cfg={cfg} setCfg={setCfg} />
      <Card title="Where the agent runs commands" subtitle="The built-in VM needs nothing else installed. WSL and Docker are alternatives if you already use them." icon={Box}
        actions={<button className="btn" onClick={detect} disabled={detecting}>{detecting ? <Spinner /> : <RefreshCw size={15} />} Detect</button>}>
        <div className="stack">
          {avail && (
            <div className="row wrap">
              <span className={"badge " + (avail.wsl ? "ok" : "")}>{avail.wsl ? <CheckCircle2 size={12} /> : <CircleAlert size={12} />} WSL {avail.wsl ? `(${avail.wsl_distros.length} distro${avail.wsl_distros.length === 1 ? "" : "s"})` : "not found"}</span>
              <span className={"badge " + (avail.docker ? "ok" : "")}>{avail.docker ? <CheckCircle2 size={12} /> : <CircleAlert size={12} />} Docker {avail.docker ? avail.docker_version : "not running"}</span>
            </div>
          )}
          <Field label="Sandbox">
            <Seg value={cfg.kind} onChange={(k) => { setCfg({ ...cfg, kind: k, distro: k === "wsl" && !cfg.distro ? avail?.wsl_distros[0] ?? "" : cfg.distro }); setTestOut(null); }}
              options={[{ value: "microvm", label: "Built-in Linux VM" }, { value: "wsl", label: "WSL" }, { value: "docker", label: "Docker" }, { value: "none", label: "None" }]} />
          </Field>
          {cfg.kind === "wsl" && (
            avail && avail.wsl_distros.length === 0 ? (
              <Callout kind="warn">No WSL distribution is installed. A small one is Alpine Linux (about 10 MB): run <span className="kbd">wsl --install -d Alpine</span> in a terminal, then press Detect. Or use the built-in VM, which needs nothing installed.</Callout>
            ) : (
              <SelectField label="Distribution" value={cfg.distro} onChange={(d) => setCfg({ ...cfg, distro: d })} error={errors.distro} placeholder="Choose a distribution"
                options={(avail?.wsl_distros ?? (cfg.distro ? [cfg.distro] : [])).map((d) => ({ value: d, label: d }))} />
            )
          )}
          {cfg.kind === "docker" && (
            <>
              {avail && !avail.docker && <Callout kind="warn">Docker is not running. Start Docker Desktop, then press Detect.</Callout>}
              <div className="grid2">
                <TextField label="Image" mono value={cfg.image} onChange={(x) => setCfg({ ...cfg, image: x.trim() })} error={errors.image} hint="alpine:3.20 is about 8 MB" />
                <Field label="Network" hint="Off keeps commands fully offline">
                  <div className="row"><Switch label="Network access" checked={cfg.network} onChange={(n) => setCfg({ ...cfg, network: n })} /><span className="small">{cfg.network ? "Allowed" : "Blocked"}</span></div>
                </Field>
              </div>
              <div className="field-hint">The container gets the shared folder at /work and is removed after every command.</div>
            </>
          )}
          <div className="grid2">
            <NumberField label="Command time limit" suffix="seconds" value={cfg.timeout_secs} onCommit={(n) => setCfg({ ...cfg, timeout_secs: n })} min={5} max={3600} />
            <Field label="Host commands" hint={`Without a sandbox, commands run in ${navigator.userAgent.includes("Windows") ? "PowerShell" : "the shell"} after you approve each one.`}>
              <div className="row"><Switch label="Allow host commands" checked={cfg.allow_host} onChange={(n) => setCfg({ ...cfg, allow_host: n })} /><span className="small">{cfg.allow_host ? "Allowed with approval" : "Never"}</span></div>
            </Field>
          </div>
          {testOut && <pre className="term">{testOut.text}</pre>}
          <div className="row end">
            <button className="btn" onClick={() => api.sandboxFolder().then((d) => api.reveal(d))}><FolderOpen size={15} /> Shared folder</button>
            <button className="btn" disabled={cfg.kind === "none" || v.hasErrors(errors) || testing} onClick={async () => {
              setTesting(true);
              setTestOut(null);
              try { const o = await api.sandboxTest(cfg); setTestOut({ ok: o.code === 0, text: `${o.target} · exit ${o.code}\n${o.stdout}${o.stderr}` }); } catch (e) { setTestOut({ ok: false, text: errText(e) }); }
              setTesting(false);
            }}>{testing ? <Spinner /> : <FlaskConical size={15} />} {testing && cfg.kind === "microvm" ? "Booting and testing…" : "Test sandbox"}</button>
            <button className="btn primary" disabled={!dirty || v.hasErrors(errors)} onClick={save}>Save</button>
          </div>
        </div>
      </Card>
      <Callout kind="info">Commands in a sandbox still ask for approval in Supervised and Balanced modes. Host commands always ask, at every autonomy level.</Callout>
    </>
  );
}

/** The built-in Linux VM: download, status, resources and a small terminal. */
function MicroVmCard({ cfg, setCfg }: { cfg: SandboxCfg; setCfg: (c: SandboxCfg) => void }) {
  const { settings, setSettings, toast } = useApp();
  const [st, setSt] = useState<VmStatus | null>(null);
  const [dl, setDl] = useState<{ done: number; total: number } | null>(null);
  const [busy, setBusy] = useState<"" | "install" | "start" | "remove">("");
  const [confirmRemove, setConfirmRemove] = useState(false);
  const [cmd, setCmd] = useState("");
  const [log, setLog] = useState<{ cmd: string; out: string; err?: boolean }[]>([]);
  const [running, setRunning] = useState(false);
  const termRef = useRef<HTMLPreElement>(null);

  useEffect(() => {
    api.vmStatus().then(setSt);
    const offs = [
      on<VmStatus>("vm-status", setSt),
      on<{ done: number; total: number }>("vm-progress", (p) => setDl(p.done >= p.total ? null : p)),
    ];
    return () => offs.forEach((f) => f());
  }, []);
  useEffect(() => { termRef.current?.scrollTo({ top: termRef.current.scrollHeight }); }, [log, running]);

  const install = async () => {
    setBusy("install");
    setDl({ done: 0, total: 10_236_000 });
    try {
      setSt(await api.vmInstall());
      toast("Linux VM downloaded, verified and set as the sandbox", "ok");
    } catch (e) {
      toast(errText(e), "bad");
    }
    setDl(null);
    setBusy("");
  };
  const start = async () => {
    setBusy("start");
    try { setSt(await api.vmStart()); } catch (e) { toast(errText(e), "bad"); }
    setBusy("");
  };
  const run = async (e?: React.FormEvent) => {
    e?.preventDefault();
    const c = cmd.trim();
    if (!c || running) return;
    if (settings.sandbox.kind !== "microvm") return toast("Choose “Built-in Linux VM” below and save first.", "info");
    setRunning(true);
    setCmd("");
    try {
      const o = await api.sandboxExec(c);
      setLog((l) => [...l.slice(-40), { cmd: c, out: (o.stdout + (o.stderr ? (o.stdout ? "\n" : "") + o.stderr : "")).trimEnd() || `(exit ${o.code})`, err: o.code !== 0 }]);
    } catch (x) {
      setLog((l) => [...l.slice(-40), { cmd: c, out: errText(x), err: true }]);
    }
    setRunning(false);
  };

  const state = st?.state ?? "off";
  const stateLabel = busy === "start" || state === "starting" ? "Booting…" : state === "ready" ? "Running" : state === "error" ? "Failed" : "Stopped";
  const active = settings.sandbox.kind === "microvm";

  return (
    <Card title="Built-in Linux VM" icon={Cpu}
      subtitle="A small Linux system (BusyBox, about 10 MB) that runs inside the app. No WSL, Docker, Hyper-V or admin rights needed, and it has no network, so commands cannot reach the internet or your files outside the shared folder."
      actions={st?.installed ? <span className={"badge " + (active ? "ok" : "")}>{active ? <><CheckCircle2 size={12} /> In use</> : "Installed"}</span> : undefined}>
      {!st ? <Skeleton lines={2} /> : !st.installed ? (
        <div className="stack">
          {dl ? (
            <Progress value={(dl.done / dl.total) * 100} label={`Downloading and verifying · ${(dl.done / 1e6).toFixed(1)} of ${(dl.total / 1e6).toFixed(1)} MB`} />
          ) : (
            <div className="row">
              <button className="btn primary" onClick={install} disabled={!!busy}><Download size={15} /> Download Linux VM (10 MB)</button>
              <span className="small muted">Each file is checked against a pinned SHA-256 checksum before it is used.</span>
            </div>
          )}
        </div>
      ) : (
        <div className="stack">
          <div className="vm-stats">
            <div className="vm-stat"><small>Status</small><b><span className={"dot " + (state === "ready" ? "live" : state === "error" ? "bad" : "")} />{stateLabel}</b></div>
            <div className="vm-stat"><small>Memory</small><b>{cfg.vm_memory_mb} MB</b></div>
            <div className="vm-stat"><small>Last boot</small><b>{st.boot_ms ? `${(st.boot_ms / 1000).toFixed(1)} s` : "—"}</b></div>
            <div className="vm-stat"><small>Network</small><b>None (isolated)</b></div>
            <div className="vm-stat"><small>On disk</small><b>{st.size_mb} MB</b></div>
          </div>
          {(busy === "start" || state === "starting") && <Progress label="Starting Linux…" />}
          {state === "error" && <Callout kind="bad">{st.error}</Callout>}
          <div className="grid2">
            <SelectField label="Memory" value={String(cfg.vm_memory_mb)} onChange={(x) => setCfg({ ...cfg, vm_memory_mb: Number(x) })} hint="Takes effect on the next start"
              options={[64, 128, 256, 512].map((n) => ({ value: String(n), label: `${n} MB` }))} />
            <SelectField label="Stop when idle" value={String(cfg.vm_idle_minutes)} onChange={(x) => setCfg({ ...cfg, vm_idle_minutes: Number(x) })} hint="Frees memory; it starts again on the next command"
              options={[{ value: "0", label: "Never" }, { value: "5", label: "After 5 minutes" }, { value: "10", label: "After 10 minutes" }, { value: "30", label: "After 30 minutes" }, { value: "60", label: "After 1 hour" }]} />
          </div>
          <div className="row wrap">
            {state === "ready" ? (
              <button className="btn" onClick={() => api.vmStop()}><Square size={13} fill="currentColor" /> Stop</button>
            ) : (
              <button className="btn" onClick={start} disabled={!!busy || state === "starting"}>{busy === "start" ? <Spinner /> : <Play size={14} />} Start now</button>
            )}
            {!active && <button className="btn primary" onClick={() => setSettings({ ...settings, sandbox: { ...settings.sandbox, ...cfg, kind: "microvm" } })}><CheckCircle2 size={15} /> Use for agent commands</button>}
            <span className="grow" />
            <button className="btn subtle danger" onClick={() => setConfirmRemove(true)}><Trash2 size={14} /> Remove</button>
          </div>
          <Field label="Try it" hint="Runs in the VM's /work folder, which mirrors the shared folder.">
            <div>
              <pre className="term" ref={termRef} style={{ minHeight: 60 }}>
                {log.length === 0 && !running && <span className="muted">Type a command such as uname -a, ls -la or echo hello &gt; note.txt</span>}
                {log.map((l, i) => <div key={i}><span className="cmd">$ {l.cmd}</span>{"\n"}<span className={l.err ? "err" : undefined}>{l.out}</span></div>)}
                {running && <div className="muted">running…</div>}
              </pre>
              <form className="row" style={{ marginTop: 8 }} onSubmit={run}>
                <div className="input-group grow"><Terminal size={15} className="lead" /><input className="input mono" value={cmd} onChange={(e) => setCmd(e.target.value)} placeholder="Command" aria-label="Command to run in the VM" disabled={running} /></div>
                <button className="btn" disabled={!cmd.trim() || running}>{running ? <Spinner /> : <Play size={14} />} Run</button>
              </form>
            </div>
          </Field>
        </div>
      )}
      {confirmRemove && (
        <ConfirmDialog title="Remove the Linux VM?" confirmLabel="Remove" onClose={() => setConfirmRemove(false)} onConfirm={async () => { setBusy("remove"); try { await api.vmRemove(); setLog([]); toast("Linux VM removed", "ok"); } catch (e) { toast(errText(e), "bad"); } setBusy(""); }}>
          The VM files (about 10 MB) are deleted. Files in the shared folder stay. You can download it again any time.
        </ConfirmDialog>
      )}
    </Card>
  );
}

/* ---------------- Tools ---------------- */

function ToolsTab() {
  const { settings, setSettings, toast } = useApp();
  const [data, setData] = useState<Awaited<ReturnType<typeof api.listAllTools>> | null>(null);
  const [filter, setFilter] = useState("");
  const refresh = () => api.listAllTools().then(setData).catch((e) => toast(errText(e), "bad"));
  useEffect(() => {
    refresh();
  }, [settings.tool_permissions, settings.web_search]);
  const f = (s: string) => s.toLowerCase().includes(filter.toLowerCase());
  const builtinPage = usePaged((data?.builtin ?? []).filter((t) => f(t.name + t.description)), 10, filter);
  return (
    <>
      <Card title="Tool settings" icon={Wrench}>
        <SettingRow label="Let chat models use tools" hint="Every tool call is shown in the chat.">
          <Switch label="Chat tools" checked={settings.chat_tools} onChange={(x) => setSettings({ ...settings, chat_tools: x })} />
        </SettingRow>
        <SettingRow label="Web search" hint="Uses DuckDuckGo. Turned off automatically in local-only mode.">
          <Switch label="Web search" checked={settings.web_search} onChange={(x) => setSettings({ ...settings, web_search: x })} />
        </SettingRow>
      </Card>
      <div className="input-group" style={{ marginBottom: 12 }}><Search size={15} className="lead" /><input className="input" placeholder={`Search ${(data?.builtin.length ?? 0) + (data?.extensions.length ?? 0)} tools`} value={filter} onChange={(e) => setFilter(e.target.value)} /></div>
      <Card title="Built-in tools" flush>
        {!data ? <div style={{ padding: 20 }}><Skeleton lines={6} /></div> : (
          <table className="list" style={{ marginTop: 12 }}>
            <thead><tr><th>Tool</th><th>Used by</th><th style={{ width: 330 }}>Permission</th></tr></thead>
            <tbody>
              {builtinPage.slice.map((t) => (
                <tr key={t.name}>
                  <td><b className="mono">{t.name}</b><div className="small muted">{t.description}</div></td>
                  <td className="small">{t.scope}</td>
                  <td>
                    {t.permission ? (
                      <Seg value={settings.tool_permissions[t.name] ?? t.permission} label={`${t.name} permission`}
                        onChange={(p) => setSettings({ ...settings, tool_permissions: { ...settings.tool_permissions, [t.name]: p } })}
                        options={[{ value: "allow", label: "Allow" }, { value: "ask", label: "Ask" }, { value: "off", label: "Off" }]} />
                    ) : <span className="small muted">Agent safety rules</span>}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
        {data && <div style={{ padding: "0 16px 14px" }}><Pagination page={builtinPage.page} pages={builtinPage.pages} total={builtinPage.total} pageSize={builtinPage.pageSize} onPage={builtinPage.setPage} onPageSize={builtinPage.setPageSize} /></div>}
      </Card>
      <Card title="From extensions" subtitle="Change these on the MCP servers and Plugins tabs." flush>
        {data && data.extensions.length === 0 ? <Empty icon={Plug} title="No extension tools yet">Enable a plugin or connect an MCP server.</Empty> : (
          <table className="list" style={{ marginTop: 12 }}>
            <tbody>
              {data?.extensions.filter((t) => f(t.name + t.description)).map((t) => (
                <tr key={t.name}>
                  <td><b className="mono">{t.name}</b><div className="small muted ellipsis" style={{ maxWidth: 560 }}>{t.description}</div></td>
                  <td className="small">{t.origin}</td>
                  <td><span className={"badge " + (t.permission === "allow" ? "ok" : "warn")}>{t.permission === "allow" ? "Always allow" : "Asks first"}</span></td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Card>
      <Callout kind="info">Host shell commands always ask for approval, whatever the setting. The agent's screen actions follow the autonomy level and fixed safety gates.</Callout>
    </>
  );
}
