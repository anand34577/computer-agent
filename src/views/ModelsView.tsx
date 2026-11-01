import { useEffect, useMemo, useState } from "react";
import {
  CheckCircle2, Cloud, Cpu, Database, Download, FlaskConical, HardDrive, Loader2, Lock, MemoryStick, Monitor, Pencil, Plug, Plus, Radar, RefreshCw, Search, Server,
  Star, Trash2, Users,
} from "lucide-react";
import { api, errText, money, on, type CatalogItem, type Model, type Overrides, type Provider } from "../api";
import { CAPS, Callout, Caps, Card, Checkbox, ConfirmDialog, Empty, Field, Modal, NumberField, Pagination, Progress, Select, SelectField, Skeleton, Spinner, Switch, TextField, usePaged, useApp } from "../components/common";
import * as v from "../validate";
import LlamaEngine from "./LlamaEngine";
import DecisionModel from "./DecisionModel";

export const PRESETS = [
  { id: "openai", label: "OpenAI", kind: "openai", base: "https://api.openai.com/v1", key: true, keyNeeded: true },
  { id: "anthropic", label: "Anthropic (Claude)", kind: "anthropic", base: "https://api.anthropic.com", key: true, keyNeeded: true },
  { id: "openrouter", label: "OpenRouter", kind: "openai", base: "https://openrouter.ai/api/v1", key: true, keyNeeded: true },
  { id: "gemini", label: "Google Gemini", kind: "openai", base: "https://generativelanguage.googleapis.com/v1beta/openai", key: true, keyNeeded: true },
  { id: "groq", label: "Groq", kind: "openai", base: "https://api.groq.com/openai/v1", key: true, keyNeeded: true },
  { id: "mistral", label: "Mistral", kind: "openai", base: "https://api.mistral.ai/v1", key: true, keyNeeded: true },
  { id: "together", label: "Together AI", kind: "openai", base: "https://api.together.xyz/v1", key: true, keyNeeded: true },
  { id: "custom", label: "Custom OpenAI-compatible", kind: "openai", base: "", key: true, keyNeeded: false },
  { id: "ollama", label: "Ollama (local)", kind: "ollama", base: "http://127.0.0.1:11434", key: false, keyNeeded: false },
  { id: "lmstudio", label: "LM Studio (local)", kind: "openai", base: "http://127.0.0.1:1234/v1", key: false, keyNeeded: false },
  { id: "llamacpp", label: "llama.cpp server (local)", kind: "openai", base: "http://127.0.0.1:8080/v1", key: false, keyNeeded: false },
  { id: "jan", label: "Jan (local)", kind: "openai", base: "http://127.0.0.1:1337/v1", key: false, keyNeeded: false },
  { id: "vllm", label: "vLLM (local)", kind: "openai", base: "http://127.0.0.1:8000/v1", key: false, keyNeeded: false },
  { id: "ollama-lan", label: "Ollama on another computer", kind: "ollama", base: "http://192.168.1.10:11434", key: false, keyNeeded: false },
  { id: "custom-local", label: "Other local or home-network server", kind: "openai", base: "http://127.0.0.1:5000/v1", key: true, keyNeeded: false },
];

type Tab = "models" | "providers" | "roles" | "engine" | "local";

export default function ModelsView() {
  const [tab, setTab] = useState<Tab>("models");
  const [adding, setAdding] = useState(false);
  const [browse, setBrowse] = useState<Provider | null>(null);
  const { refreshModels } = useApp();
  const tabs: [Tab, string, typeof Cpu][] = [["models", "Your models", Cpu], ["providers", "Providers", Server], ["roles", "Roles", Users], ["engine", "Built-in engine", Cpu], ["local", "Ollama", HardDrive]];
  return (
    <div className="page">
      <div className="page-inner wide">
        <div className="page-head">
          <div className="grow">
            <h1>Models</h1>
            <p>Bring any model: cloud APIs, OpenAI-compatible services, local runtimes or downloads. Each one gets a tested capability profile.</p>
          </div>
          <button className="btn primary" onClick={() => setAdding(true)}><Plus size={16} /> Add provider</button>
        </div>
        <div className="tabs" role="tablist">
          {tabs.map(([k, l, I]) => (
            <button key={k} role="tab" aria-selected={tab === k} className={tab === k ? "on" : ""} onClick={() => setTab(k)}><I size={15} /> {l}</button>
          ))}
        </div>
        {tab === "models" && <ModelList onAdd={() => setAdding(true)} />}
        {tab === "providers" && <Providers onAdd={() => setAdding(true)} onBrowse={setBrowse} />}
        {tab === "roles" && <><Roles /><div style={{ marginTop: 16 }}><DecisionModel /></div></>}
        {tab === "engine" && <LlamaEngine />}
        {tab === "local" && <LocalModels />}
      </div>
      {adding && <AddProvider onClose={() => setAdding(false)} onAdded={async (id) => { setAdding(false); const p = (await api.listProviders()).find((x) => x.id === id); if (p) setBrowse(p); }} />}
      {browse && <BrowseModels p={browse} onClose={() => { setBrowse(null); refreshModels(); }} />}
    </div>
  );
}

type Filter = "all" | "local" | "cloud" | "vision" | "tools" | "reasoning" | "embedding" | "untested";
type Sort = "name" | "speed" | "context" | "provider";

const fmtCtx = (n: number) => (!n ? "—" : n >= 1000 ? `${Math.round(n / 1000)}K` : String(n));

