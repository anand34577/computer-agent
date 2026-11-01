import { useEffect, useRef, useState } from "react";
import { Boxes, Check, Cpu, Download, FlaskConical, FolderOpen, Play, Plus, Sparkles, Square, Trash2, X, Zap } from "lucide-react";
import { api, errText, on, type DecisionAnswer, type DecisionCfg, type DecisionStatus } from "../api";
import { Callout, Card, ConfirmDialog, NumberField, Progress, Seg, SelectField, SettingRow, Slider, Spinner, TextField, ToggleRow, useApp } from "../components/common";

const CHECKS: { key: keyof DecisionCfg; label: string; hint: string }[] = [
  { key: "check_loading", label: "Wait while loading", hint: "Looks again shortly instead of asking the main model about a half-loaded screen." },
  { key: "check_result", label: "Check each action's result", hint: "Tells the main model when a click did nothing, showed an error or opened a pop-up." },
  { key: "check_done", label: "Notice when the goal is done", hint: "Hints the main model to finish instead of taking more steps." },
  { key: "check_plan", label: "Track the plan step", hint: "Keeps the live plan in the chat and on the bar up to date." },
  { key: "check_signin", label: "Spot sign-in and payment screens", hint: "Hands them to you. It can only add a hand-over, never skip one the built-in rules found." },
];

const mb = (n: number) => (n >= 1e9 ? `${(n / 1e9).toFixed(1)} GB` : `${Math.round(n / 1e6)} MB`);

