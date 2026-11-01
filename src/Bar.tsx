import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { Cloud, GripVertical, Hand, ListChecks, Lock, MessageCircleQuestion, Pause, Play, Plus, ShieldAlert, Square, Timer, X } from "lucide-react";
import { api, money, on, type AgentState, type Prompt, type Settings } from "./api";
import { applyTheme } from "./theme";

/** Always-on-top floating bar shown while the agent operates the computer. */
export default function Bar() {
  const [state, setState] = useState<AgentState | null>(null);
  const [prompt, setPrompt] = useState<Prompt | null>(null);
  const [paused, setPaused] = useState<string | null>(null);
  const [stopKey, setStopKey] = useState("Ctrl+Shift+F12");
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    api.getSettings().then((s) => {
      applyTheme(s);
      setStopKey(s.stop_shortcut);
    });
    api.agentStatus().then((st) => {
      if (st.status) setState(st.status);
      if (st.prompts.length) setPrompt(st.prompts.find((p) => p.kind !== "tool") ?? null);
    });
    const offs = [
      on<AgentState>("agent-state", (s) => {
        setState(s);
        if (s.status !== "paused") setPaused(null);
      }),
      on<Prompt>("agent-prompt", (p) => setPrompt(p)),
      on<{ id: string }>("agent-prompt-closed", (p) => setPrompt((cur) => (cur?.id === p.id ? null : cur))),
      on<string>("agent-paused", (m) => setPaused(m)),
      on("agent-finished", () => {
        setPrompt(null);
        setPaused(null);
      }),
      on<Settings>("settings-changed", (s) => {
        applyTheme(s);
        setStopKey(s.stop_shortcut);
      }),
    ];
    return () => offs.forEach((f) => f());
  }, []);

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const ro = new ResizeObserver(() => api.setBarSize(Math.ceil(el.offsetHeight) + 10));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const respond = (value: Record<string, unknown>) => {
    if (!prompt) return;
    api.agentRespond(prompt.id, value).catch(() => {});
    setPrompt(null);
  };

  const status = paused ? "paused" : state?.status ?? "starting";
  const busy = ["observing", "thinking", "acting", "planning", "starting", "running"].includes(status);

  return (
    <div className="bar" ref={ref}>
      <div className="bar-line" data-tauri-drag-region>
        <GripVertical size={14} className="muted" data-tauri-drag-region />
        <span className={"dot " + (paused || prompt ? "warn" : busy ? "pulse" : "")} data-tauri-drag-region />
        <div className="bar-msg ellipsis" data-tauri-drag-region title={state?.message}>
          <b data-tauri-drag-region>{state ? `Step ${state.step}` : "Agent"}</b>
          <span className="muted" data-tauri-drag-region> · {paused ?? state?.message ?? "Starting…"}</span>
        </div>
        {state && (
          <span className={"badge " + (state.local ? "ok" : "warn")} title={state.local ? "Everything stays on this computer" : `Screenshots and text go to ${state.doer}`}>
            {state.local ? <Lock size={11} /> : <Cloud size={11} />} {money(state.cost)}
          </span>
        )}
        {paused ? (
          <button className="btn sm" onClick={() => api.agentResume().then(() => setPaused(null))}><Play size={14} /> Resume</button>
        ) : (
          <button className="icon-btn" onClick={() => api.agentPause()} title="Pause after the current action" aria-label="Pause"><Pause size={16} /></button>
        )}
        <button className="bar-stop" onClick={() => api.agentStop()} title={`Emergency stop (${stopKey}). Moving the pointer into the top-left corner pauses.`}>
          <Square size={13} fill="currentColor" /> Stop
        </button>
      </div>
      {prompt && <PromptPanel prompt={prompt} respond={respond} />}
    </div>
  );
}

function PanelHead({ icon: Icon, tone, title, children }: { icon: typeof Hand; tone: "warn" | "bad" | "accent"; title: string; children?: ReactNode }) {
  return (
    <div className="row" style={{ marginBottom: 6 }}>
      <span className={"badge " + tone}><Icon size={12} /> {title}</span>
      <b className="grow ellipsis">{children}</b>
    </div>
  );
}