function ModelList({ onAdd }: { onAdd: () => void }) {
  const { models, refreshModels, toast, settings, setSettings } = useApp();
  const [testing, setTesting] = useState<Record<string, boolean>>({});
  const [edit, setEdit] = useState<Model | null>(null);
  const [del, setDel] = useState<Model[] | null>(null);
  const [q, setQ] = useState("");
  const [filter, setFilter] = useState<Filter>("all");
  const [sort, setSort] = useState<Sort>("name");
  const [sel, setSel] = useState<Set<string>>(new Set());

  const test = async (m: Model, quiet = false) => {
    setTesting((t) => ({ ...t, [m.id]: true }));
    try {
      const p = await api.testModel(m.id);
      if (!quiet) toast(`${m.label}: ${p.notes.join(" · ")}`, "ok");
    } catch (e) {
      toast(`${m.label}: ${errText(e)}`, "bad");
    }
    setTesting((t) => ({ ...t, [m.id]: false }));
    refreshModels();
  };

  const shown = useMemo(() => {
    const needle = q.trim().toLowerCase();
    const list = models.filter((m) => {
      const e = m.effective;
      if (needle && !`${m.label} ${m.name} ${m.provider_name}`.toLowerCase().includes(needle)) return false;
      switch (filter) {
        case "local": return m.local;
        case "cloud": return !m.local;
        case "vision": return e.vision;
        case "tools": return e.tools;
        case "reasoning": return e.reasoning;
        case "embedding": return e.embedding;
        case "untested": return !e.tested;
        default: return true;
      }
    });
    const by: Record<Sort, (a: Model, b: Model) => number> = {
      name: (a, b) => a.label.localeCompare(b.label),
      provider: (a, b) => a.provider_name.localeCompare(b.provider_name) || a.label.localeCompare(b.label),
      speed: (a, b) => b.profile.tokens_per_sec - a.profile.tokens_per_sec,
      context: (a, b) => b.effective.context - a.effective.context,
    };
    return [...list].sort(by[sort]);
  }, [models, q, filter, sort]);
  const pg = usePaged(shown, 25, `${q}|${filter}|${sort}`);
  const counts = useMemo(() => ({
    all: models.length, local: models.filter((m) => m.local).length, cloud: models.filter((m) => !m.local).length,
    vision: models.filter((m) => m.effective.vision).length, tools: models.filter((m) => m.effective.tools).length,
    reasoning: models.filter((m) => m.effective.reasoning).length, embedding: models.filter((m) => m.effective.embedding).length,
    untested: models.filter((m) => !m.effective.tested).length,
  }), [models]);

  if (!models.length) {
    return (
      <Card>
        <Empty icon={Cpu} title="No models yet" action={<button className="btn primary" onClick={onAdd}><Plus size={16} /> Add a provider</button>}>
          Use the built-in engine with your own GGUF files, connect a local runtime such as Ollama or LM Studio, or a cloud provider with your own API key.
        </Empty>
      </Card>
    );
  }
  const pageIds = pg.slice.map((m) => m.id);
  const allOnPage = pageIds.length > 0 && pageIds.every((id) => sel.has(id));
  const toggle = (id: string) => setSel((s) => { const n = new Set(s); if (n.has(id)) n.delete(id); else n.add(id); return n; });
  const picked = models.filter((m) => sel.has(m.id));
  const showPrices = settings.show_prices;
  const showCtx = models.some((m) => m.effective.context > 0);
  const cols = 5 + (showPrices ? 1 : 0) + (showCtx ? 1 : 0);
  const FILTERS: [Filter, string][] = [["all", "All"], ["local", "Local"], ["cloud", "Cloud"], ["vision", "Vision"], ["tools", "Tools"], ["reasoning", "Reasoning"], ["embedding", "Embedding"], ["untested", "Untested"]];

  return (
    <div className="stack" style={{ gap: 12 }}>
      <div className="list-toolbar">
        <div className="search-box"><Search size={15} /><input className="input" value={q} onChange={(e) => setQ(e.target.value)} placeholder="Search models" aria-label="Search models" /></div>
        <div className="chips" role="group" aria-label="Filter models">
          {FILTERS.filter(([k]) => k === "all" || counts[k] > 0).map(([k, l]) => (
            <button key={k} className={"chip" + (filter === k ? " on" : "")} aria-pressed={filter === k} onClick={() => setFilter(k)}>{l} <span className="muted">{counts[k]}</span></button>
          ))}
        </div>
        <span className="grow" />
        <Select value={sort} onChange={(x) => setSort(x as Sort)} label="Sort models" width={150}
          options={[{ value: "name", label: "Sort: Name" }, { value: "provider", label: "Sort: Provider" }, { value: "speed", label: "Sort: Fastest" }, { value: "context", label: "Sort: Context" }]} />
        <label className="inline-switch" title="Show input and output prices"><Switch checked={showPrices} onChange={(x) => setSettings({ ...settings, show_prices: x })} label="Show prices" /> Prices</label>
      </div>

      {sel.size > 0 && (
        <div className="bulk-bar" role="toolbar" aria-label="Selected models">
          <b>{sel.size} selected</b>
          <button className="btn sm" onClick={async () => { for (const m of picked) await test(m, true); toast(`Tested ${picked.length} model(s)`, "ok"); }}><FlaskConical size={14} /> Test</button>
          <button className="btn sm" onClick={async () => {
            await setSettings({ ...settings, subagents: { ...settings.subagents, models: [...new Set([...settings.subagents.models, ...picked.filter((m) => !m.effective.embedding).map((m) => m.id)])] } });
            toast("Allowed as sub-agent models", "ok");
          }}><Users size={14} /> Allow as sub-agents</button>
          <button className="btn sm danger" onClick={() => setDel(picked)}><Trash2 size={14} /> Remove</button>
          <span className="grow" />
          <button className="btn sm subtle" onClick={() => setSel(new Set())}>Clear selection</button>
        </div>
      )}

      <Card flush>
        <table className="list models-table">
          <thead>
            <tr>
              <th className="sel"><Checkbox checked={allOnPage} onChange={(x) => setSel((s) => { const n = new Set(s); pageIds.forEach((id) => (x ? n.add(id) : n.delete(id))); return n; })} /></th>
              <th>Model</th><th>Capabilities</th>{showCtx && <th className="num">Context</th>}<th className="num">Speed</th>{showPrices && <th className="num">Price / 1M</th>}<th />
            </tr>
          </thead>
          <tbody>
            {pg.slice.map((m) => {
              const isDefault = settings.default_model === m.id;
              const isEmbed = settings.embed_model === m.id;
              return (
                <tr key={m.id} className={sel.has(m.id) ? "selected" : undefined}>
                  <td className="sel"><Checkbox checked={sel.has(m.id)} onChange={() => toggle(m.id)} /></td>
                  <td className="model-cell">
                    <div className="row" style={{ gap: 6, flexWrap: "nowrap" }}>
                      <b className="ellipsis">{m.label}</b>
                      {isDefault && <span className="badge accent">Default</span>}
                      {isEmbed && <span className="badge ok">Documents</span>}
                    </div>
                    <div className="small muted ellipsis" title={m.profile.notes.join("\n") || undefined}>{m.provider_name}{m.label !== m.name ? ` · ${m.name}` : ""}</div>
                  </td>
                  <td><Caps m={m} /></td>
                  {showCtx && <td className="num small">{fmtCtx(m.effective.context)}</td>}
                  <td className="num small">{m.profile.tested && m.profile.tokens_per_sec ? <><b>{m.profile.tokens_per_sec}</b> tok/s<div className="muted">{(m.profile.latency_ms / 1000).toFixed(1)}s first</div></> : <span className="muted">—</span>}</td>
                  {showPrices && <td className="num small">{m.local ? <span className="muted">Free</span> : <>{money(m.price_in)}<span className="muted"> in</span><div>{money(m.price_out)}<span className="muted"> out</span></div></>}</td>}
                  <td className="row-actions">
                    <button className="icon-btn" onClick={() => test(m)} disabled={testing[m.id]} title={m.profile.tested ? "Test again: tools, images and speed" : "Test: tools, images and speed"} aria-label={`Test ${m.label}`}>
                      {testing[m.id] ? <Loader2 size={15} className="spin" /> : <FlaskConical size={15} />}
                    </button>
                    {m.effective.embedding ? (
                      <button className={"icon-btn" + (isEmbed ? " active" : "")} disabled={isEmbed} title={isEmbed ? "Used for document search" : "Use for document search"} aria-label="Use for document search" onClick={() => setSettings({ ...settings, embed_model: m.id })}><Database size={15} /></button>
                    ) : (
                      <button className={"icon-btn" + (isDefault ? " active" : "")} disabled={isDefault} title={isDefault ? "Default model" : "Make default"} aria-label="Make default" onClick={() => setSettings({ ...settings, default_model: m.id })}><Star size={15} /></button>
                    )}
                    <button className="icon-btn" onClick={() => setEdit(m)} title="Edit" aria-label={`Edit ${m.label}`}><Pencil size={15} /></button>
                    <button className="icon-btn danger" title="Remove" aria-label={`Remove ${m.label}`} onClick={() => setDel([m])}><Trash2 size={15} /></button>
                  </td>
                </tr>
              );
            })}
            {pg.slice.length === 0 && <tr><td colSpan={cols} className="muted" style={{ textAlign: "center", padding: 28 }}>No models match.</td></tr>}
          </tbody>
        </table>
        {pg.total > 10 && <div style={{ padding: "0 14px 12px" }}><Pagination page={pg.page} pages={pg.pages} total={pg.total} pageSize={pg.pageSize} onPage={pg.setPage} onPageSize={pg.setPageSize} /></div>}
      </Card>
      <div className="cap-legend small muted">
        {CAPS.map((c) => <span key={c.key}><c.icon size={12} /> {c.label}</span>)}
        <span><HardDrive size={12} /> Local</span><span><Cloud size={12} /> Cloud</span>
      </div>
      {edit && <EditModel m={edit} onClose={() => { setEdit(null); refreshModels(); }} />}
      {del && (
        <ConfirmDialog title={del.length === 1 ? `Remove ${del[0].label}?` : `Remove ${del.length} models?`} confirmLabel="Remove" onClose={() => setDel(null)}
          onConfirm={async () => { for (const m of del) await api.deleteModel(m.id); setSel(new Set()); await refreshModels(); toast(del.length === 1 ? "Model removed" : `${del.length} models removed`, "ok"); }}>
          Chats keep their history. Any role that used {del.length === 1 ? "this model" : "these models"} goes back to the default.
        </ConfirmDialog>
      )}
    </div>
  );
}