/** The fast "System 1" decision model: download, server, a try-it demo, and which checks it runs. */
export default function DecisionModel() {
  const { settings, setSettings, toast } = useApp();
  const cfg = settings.decision;
  const [st, setSt] = useState<DecisionStatus | null>(null);
  const [log, setLog] = useState<string[]>([]);
  const [busy, setBusy] = useState<string | null>(null);
  const [progress, setProgress] = useState<Record<string, number>>({});
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [url, setUrl] = useState(cfg.url);
  const [repo, setRepo] = useState(cfg.model_repo);
  const logRef = useRef<HTMLPreElement>(null);
  useEffect(() => setUrl(cfg.url), [cfg.url]);
  useEffect(() => setRepo(cfg.model_repo), [cfg.model_repo]);

  const save = (patch: Partial<DecisionCfg>) => setSettings({ ...settings, decision: { ...cfg, ...patch } });
  const refresh = () => api.decisionStatus().then((s) => { setSt(s); setLog(s.log); }).catch(() => {});

  useEffect(() => {
    refresh();
    const offs = [
      on<string>("decision-log", (l) => setLog((x) => [...x, l].slice(-200))),
      on<DecisionStatus>("decision-status", (s) => { setSt(s); if (!s.setup_running) setProgress({}); }),
      on<{ file: string; done: number; total: number }>("decision-progress", (p) => setProgress((x) => ({ ...x, [p.file]: (p.done / p.total) * 100 }))),
    ];
    const t = setInterval(refresh, 4000);
    return () => { offs.forEach((f) => f()); clearInterval(t); };
  }, []);
  useEffect(() => { if (logRef.current) logRef.current.scrollTop = logRef.current.scrollHeight; }, [log]);

  const run = async (what: string, fn: () => Promise<unknown>) => {
    setBusy(what);
    try { await fn(); } catch (e) { toast(errText(e), "bad"); } finally { setBusy(null); refresh(); }
  };

  const gguf = cfg.backend !== "python";
  const total = (st?.files ?? []).reduce((n, f) => n + f.size, 0);
  const status = !st ? "…" : st.setup_running ? "Downloading…" : st.ready ? `Running on ${st.info?.device ?? "?"} (${st.info?.dtype ?? "?"})` : st.running ? "Loading the model…" : st.installed ? "Downloaded, stopped" : st.local ? "Not downloaded" : "Using a server at the address below";
  const urlErr = /^https?:\/\/[^\s/]+/.test(url) ? undefined : "Enter an address such as http://127.0.0.1:47615";

  return (
    <Card title="Fast decisions (System 1)" icon={Zap}
      subtitle="A small vision model that answers quick yes/no and multiple-choice questions about the screen in one pass, so the main model is asked less often. Only confident answers are used.">
      <ToggleRow label="Use fast decisions in agent tasks" hint="Off: the agent works exactly as before." checked={cfg.enabled} onChange={(x) => save({ enabled: x })} />

      <SettingRow label="How it runs" hint={gguf
        ? "Recommended. The Q8 model (about 1 GB) on the built-in llama.cpp engine, using your GPU. No Python needed."
        : "The model's official engine in its own Python environment with PyTorch (about 4 GB). Slower on CPU; use it to compare or with a CUDA GPU."}>
        <Seg value={cfg.backend} onChange={(x) => { if (st?.running) api.decisionStop().finally(refresh); save({ backend: x }); }} label="How it runs"
          options={[{ value: "gguf", label: "Built-in (GGUF)", icon: Boxes }, { value: "python", label: "Python", icon: Cpu }]} />
      </SettingRow>

      {/* 1. The model files */}
      <div className="section-label">1. Model</div>
      {st?.local === false ? (
        <p className="small muted">Using a server at another address, so nothing is downloaded here.</p>
      ) : (
        <>
          <div className="decision-files">
            {(st?.files ?? []).map((f) => (
              <div key={f.name} className="decision-file">
                <span className="name" title={f.source}>{f.name}</span>
                <span className="row" style={{ gap: 6 }}>
                  <span className="small muted">{mb(f.size)}</span>
                  {f.present ? <span className="badge ok"><Check size={11} /> downloaded</span> : progress[f.name] != null ? <span className="badge accent">{Math.round(progress[f.name])}%</span> : <span className="badge">not downloaded</span>}
                </span>
                {progress[f.name] != null && !f.present && <Progress value={progress[f.name]} small />}
              </div>
            ))}
          </div>
          <div className="row wrap" style={{ gap: 8, marginBottom: 14 }}>
            {!st?.installed && gguf && (st?.engine_installed
              ? <button className="btn primary sm" disabled={!!busy || st?.setup_running} onClick={() => run("setup", () => api.decisionSetup(false))} title="Checked by size and SHA-256 after download">{st?.setup_running ? <Spinner size={13} /> : <Download size={13} />} Download the model ({mb(total)})</button>
              : <Callout kind="info">The built-in engine is needed first: Models → Built-in engine → Download.</Callout>)}
            {!st?.installed && !gguf && (
              <>
                <button className="btn primary sm" disabled={!!busy || st?.setup_running} onClick={() => run("setup", () => api.decisionSetup(true))} title="PyTorch with CUDA, about 2.5 GB, plus the model (1.7 GB)"><Download size={13} /> Download for NVIDIA GPU</button>
                <button className="btn sm" disabled={!!busy || st?.setup_running} onClick={() => run("setup", () => api.decisionSetup(false))} title="PyTorch for CPU, about 250 MB, plus the model (1.7 GB)"><Cpu size={13} /> Download for CPU</button>
              </>
            )}
            {st?.installed && <button className="btn sm" disabled={!!busy || st.setup_running} onClick={() => run("setup", () => api.decisionSetup(st.info?.device === "cuda"))} title="Checks the files and downloads any that are missing or damaged"><Download size={13} /> Check and repair</button>}
            {st?.model_dir && <button className="btn sm subtle" onClick={() => api.reveal(st.model_dir).catch((e) => toast(errText(e), "bad"))}><FolderOpen size={13} /> Open folder</button>}
            {st?.installed && <button className="btn sm subtle danger-text" disabled={!!busy || st.setup_running} onClick={() => setConfirmDelete(true)}><Trash2 size={13} /> Delete model</button>}
          </div>
        </>
      )}

      {/* 2. The server */}
      <div className="section-label">2. Server</div>
      <SettingRow label="Status" hint={st?.last_ms ? `Last decision took ${Math.round(st.last_ms)} ms.` : "Start it here, or let tasks start it."}>
        <div className="row wrap" style={{ gap: 8, justifyContent: "flex-end" }}>
          <span className={"badge " + (st?.ready ? "ok" : st?.running || st?.setup_running ? "accent" : "")}>{(st?.running && !st.ready) || st?.setup_running ? <Spinner size={11} /> : null} {status}</span>
          {st?.local && st.installed && !st.running && <button className="btn primary sm" disabled={!!busy} onClick={() => run("start", () => api.decisionStart())}>{busy === "start" ? <Spinner size={13} /> : <Play size={13} />} Start</button>}
          {st?.local && st.running && <button className="btn sm" disabled={!!busy} onClick={() => run("stop", () => api.decisionStop())}><Square size={12} /> Stop</button>}
        </div>
      </SettingRow>
      {log.length > 0 && (
        <details className="drawer tool" style={{ marginBottom: 14 }}>
          <summary><span>Server log</span></summary>
          <pre className="call-pre decision-log" ref={logRef}>{log.join("\n")}</pre>
        </details>
      )}

      {/* 3. Try it */}
      <div className="section-label">3. Try it</div>
      <TryIt ready={!!st?.ready || st?.local === false} budget={cfg.budget_ms} threshold={cfg.threshold} />

      {/* 4. Settings */}
      <div className="section-label">4. In agent tasks</div>
      <div className="grid2">
        <TextField label="Server address" value={url} onChange={setUrl} onBlur={() => !urlErr && url !== cfg.url && save({ url })} error={urlErr} mono
          hint={gguf ? "A 127.0.0.1 address lets the app run the server itself. Another computer works too: run llama-server there with the same model." : "A 127.0.0.1 address lets the app run the server itself. Another computer works too: run decision_server.py there."} />
        {!gguf && <TextField label="Model" value={repo} onChange={setRepo} onBlur={() => repo !== cfg.model_repo && save({ model_repo: repo.trim() })} mono
          hint="internlm/Intern-Decision-0.8B, or the 2B or 4B versions for more accuracy. Download again after changing." />}
        <SelectField label="Run on" value={cfg.device} onChange={(x) => save({ device: x })}
          hint={gguf ? "GPU uses the same graphics card choice as the built-in engine. Restart the server after changing." : "Auto uses the NVIDIA GPU when PyTorch can see it."}>
          <option value="auto">{gguf ? "GPU" : "Auto"}</option>
          {!gguf && <option value="cuda">NVIDIA GPU</option>}
          <option value="cpu">CPU</option>
        </SelectField>
        <NumberField label="Wait for an answer up to" value={cfg.budget_ms} onCommit={(x) => save({ budget_ms: x })} min={100} max={10000} step={100} suffix="ms"
          hint="A step goes on without the fast checks when they take longer." />
      </div>
      <SettingRow label="Confidence needed" hint="Answers below this are ignored and the main model decides as before. Higher is safer, lower saves more time.">
        <Slider value={cfg.threshold} min={0.5} max={0.99} step={0.01} onChange={(x) => save({ threshold: Math.round(x * 100) / 100 })} label="Confidence needed" format={(x) => `${Math.round(x * 100)}%`} />
      </SettingRow>
      <ToggleRow label="Start with tasks" hint="Starts the local server when a task begins (it takes a few seconds to load)." checked={cfg.autostart} onChange={(x) => save({ autostart: x })} />
      {CHECKS.map((c) => <ToggleRow key={c.key} label={c.label} hint={c.hint} checked={!!cfg[c.key]} onChange={(x) => save({ [c.key]: x } as Partial<DecisionCfg>)} />)}
      <ToggleRow label="Keep a decision log" hint={<>Saved on this computer in <span className="mono">{st?.dir ?? "the app's data folder"}</span>/decisions.jsonl, for checking or fine-tuning later.</>} checked={cfg.log} onChange={(x) => save({ log: x })} />

      {confirmDelete && (
        <ConfirmDialog title="Delete the decision model?" onClose={() => setConfirmDelete(false)} onConfirm={() => run("delete", () => api.decisionDeleteModel())}>
          {gguf ? `The model files (${mb(total)}) are removed from this computer.` : "The model and its Python environment are removed from this computer."} You can download them again any time. Fast decisions stay off until you do.
        </ConfirmDialog>
      )}
    </Card>
  );
}