function PromptPanel({ prompt, respond }: { prompt: Prompt; respond: (v: Record<string, unknown>) => void }) {
  const [steps, setSteps] = useState<string[]>(prompt.steps ?? []);
  const [answer, setAnswer] = useState("");
  useEffect(() => setSteps(prompt.steps ?? []), [prompt.id]);

  if (prompt.kind === "plan") {
    const clean = steps.map((s) => s.trim()).filter(Boolean);
    const tooLong = steps.some((s) => s.length > 300);
    return (
      <div className="bar-panel">
        <PanelHead icon={ListChecks} tone="accent" title="Plan">Review, edit, then approve</PanelHead>
        <div style={{ marginTop: 10 }}>
          {steps.map((s, i) => (
            <div className="plan-step" key={i}>
              <span className="num">{i + 1}.</span>
              <input className={"input" + (s.length > 300 ? " invalid" : "")} value={s} onChange={(e) => setSteps(steps.map((x, j) => (j === i ? e.target.value : x)))} aria-label={`Step ${i + 1}`} />
              <button className="icon-btn" onClick={() => setSteps(steps.filter((_, j) => j !== i))} aria-label="Remove step"><X size={14} /></button>
            </div>
          ))}
          {steps.length < 15 && <button className="btn sm subtle" onClick={() => setSteps([...steps, ""])}><Plus size={14} /> Add step</button>}
        </div>
        {clean.length === 0 && <div className="field-error" style={{ marginTop: 6 }}>Keep at least one step.</div>}
        {tooLong && <div className="field-error" style={{ marginTop: 6 }}>Keep each step under 300 characters.</div>}
        <div className="row end" style={{ marginTop: 10 }}>
          <button className="btn" onClick={() => respond({ decision: "deny" })}>Cancel task</button>
          <button className="btn primary" disabled={clean.length === 0 || tooLong} onClick={() => respond({ decision: "approve", steps: clean })}>Approve and run</button>
        </div>
      </div>
    );
  }
  if (prompt.kind === "approval") {
    const gate = prompt.level === "gate";
    return (
      <div className="bar-panel">
        <PanelHead icon={ShieldAlert} tone={gate ? "bad" : "warn"} title={gate ? "Safety gate" : "Approval"}>{prompt.description}</PanelHead>
        <div className="muted">{prompt.reason}</div>
        {prompt.content && <div className="content mono">{prompt.content}</div>}
        <div className="row end" style={{ marginTop: 10 }}>
          <button className="btn" onClick={() => respond({ decision: "deny" })}>Deny</button>
          {prompt.draftable && <button className="btn" onClick={() => respond({ decision: "self" })} title="Draft-only: you press the final button">I'll press it myself</button>}
          <button className={"btn " + (gate ? "danger" : "primary")} onClick={() => respond({ decision: "approve" })}>Approve once</button>
        </div>
      </div>
    );
  }
  if (prompt.kind === "handoff") {
    return (
      <div className="bar-panel">
        <PanelHead icon={Hand} tone="warn" title="Your turn">Take the wheel</PanelHead>
        <p style={{ margin: "4px 0" }}>{prompt.reason}</p>
        <p className="small muted" style={{ margin: 0 }}>The agent never reads passwords or solves CAPTCHAs.</p>
        <div className="row end" style={{ marginTop: 10 }}>
          <button className="btn" onClick={() => respond({ decision: "stop" })}>Stop task</button>
          <button className="btn primary" onClick={() => respond({ decision: "continue" })}>I'm done, continue</button>
        </div>
      </div>
    );
  }
  if (prompt.kind === "question") {
    const ok = answer.trim().length > 0 && answer.length <= 2000;
    return (
      <div className="bar-panel">
        <PanelHead icon={MessageCircleQuestion} tone="accent" title="Question">The agent needs your input</PanelHead>
        <p style={{ margin: "4px 0 10px" }}>{prompt.question}</p>
        <form className="row" onSubmit={(e) => { e.preventDefault(); if (ok) respond({ answer: answer.trim() }); }}>
          <input className="input" autoFocus value={answer} onChange={(e) => setAnswer(e.target.value)} placeholder="Type your answer" aria-label="Answer" maxLength={2000} />
          <button className="btn primary" disabled={!ok} type="submit">Send</button>
        </form>
      </div>
    );
  }
  if (prompt.kind === "limit") {
    return (
      <div className="bar-panel">
        <PanelHead icon={Timer} tone="warn" title="Limit reached">{prompt.message}</PanelHead>
        <div className="row end" style={{ marginTop: 10 }}>
          <button className="btn" onClick={() => respond({ decision: "deny" })}>Stop here</button>
          <button className="btn primary" onClick={() => respond({ decision: "approve" })}>Allow</button>
        </div>
      </div>
    );
  }
  return null;
}