function EditModel({ m, onClose }: { m: Model; onClose: () => void }) {
  const { toast } = useApp();
  const [label, setLabel] = useState(m.label);
  const [ov, setOv] = useState<Overrides>(m.overrides);
  const [pin, setPin] = useState(m.price_in);
  const [pout, setPout] = useState(m.price_out);
  const labelErr = v.required(label, "A name") ?? v.maxLen(label, 80);
  const save = async () => {
    if (labelErr) return;
    try {
      await api.updateModel(m.id, label.trim(), ov, pin, pout);
      toast("Saved", "ok");
      onClose();
    } catch (e) {
      toast(errText(e), "bad");
    }
  };
  return (
    <Modal title={`Edit ${m.name}`} icon={Pencil} onClose={onClose} footer={<><button className="btn" onClick={onClose}>Cancel</button><button className="btn primary" disabled={!!labelErr} onClick={save}>Save</button></>}>
      <div className="stack">
        <TextField label="Display name" required value={label} onChange={setLabel} error={labelErr} />
        <div>
          <div className="field-label" style={{ marginBottom: 6 }}>Capabilities</div>
          <div className="cap-toggles">
            {CAPS.map((c) => {
              const detected = !!m.profile[c.key];
              const forced = ov[c.key];
              const value = forced ?? detected;
              return (
                <div key={c.key} className={"cap-toggle" + (value ? " on" : "")}>
                  <span className={"cap " + c.key + (value ? " on" : "")}><c.icon size={15} /></span>
                  <div className="grow">
                    <div className="cap-name">{c.label}</div>
                    <div className="small muted">
                      {forced == null ? `${m.profile.tested ? "Tested" : "Guessed"}: ${detected ? "yes" : "no"}` : <>Set by you · <button className="link" onClick={() => setOv({ ...ov, [c.key]: null })}>reset to {detected ? "yes" : "no"}</button></>}
                    </div>
                  </div>
                  <Switch checked={value} label={c.label} onChange={(x) => setOv({ ...ov, [c.key]: x === detected ? null : x })} />
                </div>
              );
            })}
          </div>
          <div className="field-hint" style={{ marginTop: 6 }}>Your settings win over the test. Turning vision on for a model that cannot see makes agent tasks fail; marking a model as Embedding hides it from chat.</div>
        </div>
        {!m.local && (
          <div className="grid2">
            <NumberField label="Input price (USD / 1M tokens)" value={pin} onCommit={setPin} min={0} max={1000} step={0.01} />
            <NumberField label="Output price (USD / 1M tokens)" value={pout} onCommit={setPout} min={0} max={1000} step={0.01} />
          </div>
        )}
      </div>
    </Modal>
  );
}