type TryQ = { name: string; instructions: string; kind: "yes_no" | "choice"; options: string };

const PRESETS: { label: string; qs: TryQ[] }[] = [
  {
    label: "What the agent asks each step",
    qs: [
      { name: "loading", instructions: "Is the screen still loading or changing: a spinner, a progress bar, or a blank or half-drawn page?", kind: "yes_no", options: "" },
      { name: "sign_in", instructions: "Does the screen ask for a password, a sign-in, a CAPTCHA, a verification code or payment card details?", kind: "yes_no", options: "" },
      { name: "goal_done", instructions: "Is the goal fully achieved and visible on this screen?", kind: "yes_no", options: "" },
    ],
  },
  { label: "What app is in front?", qs: [{ name: "app", instructions: "What is mainly shown on the screen?", kind: "choice", options: "web browser, code editor, chat app, file explorer, terminal, something else" }] },
  { label: "Is there a pop-up?", qs: [{ name: "popup", instructions: "Is a dialog, pop-up, menu or notification covering part of the screen?", kind: "yes_no", options: "" }] },
];

/** Ask the model your own questions about the screen and see how sure it is. */
function TryIt({ ready, budget, threshold }: { ready: boolean; budget: number; threshold: number }) {
  const { toast } = useApp();
  const [qs, setQs] = useState<TryQ[]>(PRESETS[0].qs);
  const [context, setContext] = useState("Goal: open YouTube and search for Fahd Mirza");
  const [delay, setDelay] = useState(3);
  const [busy, setBusy] = useState(false);
  const [count, setCount] = useState(0);
  const [res, setRes] = useState<{ ms: number; answers: Record<string, DecisionAnswer>; image: string } | null>(null);

  const ask = async () => {
    setBusy(true);
    setRes(null);
    let n = delay;
    setCount(n);
    const t = setInterval(() => { n -= 1; setCount(Math.max(0, n)); }, 1000);
    try {
      const r = await api.decisionTry(qs.map((q) => ({ name: q.name.trim().replace(/\W+/g, "_") || "q", instructions: q.instructions.trim(), options: q.kind === "choice" ? q.options.split(",").map((o) => o.trim()).filter(Boolean) : null })), context, delay);
      setRes(r);
    } catch (e) {
      toast(errText(e), "bad");
    } finally {
      clearInterval(t);
      setBusy(false);
    }
  };
  const set = (i: number, patch: Partial<TryQ>) => setQs(qs.map((q, j) => (j === i ? { ...q, ...patch } : q)));
  const invalid = qs.some((q) => !q.instructions.trim() || (q.kind === "choice" && q.options.split(",").filter((o) => o.trim()).length < 2));

  return (
    <div className="try-box">
      <div className="row wrap" style={{ gap: 6 }}>
        <span className="small muted">Examples:</span>
        {PRESETS.map((p) => <button key={p.label} className="chip" onClick={() => { setQs(p.qs); setRes(null); }}><Sparkles size={12} /> {p.label}</button>)}
      </div>
      {qs.map((q, i) => (
        <div key={i} className="try-q">
          <input className="input" value={q.instructions} onChange={(e) => set(i, { instructions: e.target.value })} placeholder="Ask about the screen, e.g. Is a cookie banner shown?" aria-label={`Question ${i + 1}`} maxLength={500} />
          <select className="input" value={q.kind} onChange={(e) => set(i, { kind: e.target.value as TryQ["kind"] })} aria-label="Answer type">
            <option value="yes_no">Yes or no</option>
            <option value="choice">Pick one of…</option>
          </select>
          <button className="icon-btn" disabled={qs.length === 1} onClick={() => setQs(qs.filter((_, j) => j !== i))} aria-label="Remove question"><X size={14} /></button>
          {q.kind === "choice" && <input className="input opts" value={q.options} onChange={(e) => set(i, { options: e.target.value })} placeholder="Options, separated by commas" aria-label="Options" />}
        </div>
      ))}
      <div className="row wrap" style={{ gap: 8 }}>
        {qs.length < 8 && <button className="btn sm subtle" onClick={() => setQs([...qs, { name: `q${qs.length + 1}`, instructions: "", kind: "yes_no", options: "" }])}><Plus size={13} /> Add a question</button>}
      </div>
      <input className="input" value={context} onChange={(e) => setContext(e.target.value)} placeholder="Optional context, e.g. the goal" aria-label="Context" maxLength={2000} />
      <div className="row wrap" style={{ gap: 8 }}>
        <button className="btn primary sm" disabled={!ready || busy || invalid} onClick={ask}>
          {busy ? <Spinner size={13} /> : <FlaskConical size={13} />} {busy && count > 0 ? `Switch to the window to check… ${count}` : busy ? "Asking…" : "Ask about my screen"}
        </button>
        <select className="input" style={{ width: "auto" }} value={delay} onChange={(e) => setDelay(Number(e.target.value))} aria-label="Delay before the screenshot">
          <option value={0}>Screenshot now</option>
          <option value={3}>Screenshot in 3 s</option>
          <option value={5}>Screenshot in 5 s</option>
        </select>
        {!ready && <span className="small muted">Start the server first.</span>}
      </div>
      {res && (
        <div className="try-result">
          <div>
            <div className="small muted" style={{ marginBottom: 8 }}>
              Answered in {res.ms} ms{res.ms > budget ? ` (over the ${budget} ms a step waits)` : ""}. Answers at {Math.round(threshold * 100)}% or more would be used in a task.
            </div>
            {qs.map((q, i) => {
              const name = q.name.trim().replace(/\W+/g, "_") || "q";
              const a = res.answers[name];
              if (!a) return null;
              const sure = a.confidence >= threshold;
              return (
                <div key={i} className="try-answer">
                  <div className="q">{q.instructions} <span className={"badge " + (sure ? "ok" : "")}>{a.decision} · {Math.round(a.confidence * 100)}%{sure ? " · would be used" : " · too unsure"}</span></div>
                  {Object.entries(a.probabilities).sort((x, y) => y[1] - x[1]).map(([k, p]) => (
                    <div key={k} className={"prob-row" + (k === a.decision ? " best" : "")}>
                      <span className="ellipsis">{k}</span>
                      <span className="bar"><span style={{ width: `${Math.round(p * 100)}%` }} /></span>
                      <span>{Math.round(p * 100)}%</span>
                    </div>
                  ))}
                </div>
              );
            })}
          </div>
          <img src={`data:image/jpeg;base64,${res.image}`} alt="The screenshot the model looked at" />
        </div>
      )}
    </div>
  );
}
