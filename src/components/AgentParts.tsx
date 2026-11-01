// Pieces shared by the chat, Activity and the floating bar to show what the agent did.
import { useEffect, useState } from "react";
import { Check, Circle, CircleDot, Code2, EyeOff, Image as ImageIcon, ShieldAlert, Square } from "lucide-react";
import { api, errText, money, type FastChecks, type Step } from "../api";
import { Spinner, useApp } from "./common";

/** Arguments as sent, without the engine's own bookkeeping keys (they start with "_"). */
export function requestOf(args: unknown): Record<string, unknown> | null {
  if (!args || typeof args !== "object") return null;
  const out = Object.fromEntries(Object.entries(args as Record<string, unknown>).filter(([k]) => !k.startsWith("_")));
  return Object.keys(out).length ? out : null;
}

export function fmtMs(ms?: number) {
  if (ms == null) return "";
  return ms < 1000 ? `${ms} ms` : `${(ms / 1000).toFixed(ms < 10_000 ? 1 : 0)} s`;
}

/** Request and response of one tool call, as the agent or model saw them. */
export function CallDetail({ args, result }: { args: unknown; result?: string }) {
  const req = requestOf(args);
  return (
    <div className="call-detail">
      <div className="call-label">Request</div>
      <pre className="call-pre">{req ? JSON.stringify(req, null, 2) : "(no arguments)"}</pre>
      <div className="call-label">Response</div>
      <pre className="call-pre">{result ? result : "(none yet)"}</pre>
    </div>
  );
}

/** Elapsed seconds for a running call, updated each second. */
export function useElapsed(started?: number, running = true) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    if (!running) return;
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, [running]);
  return started ? Math.max(0, Math.round((now - started) / 1000)) : 0;
}

/** Cancels one running call. The reply or task carries on without it. */
export function CancelCall({ callId, label = "Cancel" }: { callId?: string; label?: string }) {
  const { toast } = useApp();
  const [busy, setBusy] = useState(false);
  if (!callId) return null;
  return (
    <button className="btn sm subtle danger-text" disabled={busy} title="Cancel this tool call only; the rest carries on"
      onClick={(e) => {
        e.preventDefault();
        e.stopPropagation();
        setBusy(true);
        api.cancelTool(callId).then((ok) => { if (!ok) toast("That call had already finished.", "info"); }).catch((x) => toast(errText(x), "bad"));
      }}>
      <Square size={11} fill="currentColor" /> {label}
    </button>
  );
}

/** The approved plan with the current step marked. */
export function PlanList({ plan, current, compact }: { plan: string[]; current: number; compact?: boolean }) {
  if (!plan.length) return null;
  return (
    <ol className={"plan-list" + (compact ? " compact" : "")} aria-label="Plan">
      {plan.map((p, i) => {
        const n = i + 1;
        const state = current === 0 ? "todo" : n < current ? "done" : n === current ? "now" : "todo";
        return (
          <li key={i} className={state} aria-current={state === "now" ? "step" : undefined}>
            {state === "done" ? <Check size={13} /> : state === "now" ? <CircleDot size={13} /> : <Circle size={13} />}
            <span>{p}</span>
          </li>
        );
      })}
    </ol>
  );
}

/** A screenshot the agent took, loaded on demand. */
export function StepShot({ path, alt, onZoom, auto }: { path: string; alt: string; onZoom?: (src: string) => void; auto?: boolean }) {
  const [img, setImg] = useState<string | null>(null);
  const [show, setShow] = useState(!!auto);
  useEffect(() => {
    setImg(null);
  }, [path]);
  useEffect(() => {
    if (show && path && img === null) api.readScreenshot(path).then((b) => setImg(`data:image/jpeg;base64,${b}`)).catch(() => setImg(""));
  }, [show, path, img]);
  if (!path) return null;
  if (auto) return img ? <img className="shot" src={img} alt={alt} onClick={() => onZoom?.(img)} /> : img === "" ? null : <div className="shot shot-loading"><Spinner size={14} /></div>;
  return (
    <div>
      <button className="btn sm subtle" style={{ marginTop: 4, paddingLeft: 0 }} onClick={() => setShow(!show)}>{show ? <EyeOff size={13} /> : <ImageIcon size={13} />} {show ? "Hide" : "Show"} screenshot</button>
      {show && img && <img className="shot" src={img} alt={alt} onClick={() => onZoom?.(img)} />}
      {show && img === "" && <div className="small muted">Screenshot expired (retention setting).</div>}
    </div>
  );
}

/** One recorded step: what the agent saw, decided and did, with the tool request and response. */
export function StepRow({ s, onZoom }: { s: Step; onZoom: (src: string) => void }) {
  const riskBadge = s.risk === "gate" || s.risk === "blocked" ? "bad" : s.risk === "risky" ? "warn" : "";
  const ms = typeof s.action?._ms === "number" ? (s.action._ms as number) : undefined;
  const hasCall = s.kind === "action" || s.kind === "denied" || s.kind === "blocked" || s.kind === "done" || s.kind === "question";
  return (
    <div className="step">
      <div className="n small">{s.idx}</div>
      <div style={{ minWidth: 0 }}>
        <div className="row wrap">
          <b className="grow">{s.title}</b>
          <span className="badge">{s.kind}</span>
          {ms != null && <span className="badge" title="How long the tool ran">{fmtMs(ms)}</span>}
          {s.risk && s.risk !== "routine" && s.risk !== "observe" && <span className={"badge " + riskBadge}><ShieldAlert size={11} /> {s.risk}</span>}
          {s.cost > 0 && <span className="badge">{money(s.cost)}</span>}
        </div>
        {s.detail && <div className="small muted" style={{ whiteSpace: "pre-wrap", marginTop: 4 }}>{s.detail}</div>}
        {s.result && <div className="small" style={{ marginTop: 4, whiteSpace: "pre-wrap" }}>→ {s.result.length > 400 ? s.result.slice(0, 400) + "…" : s.result}</div>}
        {hasCall && (requestOf(s.action) || s.result) && (
          <details className="drawer tool step-call">
            <summary><Code2 size={13} /> <span>Request and response</span></summary>
            <div className="tool-body"><CallDetail args={s.action} result={s.result} /></div>
          </details>
        )}
        {s.screenshot && <StepShot path={s.screenshot} alt={`What the agent saw at step ${s.idx}`} onZoom={onZoom} />}
      </div>
    </div>
  );
}

const FAST_LABEL: Record<string, string> = { loading: "loading", last_action: "last action", goal_done: "done", plan_step: "plan step", sign_in: "sign-in" };

/** One line with the fast decision model's latest answers; confident ones are highlighted. */
export function FastLine({ fast }: { fast?: FastChecks }) {
  if (!fast) return null;
  if (fast.error) return <div className="fast-line muted" title={fast.error}>Fast checks {fast.paused ? "paused for this task" : "unavailable"}: {fast.error}</div>;
  if (!fast.items?.length) return null;
  return (
    <div className="fast-line" title="Answers from the fast decision model. Highlighted ones were confident enough to use.">
      <span className="agent-now-label">Fast</span>
      {fast.items.map((i) => (
        <span key={i.name} className={"badge" + (i.sure ? " accent" : "")}>{FAST_LABEL[i.name] ?? i.name}: {i.decision} {Math.round(i.confidence * 100)}%</span>
      ))}
      {fast.ms != null && <span className="muted small">{fast.ms} ms</span>}
    </div>
  );
}