function Providers({ onAdd, onBrowse }: { onAdd: () => void; onBrowse: (p: Provider) => void }) {
  const { toast, refreshModels } = useApp();
  const [providers, setProviders] = useState<Provider[]>([]);
  const [detected, setDetected] = useState<{ name: string; kind: string; base_url: string; running: boolean }[] | null>(null);
  const [detecting, setDetecting] = useState(false);
  const [edit, setEdit] = useState<Provider | null>(null);
  const [del, setDel] = useState<Provider | null>(null);
  const refresh = () => api.listProviders().then(setProviders);
  useEffect(() => {
    refresh();
  }, []);

  const detect = async () => {
    setDetecting(true);
    setDetected(await api.detectLocal());
    setDetecting(false);
  };
  const connect = async (d: { name: string; kind: string; base_url: string }) => {
    try {
      const id = await api.addProvider(d.kind, d.name, d.base_url);
      await refresh();
      const p = (await api.listProviders()).find((x) => x.id === id);
      if (p) onBrowse(p);
    } catch (e) {
      toast(errText(e), "bad");
    }
  };

  return (
    <>
      <Card title="Local runtimes" subtitle="Finds Ollama, LM Studio, llama.cpp, Jan and vLLM running on this computer. Nothing leaves your machine." icon={Radar}
        actions={<button className="btn" onClick={detect} disabled={detecting}>{detecting ? <Loader2 size={15} className="spin" /> : <Search size={15} />} Detect</button>}>
        {detected ? (
          <div className="status-list">
            {detected.map((d) => {
              const exists = providers.some((p) => p.base_url === d.base_url);
              return (
                <div className="status-item" key={d.name}>
                  <div className="si-icon"><Monitor size={16} /></div>
                  <div className="grow"><b>{d.name}</b><div className="small muted">{d.base_url}</div></div>
                  {exists ? <span className="badge ok"><CheckCircle2 size={12} /> Connected</span> : d.running ? <button className="btn sm primary" onClick={() => connect(d)}>Connect</button> : <span className="badge">Not running</span>}
                </div>
              );
            })}
          </div>
        ) : <div className="small muted">Press Detect to look for local runtimes.</div>}
      </Card>
      <Card title="Providers" icon={Server} actions={<button className="btn" onClick={onAdd}><Plus size={15} /> Add</button>} flush={providers.length > 0}>
        {providers.length === 0 ? (
          <Empty icon={Plug} title="No providers yet">Add a provider to use its models.</Empty>
        ) : (
          <table className="list" style={{ marginTop: 12 }}>
            <tbody>
              {providers.map((p) => (
                <tr key={p.id}>
                  <td><b>{p.name}</b><div className="small muted">{p.base_url}</div></td>
                  <td>
                    <div className="row">
                      <span className={"badge " + (p.local ? "ok" : "warn")}>{p.local ? <Lock size={11} /> : <Cloud size={11} />} {p.local ? "Local" : "Cloud"}</span>
                      {!p.local && <span className={"badge " + (p.has_key ? "ok" : "bad")}>{p.has_key ? "Key stored securely" : "No key"}</span>}
                    </div>
                  </td>
                  <td style={{ textAlign: "right", whiteSpace: "nowrap" }}>
                    <button className="btn sm" onClick={() => onBrowse(p)}><Plus size={14} /> Models</button>
                    <button className="icon-btn" onClick={() => setEdit(p)} title="Edit" aria-label={`Edit ${p.name}`}><Pencil size={15} /></button>
                    <button className="icon-btn danger" title="Remove" aria-label={`Remove ${p.name}`} onClick={() => setDel(p)}><Trash2 size={15} /></button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Card>
      {edit && <EditProvider p={edit} onClose={() => { setEdit(null); refresh(); }} />}
      {del && (
        <ConfirmDialog title={`Remove ${del.name}?`} confirmLabel="Remove" onClose={() => setDel(null)} onConfirm={async () => { await api.deleteProvider(del.id); await refresh(); await refreshModels(); toast("Provider removed", "ok"); }}>
          Its models are removed from the app and its API key is deleted from the credential store.
        </ConfirmDialog>
      )}
    </>
  );
}

/** Add-provider form with field validation and a live connection test before saving. */
export function AddProvider({ onClose, onAdded }: { onClose: () => void; onAdded: (id: string) => void }) {
  const { toast, settings } = useApp();
  const [preset, setPreset] = useState(PRESETS[settings.local_only ? 8 : 0]);
  const [name, setName] = useState(preset.label);
  const [base, setBase] = useState(preset.base);
  const [key, setKey] = useState("");
  const [agree, setAgree] = useState(false);
  const [touched, setTouched] = useState<Record<string, boolean>>({});
  const [test, setTest] = useState<{ state: "idle" | "running" | "ok" | "bad"; text?: string }>({ state: "idle" });
  const [serverErr, setServerErr] = useState<v.Errors>({});
  const [saving, setSaving] = useState(false);
  const local = v.isLocalUrl(base);
  const errors: v.Errors = {
    name: serverErr.Name ?? v.required(name, "A name") ?? v.maxLen(name, 60),
    base: serverErr["Base address"] ?? v.url(base),
    key: serverErr["API key"] ?? (preset.key ? v.apiKey(key, preset.keyNeeded && !local) : undefined),
    agree: !local && !agree ? "Please confirm to continue" : undefined,
  };
  const show = (f: string) => (touched[f] ? errors[f] : undefined);
  const blockedByLocalOnly = settings.local_only && !local;

  const runTest = async () => {
    setTouched({ name: true, base: true, key: true });
    if (errors.base || errors.key) return;
    setTest({ state: "running" });
    try {
      const r = await api.testProvider(preset.kind, base, key);
      const fixed = r.base_url.replace(/\/$/, "") !== base.trim().replace(/\/$/, "");
      if (fixed) setBase(r.base_url);
      setTest({ state: "ok", text: `Connected. ${r.models.length} model(s) available.${fixed ? ` The address was corrected to ${r.base_url}.` : ""}` });
    } catch (e) {
      setTest({ state: "bad", text: errText(e) });
    }
  };
  const add = async () => {
    setTouched({ name: true, base: true, key: true, agree: true });
    if (v.hasErrors(errors) || blockedByLocalOnly) return;
    setSaving(true);
    try {
      if (!local) await api.recordConsent(`provider-terms:${preset.id}`);
      onAdded(await api.addProvider(preset.kind, name.trim(), base.trim(), key.trim()));
      toast(`${name} added`, "ok");
    } catch (e) {
      const parsed = v.parseEngineErrors(e);
      setServerErr(parsed.fields);
      if (parsed.message) toast(parsed.message, "bad");
    }
    setSaving(false);
  };
  return (
    <Modal title="Add a provider" subtitle="Your key is stored in the system's secure credential store, never in files." icon={Plug} onClose={onClose}
      footer={
        <>
          <button className="btn subtle" onClick={runTest} disabled={test.state === "running"} style={{ marginRight: "auto" }}>
            {test.state === "running" ? <Loader2 size={15} className="spin" /> : <FlaskConical size={15} />} Test connection
          </button>
          <button className="btn" onClick={onClose}>Cancel</button>
          <button className="btn primary" disabled={saving || blockedByLocalOnly} onClick={add}>{saving ? "Adding…" : "Add provider"}</button>
        </>
      }>
      <div className="stack">
        <SelectField label="Provider" value={preset.id} onChange={(id) => { const p = PRESETS.find((x) => x.id === id)!; setPreset(p); setName(p.label); setBase(p.base); setTest({ state: "idle" }); setServerErr({}); }}>
          <optgroup label="Cloud">{PRESETS.filter((p) => !p.base.startsWith("http://")).map((p) => <option key={p.id} value={p.id}>{p.label}</option>)}</optgroup>
          <optgroup label="On this computer or your network">{PRESETS.filter((p) => p.base.startsWith("http://")).map((p) => <option key={p.id} value={p.id}>{p.label}</option>)}</optgroup>
        </SelectField>
        <TextField label="Name" required value={name} onChange={(x) => { setName(x); setServerErr({}); }} onBlur={() => setTouched({ ...touched, name: true })} error={show("name")} />
        <TextField label="Base address" required value={base} mono placeholder="https://api.example.com/v1" onChange={(x) => { setBase(x); setTest({ state: "idle" }); setServerErr({}); }} onBlur={() => setTouched({ ...touched, base: true })} error={show("base")}
          hint={v.insecureWarning(base) ?? (local ? "Local or home-network address: nothing goes to the internet. /v1 is added automatically if needed." : "Data you send goes to this address.")} />
        {preset.key && (
          <TextField label={`API key${!preset.keyNeeded || local ? " (optional)" : ""}`} required={preset.keyNeeded && !local} type="password" value={key} onChange={(x) => { setKey(x); setTest({ state: "idle" }); setServerErr({}); }}
            onBlur={() => setTouched({ ...touched, key: true })} error={show("key")} placeholder="Paste your key" />
        )}
        {test.state === "ok" && <Callout kind="ok">{test.text}</Callout>}
        {test.state === "bad" && <Callout kind="bad">{test.text}</Callout>}
        {blockedByLocalOnly && <Callout kind="warn">Local-only mode is on, so cloud providers cannot be added. Turn it off in Settings first.</Callout>}
        {!local && !blockedByLocalOnly && (
          <Field label="Consent" error={show("agree")}>
            <Checkbox checked={agree} onChange={setAgree}>
              <span className="small" style={{ color: "var(--text-2)" }}>I understand that chats and, when the agent runs, screenshots of my screen will be sent to {name || "this provider"}. Some providers restrict automated actions in their terms.</span>
            </Checkbox>
          </Field>
        )}
      </div>
    </Modal>
  );
}

function EditProvider({ p, onClose }: { p: Provider; onClose: () => void }) {
  const { toast } = useApp();
  const [name, setName] = useState(p.name);
  const [base, setBase] = useState(p.base_url);
  const [key, setKey] = useState("");
  const [test, setTest] = useState<{ state: "idle" | "running" | "ok" | "bad"; text?: string }>({ state: "idle" });
  const errors = { name: v.required(name, "A name") ?? v.maxLen(name, 60), base: v.url(base), key: v.apiKey(key, false) };
  const save = async () => {
    if (v.hasErrors(errors)) return;
    try {
      await api.updateProvider(p.id, name.trim(), base.trim(), key.trim());
      toast("Saved", "ok");
      onClose();
    } catch (e) {
      toast(errText(e), "bad");
    }
  };
  return (
    <Modal title={`Edit ${p.name}`} icon={Pencil} onClose={onClose}
      footer={
        <>
          <button className="btn subtle" style={{ marginRight: "auto" }} disabled={test.state === "running" || !!errors.base} onClick={async () => {
            setTest({ state: "running" });
            try {
              const r = await api.testProvider(p.kind, base, key, p.id);
              if (r.base_url !== base.trim()) setBase(r.base_url);
              setTest({ state: "ok", text: `Connected. ${r.models.length} model(s) available.` });
            }
            catch (e) { setTest({ state: "bad", text: errText(e) }); }
          }}>{test.state === "running" ? <Loader2 size={15} className="spin" /> : <FlaskConical size={15} />} Test connection</button>
          <button className="btn" onClick={onClose}>Cancel</button>
          <button className="btn primary" disabled={v.hasErrors(errors)} onClick={save}>Save</button>
        </>
      }>
      <div className="stack">
        <TextField label="Name" required value={name} onChange={setName} error={errors.name} />
        <TextField label="Base address" required mono value={base} onChange={setBase} error={errors.base} />
        <TextField label="Replace API key" type="password" value={key} onChange={setKey} error={errors.key} placeholder={p.has_key ? "Leave empty to keep the saved key" : "No key saved"} />
        {test.state === "ok" && <Callout kind="ok">{test.text}</Callout>}
        {test.state === "bad" && <Callout kind="bad">{test.text}</Callout>}
      </div>
    </Modal>
  );
}

export function BrowseModels({ p, onClose }: { p: Provider; onClose: () => void }) {
  const { models, toast, refreshModels } = useApp();
  const [list, setList] = useState<{ name: string; size: number }[] | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [filter, setFilter] = useState("");
  const [manual, setManual] = useState("");
  const [adding, setAdding] = useState<string | null>(null);
  const load = () => { setErr(null); setList(null); api.remoteModels(p.id).then(setList).catch((e) => setErr(errText(e))); };
  useEffect(load, [p.id]);
  const have = new Set(models.filter((m) => m.provider_id === p.id).map((m) => m.name));
  const add = async (name: string) => {
    setAdding(name);
    try {
      const id = await api.addModel(p.id, name);
      await refreshModels();
      toast(`Added ${name}. Testing its capabilities…`, "ok");
      api.testModel(id).then(() => refreshModels()).catch(() => {});
    } catch (e) {
      toast(errText(e), "bad");
    }
    setAdding(null);
  };
  const shown = (list ?? []).filter((m) => m.name.toLowerCase().includes(filter.toLowerCase()));
  const pg = usePaged(shown, 50, filter);
  const manualErr = manual && (manual.trim().length > 200 ? "Too long" : have.has(manual.trim()) ? "Already added" : undefined);
  return (
    <Modal title={`Models from ${p.name}`} subtitle="Pick the models you want. Each one is tested automatically after it is added." icon={Cpu} onClose={onClose} wide footer={<button className="btn primary" onClick={onClose}>Done</button>}>
      {err && <Callout kind="bad" action={<button className="btn sm" onClick={load}><RefreshCw size={14} /> Retry</button>}>{err}</Callout>}
      {!list && !err && <><div className="row muted" style={{ marginBottom: 8 }}><Spinner /> Asking {p.name} for its models…</div><Skeleton lines={5} /></>}
      {list && (
        <>
          <div className="filters">
            <div className="input-group"><Search size={15} className="lead" /><input className="input" placeholder={`Filter ${list.length} models`} value={filter} onChange={(e) => setFilter(e.target.value)} autoFocus /></div>
            <button className="btn" onClick={load} title="Fetch the list again"><RefreshCw size={15} /> Refresh</button>
          </div>
          <div style={{ maxHeight: 340, overflow: "auto", border: "1px solid var(--line)", borderRadius: "var(--r)" }}>
            <table className="list">
              <tbody>
                {pg.slice.map((m) => (
                  <tr key={m.name}>
                    <td className="mono">{m.name}{m.size ? <span className="small muted"> · {(m.size / 1e9).toFixed(1)} GB</span> : null}</td>
                    <td style={{ textAlign: "right" }}>
                      {have.has(m.name) ? <span className="badge ok"><CheckCircle2 size={12} /> Added</span> : <button className="btn sm" disabled={adding === m.name} onClick={() => add(m.name)}>{adding === m.name ? <Loader2 size={14} className="spin" /> : <Plus size={14} />} Add</button>}
                    </td>
                  </tr>
                ))}
                {shown.length === 0 && <tr><td className="muted">No models listed{p.kind === "ollama" ? ". Download one under Local models." : "."}</td></tr>}
              </tbody>
            </table>
          </div>
          <Pagination page={pg.page} pages={pg.pages} total={pg.total} pageSize={pg.pageSize} onPage={pg.setPage} />
        </>
      )}
      <form className="row" style={{ marginTop: 12, alignItems: "flex-start" }} onSubmit={(e) => { e.preventDefault(); if (manual.trim() && !manualErr) { add(manual.trim()); setManual(""); } }}>
        <div className="grow"><TextField label="Or type a model name exactly" mono value={manual} onChange={setManual} error={manualErr || undefined} placeholder="e.g. gpt-4.1-mini" /></div>
        <button className="btn" style={{ marginTop: 25 }} disabled={!manual.trim() || !!manualErr} type="submit">Add</button>
      </form>
    </Modal>
  );
}

function Roles() {
  const { models, settings, setSettings } = useApp();
  const vision = models.filter((m) => m.effective.vision);
  const sel = (label: string, hint: string, value: string | null, onChange: (v: string | null) => void, list: Model[], none: string) => (
    <SelectField label={label} hint={hint} value={value ?? ""} onChange={(x) => onChange(x || null)}>
      <option value="">{none}</option>
      {list.map((m) => <option key={m.id} value={m.id}>{m.label} · {m.provider_name}{m.local ? " (local)" : ""}</option>)}
    </SelectField>
  );
  const doer = models.find((m) => m.id === (settings.roles.doer ?? settings.default_model));
  const mode = (doer?.effective.vision ? "Vision: the Doer reads screenshots with numbered tags." : settings.roles.eyes ? "Eyes: a vision model describes the screen for the text-only Doer." : "Accessibility view: the Doer works from a text map of the window. Add an Eyes model for apps that expose little.") + (settings.roles.locator ? " The Locator pinpoints anything the Doer describes." : "");
  return (
    <Card title="Who does what" subtitle="Leave everything on the default model, or mix: a local model for routine steps and a stronger one for planning." icon={Users}>
      <div className="grid2">
        {sel("Default chat model", "Used for chat and as the agent's default.", settings.default_model, (x) => setSettings({ ...settings, default_model: x }), models, "None")}
        {sel("Fallback model", "Offered when a provider fails. Asks first.", settings.fallback_model, (x) => setSettings({ ...settings, fallback_model: x }), models, "None")}
        {sel("Planner", "Breaks goals into steps and helps when stuck.", settings.roles.planner, (x) => setSettings({ ...settings, roles: { ...settings.roles, planner: x } }), models, "Same as Doer")}
        {sel("Doer", "Performs each step on screen.", settings.roles.doer, (x) => setSettings({ ...settings, roles: { ...settings.roles, doer: x } }), models, "Default chat model")}
        {sel("Eyes", "A vision model that describes the screen for text-only Doers.", settings.roles.eyes, (x) => setSettings({ ...settings, roles: { ...settings.roles, eyes: x } }), vision, vision.length ? "None" : "No vision models yet")}
        {sel("Locator (fast)", "A small vision grounding model, such as UI-TARS, Holo, GroundNext, Phi-Ground or Qwen-VL. It finds what the Doer describes, so the Doer can act on screens with no numbered tags.", settings.roles.locator, (x) => setSettings({ ...settings, roles: { ...settings.roles, locator: x } }), vision, vision.length ? "None" : "No vision models yet")}
        {settings.roles.locator && (
          <SelectField label="Locator points" hint="How the locator writes positions. Auto works for most; pick one if clicks land in the wrong place." value={settings.roles.locator_coords || "auto"} onChange={(x) => setSettings({ ...settings, roles: { ...settings.roles, locator_coords: x } })}>
            <option value="auto">Auto-detect</option>
            <option value="pixels">Image pixels (UI-TARS 1.5, Qwen2.5-VL)</option>
            <option value="norm1000">0 to 1000 (Qwen3-VL, UI-TARS-2, GLM)</option>
            <option value="norm1">0 to 1 (Holo, some Gemma)</option>
          </SelectField>
        )}
        <TextField label="Checker" value="Built-in safety rules (always on)" onChange={() => {}} disabled hint="Safety decisions are never left to a model." />
      </div>
      <div style={{ marginTop: 16 }}><Callout kind="info">Current agent mode: {mode}</Callout></div>
    </Card>
  );
}

function LocalModels() {
  const { toast, refreshModels } = useApp();
  const [hw, setHw] = useState<Awaited<ReturnType<typeof api.hardware>> | null>(null);
  const [catalog, setCatalog] = useState<CatalogItem[]>([]);
  const [ollama, setOllama] = useState<Provider | null>(null);
  const [installed, setInstalled] = useState<{ name: string; size: number }[]>([]);
  const [pulls, setPulls] = useState<Record<string, { status: string; pct: number }>>({});
  const [running, setRunning] = useState<boolean | null>(null);
  const [del, setDel] = useState<string | null>(null);
  const [custom, setCustom] = useState("");

  const refresh = async () => {
    const ps = await api.listProviders();
    const o = ps.find((p) => p.kind === "ollama") ?? null;
    setOllama(o);
    if (o) {
      try {
        setInstalled(await api.remoteModels(o.id));
        setRunning(true);
      } catch {
        setRunning(false);
      }
    } else {
      const d = await api.detectLocal();
      setRunning(d.find((x) => x.name === "Ollama")?.running ?? false);
    }
  };

  useEffect(() => {
    api.hardware().then(setHw);
    api.catalog().then(setCatalog);
    refresh();
    return on<{ name: string; status: string; completed: number; total: number }>("ollama-pull", (p) => {
      setPulls((s) => ({ ...s, [p.name]: { status: p.status, pct: p.total ? Math.round((p.completed / p.total) * 100) : 0 } }));
      if (p.status === "success") {
        refresh();
        setTimeout(() => setPulls((s) => { const n = { ...s }; delete n[p.name]; return n; }), 1500);
      }
    });
  }, []);

  const base = ollama?.base_url ?? "http://127.0.0.1:11434";
  const ensureProvider = async () => {
    if (ollama) return ollama.id;
    const id = await api.addProvider("ollama", "Ollama", base);
    await refresh();
    return id;
  };
  const fitOf = (size: number) => (!hw ? null : size * 1.3 <= hw.ram_gb * 0.6 ? "good" : size * 1.3 <= hw.ram_gb * 0.9 ? "tight" : "too-large");
  const pull = async (c: CatalogItem) => {
    if (hw && hw.disk_free_gb && c.size_gb * 1.1 > hw.disk_free_gb) return toast(`Not enough disk space: ${c.name} needs about ${c.size_gb} GB and ${hw.disk_free_gb} GB is free.`, "bad");
    if (fitOf(c.size_gb) === "too-large") return toast(`${c.name} needs more memory than this computer has. Pick a smaller model.`, "bad");
    try {
      await ensureProvider();
      setPulls((s) => ({ ...s, [c.name]: { status: "starting", pct: 0 } }));
      await api.ollamaPull(base, c.name);
    } catch (e) {
      setPulls((s) => { const n = { ...s }; delete n[c.name]; return n; });
      toast(errText(e), "bad");
    }
  };
  const addToApp = async (name: string) => {
    const pid = await ensureProvider();
    const id = await api.addModel(pid, name);
    await refreshModels();
    toast(`Added ${name}. Testing…`, "ok");
    api.testModel(id).then(() => refreshModels()).catch((e) => toast(errText(e), "bad"));
  };

  return (
    <>
      <Card title="This computer" subtitle="Recommendations compare model size with your memory. Ollama unloads idle models automatically." icon={MemoryStick}>
        {hw ? (
          <dl className="kv">
            <dt>Memory</dt><dd>{hw.ram_gb} GB ({hw.ram_free_gb} GB free)</dd>
            <dt>Processor</dt><dd>{hw.cpu || "Unknown"} · {hw.cores} threads</dd>
            <dt>Graphics</dt><dd>{hw.gpus.join(", ") || "Unknown"}</dd>
            <dt>Free disk</dt><dd>{hw.disk_free_gb} GB</dd>
          </dl>
        ) : <Skeleton lines={4} />}
      </Card>
      {running === null && <Progress label="Looking for Ollama…" />}
      {running === false && <OllamaConnect current={base} onConnected={refresh} />}
      {running && (
        <form className="row" style={{ marginTop: 4, alignItems: "flex-start" }} onSubmit={(e) => { e.preventDefault(); const n = custom.trim(); if (n) { pull({ name: n, size_gb: 0, vision: false, tools: false, license: "", role: "", about: "" }); setCustom(""); } }}>
          <div className="grow"><TextField label="Download any model by name" mono value={custom} onChange={setCustom} placeholder="e.g. llama3.1:8b or hf.co/user/model-GGUF" hint={<>Browse names at ollama.com/library. Connected to <span className="mono">{base}</span>.</>} /></div>
          <button className="btn primary" style={{ marginTop: 25 }} disabled={!custom.trim() || !!pulls[custom.trim()]}><Download size={15} /> Download</button>
        </form>
      )}
      {Object.entries(pulls).filter(([n]) => !catalog.some((c) => c.name === n)).map(([n, p]) => (
        <div key={n} style={{ marginTop: 10 }}><Progress value={p.pct || undefined} label={<><span className="mono">{n}</span> · {p.status}</>} /></div>
      ))}
      <div style={{ height: 16 }} />
      <Card title="Recommended open-weight models" icon={Download} flush>
        <table className="list" style={{ marginTop: 12 }}>
          <thead><tr><th>Model</th><th>Good for</th><th>Size</th><th>Fit</th><th>License</th><th></th></tr></thead>
          <tbody>
            {catalog.map((c) => {
              const inst = installed.some((i) => i.name === c.name || i.name === `${c.name}:latest`);
              const p = pulls[c.name];
              const fit = fitOf(c.size_gb);
              return (
                <tr key={c.name}>
                  <td><div className="row"><b className="mono">{c.name}</b>{c.vision && <span className="badge ok">Vision</span>}{c.tools && <span className="badge ok">Tools</span>}</div><div className="small muted">{c.about}</div></td>
                  <td className="small">{c.role}</td>
                  <td className="small">{c.size_gb} GB</td>
                  <td>{fit && <span className={"badge " + (fit === "good" ? "ok" : fit === "tight" ? "warn" : "bad")}>{fit === "good" ? "Runs well" : fit === "tight" ? "Tight fit" : "Too large"}</span>}</td>
                  <td className="small muted">{c.license}</td>
                  <td style={{ minWidth: 150, textAlign: "right" }}>
                    {p ? (
                      <Progress small value={p.pct || undefined} label={p.status} />
                    ) : inst ? (
                      <button className="btn sm" onClick={() => addToApp(c.name)}>Use in app</button>
                    ) : (
                      <button className="btn sm primary" disabled={running !== true || fit === "too-large"} onClick={() => pull(c)}><Download size={14} /> Download</button>
                    )}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </Card>
      {installed.length > 0 && (
        <Card title="Installed in Ollama" icon={HardDrive} flush>
          <table className="list" style={{ marginTop: 12 }}>
            <tbody>
              {installed.map((m) => (
                <tr key={m.name}>
                  <td className="mono">{m.name}</td>
                  <td className="small muted">{(m.size / 1e9).toFixed(1)} GB</td>
                  <td style={{ textAlign: "right" }}>
                    <button className="btn sm" onClick={() => addToApp(m.name)}>Use in app</button>
                    <button className="icon-btn danger" aria-label={`Delete ${m.name}`} title="Delete from disk" onClick={() => setDel(m.name)}><Trash2 size={15} /></button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>
      )}
      {del && (
        <ConfirmDialog title={`Delete ${del}?`} onClose={() => setDel(null)} onConfirm={async () => { try { await api.ollamaDelete(base, del); refresh(); } catch (e) { toast(errText(e), "bad"); } }}>
          The model files are removed from disk. You can download it again later.
        </ConfirmDialog>
      )}
    </>
  );
}

/** Shown when Ollama is not found: install link, or point the app at Ollama on another address or computer. */
function OllamaConnect({ current, onConnected }: { current: string; onConnected: () => void }) {
  const { toast, refreshModels } = useApp();
  const [addr, setAddr] = useState(current);
  const [busy, setBusy] = useState(false);
  const err = v.url(addr);
  const connect = async () => {
    if (err) return;
    setBusy(true);
    try {
      const r = await api.testProvider("ollama", addr);
      const ps = await api.listProviders();
      const existing = ps.find((p) => p.kind === "ollama");
      if (existing) await api.updateProvider(existing.id, existing.name, r.base_url);
      else await api.addProvider("ollama", "Ollama", r.base_url);
      await refreshModels();
      toast(`Connected to Ollama at ${r.base_url} · ${r.models.length} model(s) installed`, "ok");
      onConnected();
    } catch (e) {
      toast(errText(e), "bad");
    }
    setBusy(false);
  };
  return (
    <Card title="Connect to Ollama" subtitle="Downloads and local models run through Ollama, which is free." icon={Plug}>
      <Callout kind="warn">Ollama did not answer at <span className="mono">{current}</span>. Install it from ollama.com and start it, or enter the address where it runs (another port, or another computer on your network with OLLAMA_HOST=0.0.0.0).</Callout>
      <form className="row" style={{ marginTop: 12, alignItems: "flex-start" }} onSubmit={(e) => { e.preventDefault(); connect(); }}>
        <div className="grow"><TextField label="Ollama address" mono value={addr} onChange={setAddr} error={addr ? err : undefined} placeholder="http://127.0.0.1:11434" hint={v.insecureWarning(addr)} /></div>
        <button className="btn primary" style={{ marginTop: 25 }} disabled={busy || !!err}>{busy ? <Spinner /> : <Plug size={15} />} Connect</button>
        <button type="button" className="btn" style={{ marginTop: 25 }} onClick={() => api.openLink("https://ollama.com/download")}><Download size={15} /> Get Ollama</button>
      </form>
    </Card>
  );
}
