import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Check, Cpu, Database, Download, Eye, FileBox, FlaskConical, FolderOpen, FolderPlus, Gauge, MonitorCog, Pencil, Play, Plus, RefreshCw, ScrollText, Search, Server, Settings2, SlidersHorizontal, Sparkles, Square, Trash2 } from "lucide-react";
import { api, errText, on, type GgufInfo, type LlamaCfg, type LlamaDevice, type LlamaModel, type LlamaStatus } from "../api";
import { Callout, Card, Checkbox, ConfirmDialog, Empty, Field, Modal, NumberField, Pagination, Progress, Seg, Select, SelectField, Skeleton, Slider, Spinner, TextField, ToggleRow, usePaged, useApp } from "../components/common";

const gb = (n: number) => (n >= 1e9 ? `${(n / 1e9).toFixed(1)} GB` : `${Math.max(1, Math.round(n / 1e6))} MB`);
const base = (p: string) => p.split(/[\\/]/).pop() ?? p;

/** Built-in llama.cpp: download the engine, import GGUF models (with vision projectors), run them locally. */
export default function LlamaEngine() {
  const { settings, setSettings, toast, models, refreshModels } = useApp();
  const [st, setSt] = useState<LlamaStatus | null>(null);
  const [variants, setVariants] = useState<Awaited<ReturnType<typeof api.llamaVariants>>>([]);
  const [variant, setVariant] = useState("");
  const [dl, setDl] = useState<{ stage: string; done: number; total: number } | null>(null);
  const [importing, setImporting] = useState<string | null>(null);
  const [editing, setEditing] = useState<LlamaModel | null>(null);
  const [removing, setRemoving] = useState<LlamaModel[] | null>(null);
  const [sel, setSel] = useState<Set<string>>(new Set());
  const [uninstall, setUninstall] = useState(false);
  const [log, setLog] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [q, setQ] = useState("");
  const cfg = settings.llama;

  useEffect(() => {
    api.llamaStatus().then(setSt).catch((e) => toast(errText(e), "bad"));
    api.llamaVariants().then((v) => { setVariants(v); setVariant((v.find((x) => x.recommended) ?? v[0])?.id ?? ""); });
    const offs = [
      on<LlamaStatus>("llama-status", setSt),
      on<{ stage: string; done: number; total: number }>("llama-progress", setDl),
    ];
    return () => offs.forEach((f) => f());
  }, []);

  const install = async (id = variant) => {
    setBusy("install");
    setDl({ stage: "download", done: 0, total: variants.find((v) => v.id === id)?.size ?? 1 });
    try {
      setSt(await api.llamaInstall(id));
      await refreshModels();
      toast("llama.cpp is installed and verified", "ok");
    } catch (e) {
      toast(errText(e), "bad");
    }
    setDl(null);
    setBusy(null);
  };

  const pickModel = async () => {
    const f = await open({ multiple: false, directory: false, filters: [{ name: "GGUF model", extensions: ["gguf"] }] });
    if (typeof f === "string") setImporting(f);
  };
  const pickFolder = async () => {
    const d = await open({ directory: true, multiple: false });
    if (typeof d !== "string") return;
    setBusy("folder");
    try {
      const r = await api.llamaImportFolder(d);
      await refreshModels();
      toast(r.added.length ? `Imported ${r.added.length} model(s)${r.skipped.length ? `; ${r.skipped.length} skipped` : ""}` : r.skipped[0] ?? "No GGUF models found in that folder", r.added.length ? "ok" : "bad");
    } catch (e) {
      toast(errText(e), "bad");
    }
    setBusy(null);
  };
  const load = async (m: LlamaModel) => {
    setBusy(m.id);
    try {
      setSt(await api.llamaLoad(m.id));
      toast(`${m.id} is loaded`, "ok");
    } catch (e) {
      toast(errText(e).split("\n")[0], "bad");
    }
    setBusy(null);
  };
  const appModel = (m: LlamaModel) => models.find((x) => x.provider_kind === "llamacpp" && x.name === m.id);
  const test = async (m: LlamaModel) => {
    const am = appModel(m);
    if (!am) return;
    setBusy("test:" + m.id);
    try {
      await api.testModel(am.id);
      await refreshModels();
      toast(`${m.id} tested`, "ok");
    } catch (e) {
      toast(errText(e).split("\n")[0], "bad");
    }
    setBusy(null);
  };

  const shown = cfg.models.filter((m) => (m.id + m.path + m.arch).toLowerCase().includes(q.toLowerCase()));
  const pg = usePaged(shown, 10, q);
  const stateLabel = st?.state === "ready" ? "Running" : st?.state === "loading" ? "Loading model…" : st?.state === "error" ? "Failed" : "Stopped";

  if (!st) return <Card><Skeleton lines={4} /></Card>;

  return (
    <>
      <Card title="Built-in llama.cpp engine" icon={Cpu}
        subtitle="Run GGUF models directly in the app, including vision models with a projector (mmproj). Nothing else to install; models stay where they are on disk."
        actions={st.installed ? <span className="badge ok">Installed · {st.tag}</span> : undefined}>
        {!st.installed ? (
          <div className="stack">
            <div className="grid2">
              <SelectField label="Build" value={variant} onChange={setVariant} hint={variants.find((v) => v.id === variant)?.about}
                options={variants.map((v) => ({ value: v.id, label: `${v.label}${v.recommended ? " (recommended)" : ""}`, hint: gb(v.size) }))} />
              <Field label=" ">
                <button className="btn primary" disabled={!variant || busy === "install"} onClick={() => install()}>
                  {busy === "install" ? <Spinner /> : <Download size={15} />} Download engine ({gb(variants.find((v) => v.id === variant)?.size ?? 0)})
                </button>
              </Field>
            </div>
            {dl && <Progress value={dl.stage === "unpack" ? undefined : (dl.done / dl.total) * 100} label={dl.stage === "unpack" ? "Unpacking…" : `Downloading and verifying · ${gb(dl.done)} of ${gb(dl.total)}`} />}
            {variants.length === 0 && <Callout kind="warn">No prebuilt engine is available for this kind of computer.</Callout>}
            <div className="small muted">Official llama.cpp release {st.tag}. Every file is checked against the SHA-256 checksum GitHub publishes for it.</div>
          </div>
        ) : (
          <div className="stack">
            <div className="vm-stats">
              <div className="vm-stat"><small>Status</small><b><span className={"dot " + (st.state === "ready" ? "live" : st.state === "error" ? "bad" : st.state === "loading" ? "warn" : "")} />{stateLabel}</b></div>
              <div className="vm-stat"><small>Loaded model</small><b className="ellipsis" title={st.current ?? ""}>{st.current ?? "None"}</b></div>
              <div className="vm-stat"><small>Build</small><b>{variants.find((v) => v.id === st.variant)?.label ?? st.variant}</b></div>
              <div className="vm-stat"><small>Address</small><b className="mono" style={{ fontSize: 12.5 }}>127.0.0.1:{st.port}</b></div>
              <div className="vm-stat"><small>Embedding server</small><b className="ellipsis" title={st.embed_current ?? ""}><span className={"dot " + (st.embed_state === "ready" ? "live" : st.embed_state === "loading" ? "warn" : st.embed_state === "error" ? "bad" : "")} />{st.embed_current ?? "Idle"}</b></div>
            </div>
            {st.state === "loading" && <Progress label="Loading the model into memory…" />}
            {st.state === "error" && <Callout kind="bad" action={<button className="btn sm" onClick={async () => setLog(await api.llamaLog())}><ScrollText size={14} /> Log</button>}><pre className="mono" style={{ margin: 0, whiteSpace: "pre-wrap" }}>{st.error}</pre></Callout>}
            <div className="row wrap">
              {st.current && <button className="btn" onClick={() => api.llamaUnload()}><Square size={13} fill="currentColor" /> Unload</button>}
              <button className="btn" onClick={async () => setLog(await api.llamaLog())}><ScrollText size={15} /> Server log</button>
              <span className="grow" />
              <Select width={210} value={variant} onChange={setVariant} label="Build" options={variants.map((v) => ({ value: v.id, label: v.label, hint: gb(v.size) }))} />
              <button className="btn" disabled={busy === "install" || variant === st.variant} onClick={() => install()}>{busy === "install" ? <Spinner /> : <Download size={15} />} Switch build</button>
              <button className="btn subtle danger" onClick={() => setUninstall(true)}><Trash2 size={14} /> Remove</button>
            </div>
            {dl && <Progress value={dl.stage === "unpack" ? undefined : (dl.done / dl.total) * 100} label={dl.stage === "unpack" ? "Unpacking…" : `Downloading · ${gb(dl.done)} of ${gb(dl.total)}`} />}
          </div>
        )}
      </Card>

      <Card title="Your GGUF models" icon={FileBox} flush
        actions={
          <div className="row">
            <button className="btn" onClick={pickFolder} disabled={busy === "folder"}>{busy === "folder" ? <Spinner /> : <FolderPlus size={15} />} Import folder</button>
            <button className="btn primary" onClick={pickModel}><Plus size={15} /> Import model</button>
          </div>
        }>
        {cfg.models.length === 0 ? (
          <Empty icon={FileBox} title="No models imported yet">
            Import a .gguf file from Hugging Face or LM Studio. For vision models, also pick the matching mmproj file; it is found automatically when it sits next to the model.
          </Empty>
        ) : (
          <>
            {cfg.models.length > 5 && <div style={{ padding: "12px 16px 0" }}><div className="input-group"><Search size={15} className="lead" /><input className="input" placeholder={`Search ${cfg.models.length} models`} value={q} onChange={(e) => setQ(e.target.value)} /></div></div>}
            {sel.size > 0 && (
              <div className="bulk-bar" style={{ margin: "12px 16px 0" }}>
                <b>{sel.size} selected</b>
                <button className="btn sm danger" onClick={() => setRemoving(cfg.models.filter((m) => sel.has(m.id)))}><Trash2 size={14} /> Remove from app</button>
                <span className="grow" />
                <button className="btn sm subtle" onClick={() => setSel(new Set())}>Clear selection</button>
              </div>
            )}
            <table className="list" style={{ marginTop: 12 }}>
              <thead><tr><th className="sel"><Checkbox checked={pg.slice.length > 0 && pg.slice.every((m) => sel.has(m.id))} onChange={(x) => setSel((s) => { const n = new Set(s); pg.slice.forEach((m) => (x ? n.add(m.id) : n.delete(m.id))); return n; })} /></th><th>Model</th><th>Type</th><th>Size</th><th style={{ textAlign: "right" }}></th></tr></thead>
              <tbody>
                {pg.slice.map((m) => {
                  const loaded = st.current === m.id && st.state === "ready";
                  const am = appModel(m);
                  return (
                    <tr key={m.id} className={sel.has(m.id) ? "selected" : undefined}>
                      <td className="sel"><Checkbox checked={sel.has(m.id)} onChange={(x) => setSel((s) => { const n = new Set(s); if (x) n.add(m.id); else n.delete(m.id); return n; })} /></td>
                      <td style={{ maxWidth: 380 }}>
                        <div className="row"><b className="mono">{m.id}</b>{loaded && <span className="badge ok">Loaded</span>}{st.embed_current === m.id && st.embed_state === "ready" && <span className="badge ok">Embedding server</span>}</div>
                        <div className="small muted ellipsis" title={m.path}>{m.path}</div>
                        {m.mmproj && <div className="small muted ellipsis" title={m.mmproj}><Eye size={11} style={{ verticalAlign: -1 }} /> {base(m.mmproj)}</div>}
                      </td>
                      <td>
                        <div className="row wrap" style={{ gap: 4 }}>
                          {m.arch && <span className="badge">{m.arch}</span>}
                          {m.mmproj && <span className="badge ok">Vision</span>}
                          {(m.embedding || am?.effective.embedding) && <span className="badge accent"><Database size={11} /> Embedding</span>}
                          {am?.effective.tools && <span className="badge ok">Tools</span>}
                        </div>
                        <div className="small muted">{m.ctx ? `${m.ctx.toLocaleString()} ctx` : `${Math.min(cfg.ctx_size, m.train_ctx || cfg.ctx_size).toLocaleString()} ctx`}{m.train_ctx ? ` · trained ${m.train_ctx.toLocaleString()}` : ""}</div>
                      </td>
                      <td className="small" style={{ whiteSpace: "nowrap" }}>{gb(m.size)}</td>
                      <td style={{ textAlign: "right", whiteSpace: "nowrap" }}>
                        {!loaded && !(m.embedding || am?.effective.embedding) && <button className="btn sm" disabled={!st.installed || !!busy} onClick={() => load(m)}>{busy === m.id ? <Spinner /> : <Play size={13} />} Load</button>}
                        <button className="btn sm" disabled={!st.installed || !am || !!busy} onClick={() => test(m)} title="Checks tool calling, image reading and speed">{busy === "test:" + m.id ? <Spinner /> : <FlaskConical size={13} />} Test</button>
                        <button className="icon-btn" aria-label={`Edit ${m.id}`} title="Projector and context" onClick={() => setEditing(m)}><Pencil size={15} /></button>
                        <button className="icon-btn danger" aria-label={`Remove ${m.id}`} title="Remove from app" onClick={() => setRemoving([m])}><Trash2 size={15} /></button>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
            <div style={{ padding: "0 16px 14px" }}><Pagination page={pg.page} pages={pg.pages} total={pg.total} pageSize={pg.pageSize} onPage={pg.setPage} /></div>
          </>
        )}
        {!st.installed && cfg.models.length > 0 && <div style={{ padding: "0 16px 16px" }}><Callout kind="warn">Download the engine above to run these models.</Callout></div>}
      </Card>

      <EmbeddingCard installed={st.installed} />

      {st.installed && <EngineSettings cfg={cfg} onSave={(c) => setSettings({ ...settings, llama: c })} gpuBuild={!/cpu/i.test(st.variant)} current={st.state === "ready" ? st.current : null} />}

      {importing && <ImportDialog path={importing} onClose={() => setImporting(null)} onDone={async () => { setImporting(null); await refreshModels(); }} />}
      {editing && <EditDialog m={editing} onClose={() => setEditing(null)} />}
      {removing && (
        <ConfirmDialog title={removing.length === 1 ? `Remove ${removing[0].id}?` : `Remove ${removing.length} models?`} confirmLabel="Remove" onClose={() => setRemoving(null)}
          onConfirm={async () => { try { for (const m of removing) await api.llamaRemoveModel(m.id); setSel(new Set()); await refreshModels(); toast("Removed from the app. The files are still on disk.", "ok"); } catch (e) { toast(errText(e), "bad"); } }}>
          {removing.length === 1 ? "It is" : "They are"} removed from the app and chats keep their history. GGUF files on disk are not deleted.
        </ConfirmDialog>
      )}
      {uninstall && (
        <ConfirmDialog title="Remove the llama.cpp engine?" confirmLabel="Remove" onClose={() => setUninstall(false)} onConfirm={async () => { await api.llamaUninstall(); toast("Engine removed", "ok"); }}>
          The engine files are deleted. Your imported models stay listed and your GGUF files are untouched; download the engine again to use them.
        </ConfirmDialog>
      )}
      {log !== null && (
        <Modal title="Server log" subtitle="The last 200 lines from llama-server." icon={ScrollText} wide onClose={() => setLog(null)} footer={<button className="btn primary" onClick={() => setLog(null)}>Close</button>}>
          <pre className="term" style={{ maxHeight: 420 }}>{log || "(empty)"}</pre>
        </Modal>
      )}
    </>
  );
}

function ImportDialog({ path, onClose, onDone }: { path: string; onClose: () => void; onDone: () => void }) {
  const { toast } = useApp();
  const [info, setInfo] = useState<GgufInfo | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [mmproj, setMmproj] = useState("");
  const [ctx, setCtx] = useState(0);
  const [saving, setSaving] = useState(false);
  useEffect(() => {
    api.llamaInspect(path).then((r) => { setInfo(r.info); if (r.projector) setMmproj(r.projector); }).catch((e) => setErr(errText(e)));
  }, [path]);
  const pickProjector = async () => {
    const f = await open({ multiple: false, filters: [{ name: "Vision projector (mmproj GGUF)", extensions: ["gguf"] }] });
    if (typeof f === "string") setMmproj(f);
  };
  const save = async () => {
    setSaving(true);
    try {
      await api.llamaImport(path, mmproj, ctx);
      toast(`Imported ${base(path)}. Load or test it from the list.`, "ok");
      onDone();
    } catch (e) {
      setErr(errText(e));
    }
    setSaving(false);
  };
  return (
    <Modal title="Import a GGUF model" icon={FileBox} onClose={onClose}
      footer={<><button className="btn" onClick={onClose}>Cancel</button><button className="btn primary" disabled={!info || info.is_projector || saving} onClick={save}>{saving ? <Spinner /> : <Plus size={15} />} Import</button></>}>
      <div className="stack">
        <Field label="Model file"><div className="mono small ellipsis" title={path}>{path}</div></Field>
        {!info && !err && <Skeleton lines={2} />}
        {info && (
          <dl className="kv">
            <dt>Name</dt><dd>{info.name || base(path)}</dd>
            <dt>Architecture</dt><dd>{info.arch || "Unknown"}</dd>
            <dt>Size</dt><dd>{gb(info.size)}</dd>
            {info.context_length > 0 && <><dt>Trained context</dt><dd>{info.context_length.toLocaleString()} tokens</dd></>}
          </dl>
        )}
        {info?.is_projector && <Callout kind="warn">This is a vision projector, not a model. Choose the model file and add this one as its projector.</Callout>}
        {err && <Callout kind="bad">{err}</Callout>}
        <Field label="Vision projector (optional)" hint={mmproj ? "Found next to the model. The model will be able to read images." : "Only for vision models: the mmproj-*.gguf file published with the model."}>
          <div className="row">
            <input className="input mono grow" value={mmproj} readOnly placeholder="None" aria-label="Vision projector" />
            <button className="btn" onClick={pickProjector}><FolderOpen size={15} /> Browse</button>
            {mmproj && <button className="btn subtle" onClick={() => setMmproj("")}>Clear</button>}
          </div>
        </Field>
        <NumberField label="Context size" hint="0 uses the engine default (Settings below). Larger uses more memory." value={ctx} onCommit={setCtx} min={0} max={1048576} step={1024} />
      </div>
    </Modal>
  );
}

function EditDialog({ m, onClose }: { m: LlamaModel; onClose: () => void }) {
  const { toast } = useApp();
  const [mmproj, setMmproj] = useState(m.mmproj);
  const [ctx, setCtx] = useState(m.ctx);
  const pick = async () => {
    const f = await open({ multiple: false, filters: [{ name: "Vision projector (mmproj GGUF)", extensions: ["gguf"] }] });
    if (typeof f === "string") setMmproj(f);
  };
  return (
    <Modal title={`Edit ${m.id}`} icon={Settings2} onClose={onClose}
      footer={<><button className="btn" onClick={onClose}>Cancel</button><button className="btn primary" onClick={async () => { try { await api.llamaUpdateModel(m.id, mmproj, ctx); toast("Saved. It reloads with these options next time.", "ok"); onClose(); } catch (e) { toast(errText(e), "bad"); } }}>Save</button></>}>
      <div className="stack">
        <Field label="Vision projector">
          <div className="row">
            <input className="input mono grow" value={mmproj} readOnly placeholder="None (text only)" aria-label="Vision projector" />
            <button className="btn" onClick={pick}><FolderOpen size={15} /> Browse</button>
            {mmproj && <button className="btn subtle" onClick={() => setMmproj("")}>Clear</button>}
          </div>
        </Field>
        <NumberField label="Context size" hint={`0 uses the engine default.${m.train_ctx ? ` Trained for ${m.train_ctx.toLocaleString()}.` : ""}`} value={ctx} onCommit={setCtx} min={0} max={1048576} step={1024} />
      </div>
    </Modal>
  );
}

const CACHE_TYPES = ["f16", "bf16", "q8_0", "q5_1", "q5_0", "q4_1", "q4_0", "iq4_nl", "f32"].map((v) => ({ value: v, label: v === "f16" ? "f16 (default)" : v, hint: v.startsWith("q") || v.startsWith("iq") ? "Saves memory" : undefined }));

/** A nullable number: "Model default" or a value. */
function Optional({ label, hint, value, onChange, min, max, step, def }: { label: string; hint: string; value: number | null; onChange: (v: number | null) => void; min: number; max: number; step: number; def: number }) {
  return (
    <div className="opt-field">
      <div className="row" style={{ justifyContent: "space-between" }}>
        <span className="field-label">{label}</span>
        <Checkbox checked={value == null} onChange={(x) => onChange(x ? null : def)}>Model default</Checkbox>
      </div>
      {value == null ? <div className="field-hint">{hint}</div> : <Slider label={label} value={value} min={min} max={max} step={step} onChange={onChange} format={(v) => (step < 1 ? v.toFixed(2) : String(v))} />}
    </div>
  );
}

function EngineSettings({ cfg, onSave, gpuBuild, current }: { cfg: LlamaCfg; onSave: (c: LlamaCfg) => Promise<boolean>; gpuBuild: boolean; current: string | null }) {
  const { toast } = useApp();
  const [c, setC] = useState(cfg);
  const [devices, setDevices] = useState<LlamaDevice[] | null>(null);
  const [scanning, setScanning] = useState(false);
  const [tab, setTab] = useState<"hardware" | "performance" | "sampling" | "server">("hardware");
  useEffect(() => setC(cfg), [cfg]);
  const scan = async (refresh: boolean) => {
    setScanning(true);
    try { setDevices(await api.llamaDevices(refresh)); } catch (e) { setDevices([]); if (refresh) toast(errText(e), "bad"); }
    setScanning(false);
  };
  useEffect(() => { scan(false); }, []);
  const argsErr = /(^|\s)(-m|--model|--host|--port|-a|--alias|--mmproj|--api-key)(\s|$)/.test(c.extra_args) ? "Model, address and projector are set by the app" : /[&|;`$]/.test(c.extra_args) ? "Use plain arguments only" : undefined;
  const tsErr = c.tensor_split && !/^\s*\d+(\.\d+)?(\s*,\s*\d+(\.\d+)?)*\s*$/.test(c.tensor_split) ? "Numbers separated by commas, e.g. 3,1" : undefined;
  const portErr = c.embed_port === c.port ? "Must differ from the chat port" : undefined;
  const dirty = JSON.stringify({ ...c, models: [] }) !== JSON.stringify({ ...cfg, models: [] });
  const chosen = c.devices === "auto" || c.devices === "none" ? [] : c.devices.split(",");
  const setDevicesSel = (ids: string[]) => setC({ ...c, devices: ids.length ? ids.join(",") : "auto" });
  const save = async () => {
    if (!(await onSave({ ...c, models: cfg.models }))) return;
    if (current) {
      try { await api.llamaUnload(); await api.llamaLoad(current); toast(`Saved and reloaded ${current}`, "ok"); } catch (e) { toast(errText(e).split("\n")[0], "bad"); }
    } else toast("Saved. Applies to the next model load.", "ok");
  };
  const TABS: [typeof tab, string, typeof Cpu][] = [["hardware", "Hardware", MonitorCog], ["performance", "Performance", Gauge], ["sampling", "Sampling", SlidersHorizontal], ["server", "Server", Server]];

  return (
    <Card title="Engine settings" icon={Settings2} subtitle={current ? `Saving reloads ${current} with the new settings.` : "Applies to the next model load."}
      actions={<div className="row">{dirty && <span className="badge warn">Unsaved</span>}<button className="btn sm" disabled={!dirty} onClick={() => setC(cfg)}>Reset</button><button className="btn sm primary" disabled={!dirty || !!argsErr || !!tsErr || !!portErr} onClick={save}>{current ? "Save and reload" : "Save"}</button></div>}>
      <div className="tabs sub" role="tablist">
        {TABS.map(([k, l, I]) => <button key={k} role="tab" aria-selected={tab === k} className={tab === k ? "on" : ""} onClick={() => setTab(k)}><I size={14} /> {l}</button>)}
      </div>

      {tab === "hardware" && (
        <div className="stack">
          <div className="row" style={{ justifyContent: "space-between" }}>
            <div className="field-label">Where the model runs</div>
            <button className="btn sm subtle" onClick={() => scan(true)} disabled={scanning}>{scanning ? <Spinner size={13} /> : <RefreshCw size={13} />} Detect again</button>
          </div>
          {!gpuBuild && <Callout kind="info">The installed build is CPU-only. Switch to the Vulkan build above to use your graphics cards.</Callout>}
          <div className="device-grid">
            <button className={"device" + (c.devices === "auto" ? " on" : "")} onClick={() => setC({ ...c, devices: "auto" })} disabled={!gpuBuild}>
              <Sparkles size={18} /><div><b>Automatic</b><div className="small muted">llama.cpp uses every GPU it finds</div></div>
            </button>
            <button className={"device" + (c.devices === "none" || !gpuBuild ? " on" : "")} onClick={() => setC({ ...c, devices: "none" })}>
              <Cpu size={18} /><div><b>CPU only</b><div className="small muted">Slowest; uses system memory</div></div>
            </button>
            {devices === null && <div className="device"><Spinner /> Detecting graphics cards…</div>}
            {devices?.map((d) => {
              const on = chosen.includes(d.id);
              return (
                <button key={d.id} className={"device" + (on ? " on" : "")} disabled={!gpuBuild} aria-pressed={on}
                  onClick={() => setDevicesSel(on ? chosen.filter((x) => x !== d.id) : [...chosen, d.id])}>
                  <span className="device-check">{on ? <Check size={13} /> : null}</span>
                  <MonitorCog size={18} />
                  <div className="grow" style={{ minWidth: 0 }}>
                    <b className="ellipsis" title={d.name}>{d.name}</b>
                    <div className="small muted">{d.id} · {d.integrated ? "integrated, shares system memory" : "dedicated"}</div>
                    {d.total_mb > 0 && (
                      <div className="mem-bar" title={`${(d.free_mb / 1024).toFixed(1)} GB free of ${(d.total_mb / 1024).toFixed(1)} GB`}>
                        <i style={{ width: `${Math.round(((d.total_mb - d.free_mb) / d.total_mb) * 100)}%` }} />
                        <span>{(d.free_mb / 1024).toFixed(1)} / {(d.total_mb / 1024).toFixed(1)} GB free</span>
                      </div>
                    )}
                  </div>
                </button>
              );
            })}
          </div>
          {devices && devices.length === 0 && gpuBuild && <Callout kind="warn">No graphics card was found by this build. Check your graphics driver, or use CPU only.</Callout>}
          <div className="grid2">
            <div>
              <div className="field-label">Layers on the GPU</div>
              <Seg value={c.gpu_layers < 0 ? "all" : "custom"} label="GPU layers" onChange={(x) => setC({ ...c, gpu_layers: x === "all" ? -1 : 24 })}
                options={[{ value: "all", label: "All (fastest)" }, { value: "custom", label: "Custom" }]} />
              {c.gpu_layers >= 0 ? <Slider label="Layers on GPU" value={c.gpu_layers} min={0} max={200} onChange={(n) => setC({ ...c, gpu_layers: n })} format={(v) => (v === 0 ? "0 (CPU)" : String(v))} />
                : <div className="field-hint">Lower this if the model does not fit in graphics memory; the rest runs on the CPU.</div>}
            </div>
            <NumberField label="MoE experts on CPU" hint="For mixture-of-experts models: keep the experts of the first N layers in system memory to fit larger models. 0 = off." value={c.cpu_moe} onCommit={(n) => setC({ ...c, cpu_moe: n })} min={0} max={999} />
          </div>
          {chosen.length > 1 && (
            <div className="grid2">
              <div>
                <div className="field-label">Split across GPUs</div>
                <Seg value={c.split_mode as "layer" | "row" | "none"} label="Split mode" onChange={(x) => setC({ ...c, split_mode: x })}
                  options={[{ value: "layer", label: "By layer" }, { value: "row", label: "By row" }, { value: "none", label: "One GPU" }]} />
                <div className="field-hint">By layer is the safe default. One GPU uses the main GPU only.</div>
              </div>
              {c.split_mode === "none" ? (
                <SelectField label="Main GPU" value={String(c.main_gpu)} onChange={(x) => setC({ ...c, main_gpu: Number(x) })}
                  options={chosen.map((id, i) => ({ value: String(i), label: devices?.find((d) => d.id === id)?.name ?? id, hint: id }))} />
              ) : (
                <TextField label="Share per GPU" mono value={c.tensor_split} onChange={(x) => setC({ ...c, tensor_split: x })} error={tsErr} placeholder={chosen.map(() => "1").join(",")}
                  hint={`In the order ${chosen.join(", ")}. Empty splits by free memory; 3,1 puts three quarters on the first.`} />
              )}
            </div>
          )}
        </div>
      )}

      {tab === "performance" && (
        <div className="grid2">
          <NumberField label="Default context size" suffix="tokens" value={c.ctx_size} onCommit={(n) => setC({ ...c, ctx_size: n })} min={512} max={1048576} step={1024} hint="Per model sizes win. Larger uses more memory." />
          <div>
            <div className="field-label">Flash attention</div>
            <Seg value={c.flash_attn as "auto" | "on" | "off"} label="Flash attention" onChange={(x) => setC({ ...c, flash_attn: x })} options={[{ value: "auto", label: "Auto" }, { value: "on", label: "On" }, { value: "off", label: "Off" }]} />
            <div className="field-hint">Faster and smaller memory use on most GPUs. Needed for a quantised V cache.</div>
          </div>
          <SelectField label="K cache type" value={c.cache_k} onChange={(x) => setC({ ...c, cache_k: x })} options={CACHE_TYPES} hint="q8_0 halves cache memory with almost no quality loss." />
          <SelectField label="V cache type" value={c.cache_v} onChange={(x) => setC({ ...c, cache_v: x })} options={CACHE_TYPES} hint="Quantised V needs flash attention." />
          <NumberField label="Batch size" hint="0 = default (2048). Larger reads prompts faster, uses more memory." value={c.batch} onCommit={(n) => setC({ ...c, batch: n })} min={0} max={16384} step={256} />
          <NumberField label="Micro-batch size" hint="0 = default (512)." value={c.ubatch} onCommit={(n) => setC({ ...c, ubatch: n })} min={0} max={16384} step={128} />
          <NumberField label="CPU threads" hint="0 lets llama.cpp decide." value={c.threads} onCommit={(n) => setC({ ...c, threads: n })} min={0} max={256} />
          <NumberField label="Parallel requests" hint="0 = automatic. Each slot shares the context." value={c.parallel} onCommit={(n) => setC({ ...c, parallel: n })} min={0} max={64} />
          <ToggleRow label="Lock model in memory (mlock)" hint="Stops the system paging the model out. Needs enough free RAM." checked={c.mlock} onChange={(x) => setC({ ...c, mlock: x })} />
          <ToggleRow label="Load fully instead of mapping (no mmap)" hint="Slower to load, can help on network drives." checked={c.no_mmap} onChange={(x) => setC({ ...c, no_mmap: x })} />
        </div>
      )}

      {tab === "sampling" && (
        <div className="grid2">
          <Optional label="Temperature" hint="Uses the model's own default. Lower is more focused." value={c.temperature} onChange={(v) => setC({ ...c, temperature: v })} min={0} max={2} step={0.05} def={0.7} />
          <Optional label="Top P" hint="Uses the model's own default." value={c.top_p} onChange={(v) => setC({ ...c, top_p: v })} min={0} max={1} step={0.01} def={0.95} />
          <Optional label="Top K" hint="Uses the model's own default." value={c.top_k} onChange={(v) => setC({ ...c, top_k: v })} min={0} max={200} step={1} def={40} />
          <Optional label="Min P" hint="Uses the model's own default." value={c.min_p} onChange={(v) => setC({ ...c, min_p: v })} min={0} max={1} step={0.01} def={0.05} />
          <Optional label="Repeat penalty" hint="Uses the model's own default (off)." value={c.repeat_penalty} onChange={(v) => setC({ ...c, repeat_penalty: v })} min={0.5} max={2} step={0.01} def={1.1} />
          <div className="field-hint" style={{ alignSelf: "end" }}>These are server defaults. A chat's own temperature (Chat settings) still wins.</div>
        </div>
      )}

      {tab === "server" && (
        <div className="grid2">
          <SelectField label="Unload when idle" value={String(c.idle_minutes)} onChange={(x) => setC({ ...c, idle_minutes: Number(x) })} hint="Frees memory; it loads again on the next message."
            options={[{ value: "0", label: "Never" }, { value: "5", label: "After 5 minutes" }, { value: "10", label: "After 10 minutes" }, { value: "30", label: "After 30 minutes" }, { value: "60", label: "After 1 hour" }]} />
          <div className="grid2" style={{ gap: 12 }}>
            <NumberField label="Chat port" value={c.port} onCommit={(n) => setC({ ...c, port: n })} min={1024} max={65535} />
            <NumberField label="Embedding port" value={c.embed_port} onCommit={(n) => setC({ ...c, embed_port: n })} min={1024} max={65535} />
          </div>
          {portErr && <div className="field-error">{portErr}</div>}
          <TextField label="Extra arguments (advanced)" mono value={c.extra_args} onChange={(x) => setC({ ...c, extra_args: x })} error={argsErr} placeholder="e.g. --cache-reuse 256 --swa-full" hint="Passed to llama-server as is, after the settings above." />
        </div>
      )}
    </Card>
  );
}

function EmbeddingCard({ installed }: { installed: boolean }) {
  const { settings, setSettings, models, refreshModels, toast } = useApp();
  const [cat, setCat] = useState<Awaited<ReturnType<typeof api.llamaEmbedCatalog>>>([]);
  const [busy, setBusy] = useState<string | null>(null);
  const [prog, setProg] = useState<{ done: number; total: number } | null>(null);
  const refresh = () => api.llamaEmbedCatalog().then(setCat).catch(() => {});
  useEffect(() => {
    refresh();
    return on<{ stage: string; id?: string; done: number; total: number }>("llama-progress", (p) => { if (p.stage === "model") setProg({ done: p.done, total: p.total }); });
  }, []);
  const embedModels = models.filter((m) => m.effective.embedding);
  const get = async (id: string) => {
    setBusy(id);
    setProg(null);
    try {
      await api.llamaFetchEmbed(id);
      await refreshModels();
      await refresh();
      toast("Embedding model downloaded, verified and ready for documents", "ok");
    } catch (e) {
      toast(errText(e), "bad");
    }
    setBusy(null);
    setProg(null);
  };
  return (
    <Card title="Embeddings for documents" icon={Database}
      subtitle="An embedding model turns documents into searchable vectors, so the chat model reads only the passages that matter. It runs on its own small server next to your chat model.">
      <div className="stack">
        <SelectField label="Embedding model used for documents" value={settings.embed_model ?? ""} onChange={(x) => setSettings({ ...settings, embed_model: x || null })}
          hint={embedModels.length ? "Any model marked Embedding works, including ones from LM Studio or Ollama." : "Download one below, or mark a model as Embedding in Your models."}
          options={[{ value: "", label: "None (documents are sent in full)" }, ...embedModels.map((m) => ({ value: m.id, label: m.label, hint: m.provider_name }))]} />
        <div className="embed-grid">
          {cat.map((m) => (
            <div key={m.id} className={"embed-pick" + (m.installed ? " done" : "")}>
              <div className="grow" style={{ minWidth: 0 }}>
                <b>{m.label}</b>
                <div className="small muted">{m.note} · {gb(m.size)}</div>
              </div>
              {m.installed ? <span className="badge ok"><Check size={12} /> Ready</span> : (
                <button className="btn sm" disabled={!installed || !!busy} onClick={() => get(m.id)} title={installed ? undefined : "Download the engine first"}>{busy === m.id ? <Spinner size={13} /> : <Download size={13} />} Get</button>
              )}
              {busy === m.id && <div style={{ gridColumn: "1 / -1" }}><Progress small value={prog ? (prog.done / prog.total) * 100 : undefined} label={prog ? `${gb(prog.done)} of ${gb(prog.total)}` : "Starting…"} /></div>}
            </div>
          ))}
        </div>
        <div className="small muted">Downloaded from Hugging Face and checked against a pinned SHA-256. You can also import your own embedding GGUF with Import model; it is recognised automatically.</div>
      </div>
    </Card>
  );
}
