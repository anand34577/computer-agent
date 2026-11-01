import { useEffect, useState } from "react";
import {
  BookmarkPlus, CheckCircle2, Clock, Download, Eye, EyeOff, History, Image as ImageIcon, ListOrdered, Play, Repeat, Search, ShieldAlert, Trash2, Undo2, X, XCircle,
} from "lucide-react";
import { api, errText, money, on, type Step, type Task } from "../api";
import { Callout, Card, Checkbox, ConfirmDialog, Empty, Pagination, PromptDialog, Skeleton, Spinner, timeAgo, usePaged, useApp } from "../components/common";

const STATUS: Record<string, string> = { done: "ok", failed: "bad", stopped: "warn", interrupted: "warn", running: "accent", resumed: "" };

export default function ActivityView({ onPlaybooksChanged }: { onPlaybooksChanged: () => void }) {
  const { toast } = useApp();
  const [tasks, setTasks] = useState<Task[] | null>(null);
  const [sel, setSel] = useState<string | null>(null);
  const [steps, setSteps] = useState<Step[]>([]);
  const [q, setQ] = useState("");
  const [filter, setFilter] = useState("");
  const [playbooks, setPlaybooks] = useState<{ id: string; name: string; goal: string }[]>([]);
  const [queue, setQueue] = useState<{ id: string; goal: string }[]>([]);
  const [big, setBig] = useState<string | null>(null);
  const [saveAs, setSaveAs] = useState<Task | null>(null);
  const [confirm, setConfirm] = useState<{ kind: "undo" | "delete" | "playbook"; id: string; label: string } | null>(null);
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [bulkDel, setBulkDel] = useState(false);

  const refresh = async () => {
    setTasks(await api.listTasks());
    setPlaybooks(await api.listPlaybooks());
    setQueue((await api.agentStatus()).queue);
  };
  useEffect(() => {
    refresh().catch((e) => toast(errText(e), "bad"));
    const offs = [
      on<Step & { task_id: string }>("agent-step", (s) => {
        setSel((cur) => {
          if (cur === s.task_id) setSteps((st) => [...st, s]);
          return cur;
        });
      }),
      on("agent-finished", () => refresh()),
      on<{ id: string; goal: string }[]>("agent-queue", (qq) => setQueue(qq)),
    ];
    return () => offs.forEach((f) => f());
  }, []);
  useEffect(() => {
    if (sel) api.taskSteps(sel, q || undefined).then(setSteps);
  }, [sel, q]);

  const task = tasks?.find((t) => t.id === sel);
  const [status, setStatus] = useState("");
  const visible = (tasks ?? []).filter((t) => (!filter || t.goal.toLowerCase().includes(filter.toLowerCase())) && (!status || t.status === status));
  const pg = usePaged(visible, 15, filter + status);
  const counts = (tasks ?? []).reduce<Record<string, number>>((m, t) => ({ ...m, [t.status]: (m[t.status] ?? 0) + 1 }), {});

  const runGoal = async (goal: string) => {
    try {
      await api.startTask(goal, null);
      toast("Task started. Watch the floating bar.", "ok");
      setTimeout(refresh, 600);
    } catch (e) {
      toast(errText(e), "bad");
    }
  };

  return (
    <div className="page">
      <div className="page-inner">
        <div className="page-head">
          <div className="grow">
            <h1>Activity</h1>
            <p>A local log of every task: what the agent saw, decided and did. Sensitive fields were masked when captured.</p>
          </div>
        </div>

        {(playbooks.length > 0 || queue.length > 0) && (
          <div className="grid2">
            {playbooks.length > 0 && (
              <Card title="Playbooks" subtitle="Saved tasks you can run in one click." icon={Repeat}>
                {playbooks.map((p) => (
                  <div key={p.id} className="list-row">
                    <div className="grow"><b>{p.name}</b><div className="small muted ellipsis">{p.goal}</div></div>
                    <button className="btn sm primary" onClick={() => runGoal(p.goal)}><Play size={13} /> Run</button>
                    <button className="icon-btn danger" onClick={() => setConfirm({ kind: "playbook", id: p.id, label: p.name })} aria-label={`Delete ${p.name}`}><Trash2 size={15} /></button>
                  </div>
                ))}
              </Card>
            )}
            {queue.length > 0 && (
              <Card title="Waiting in queue" subtitle="Only one task controls the desktop at a time." icon={ListOrdered}>
                {queue.map((t) => (
                  <div key={t.id} className="list-row">
                    <span className="grow ellipsis">{t.goal}</span>
                    <button className="btn sm" onClick={() => api.cancelQueued(t.id)}><X size={13} /> Cancel</button>
                  </div>
                ))}
              </Card>
            )}
          </div>
        )}

        <div className="split">
          <div>
            <div className="input-group" style={{ marginBottom: 8 }}><Search size={15} className="lead" /><input className="input" placeholder="Search tasks" value={filter} onChange={(e) => setFilter(e.target.value)} /></div>
            {tasks && tasks.length > 0 && (
              <div className="row wrap" style={{ gap: 6, marginBottom: 10 }}>
                {["", "done", "failed", "stopped", "interrupted", "running"].filter((x) => !x || counts[x]).map((x) => (
                  <button key={x || "all"} className={"chip" + (status === x ? " on" : "")} onClick={() => setStatus(x)}>{x ? `${x[0].toUpperCase()}${x.slice(1)} · ${counts[x]}` : `All · ${tasks.length}`}</button>
                ))}
              </div>
            )}
            {!tasks && <Skeleton lines={6} />}
            {tasks && tasks.length === 0 && <Card><Empty icon={History} title="No tasks yet">Switch on Agent in a chat and describe what to do.</Empty></Card>}
            {tasks && tasks.length > 0 && visible.length === 0 && <p className="muted small">No tasks match.</p>}
            {pg.slice.length > 0 && (
              <div className={"select-strip" + (picked.size ? " on" : "")}>
                <Checkbox checked={pg.slice.every((t) => picked.has(t.id))} onChange={(x) => setPicked((s) => { const n = new Set(s); pg.slice.forEach((t) => (t.status === "running" ? null : x ? n.add(t.id) : n.delete(t.id))); return n; })}>
                  {picked.size ? `${picked.size} selected` : "Select page"}
                </Checkbox>
                <span className="grow" />
                {picked.size > 0 && <>
                  <button className="btn sm subtle" onClick={() => setPicked(new Set())}>Clear</button>
                  <button className="btn sm danger" onClick={() => setBulkDel(true)}><Trash2 size={13} /> Delete</button>
                </>}
              </div>
            )}
            {pg.slice.map((t) => (
              <div key={t.id} role="button" tabIndex={0} className={"task-item " + (t.id === sel ? "on" : "") + (picked.has(t.id) ? " picked" : "")} onClick={() => setSel(t.id)}
                onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); setSel(t.id); } }}>
                <div className="row" style={{ gap: 8, flexWrap: "nowrap" }}>
                  {t.status !== "running" && <span onClick={(e) => e.stopPropagation()}><Checkbox checked={picked.has(t.id)} onChange={(x) => setPicked((s) => { const n = new Set(s); if (x) n.add(t.id); else n.delete(t.id); return n; })} /></span>}
                  <div className="g ellipsis grow" title={t.goal}>{t.goal}</div>
                </div>
                <div className="row small muted">
                  <span className={"badge " + (STATUS[t.status] ?? "")}>{t.status}</span>
                  <span>{t.steps} steps</span>
                  <span>{money(t.cost)}</span>
                  <span className="grow" />
                  {t.status === "running" ? <Spinner size={12} /> : <Clock size={12} />} <span>{timeAgo(t.started)}</span>
                </div>
              </div>
            ))}
            <Pagination page={pg.page} pages={pg.pages} total={pg.total} pageSize={pg.pageSize} onPage={pg.setPage} />
          </div>
          <div>
            {task ? (
              <Card>
                <div className="card-head">
                  <div className="card-icon">{task.status === "done" ? <CheckCircle2 size={17} /> : task.status === "failed" ? <XCircle size={17} /> : <History size={17} />}</div>
                  <div className="grow">
                    <h2>{task.goal}</h2>
                    <p>
                      {task.models.mode && <>Mode: {task.models.mode} · </>}Doer: {task.models.doer ?? "Unknown"}
                      {task.models.planner && task.models.planner !== task.models.doer ? ` · Planner: ${task.models.planner}` : ""}
                      {task.models.eyes ? ` · Eyes: ${task.models.eyes}` : ""} · {money(task.cost)} · {task.ended ? `${Math.round((task.ended - task.started) / 1000)}s` : "running"}
                    </p>
                  </div>
                </div>
                <div className="row wrap" style={{ marginBottom: 14 }}>
                  {task.status === "interrupted" && <button className="btn primary sm" onClick={async () => { await api.resumeTask(task.id); toast("Resuming. The agent re-checks the screen first.", "ok"); setTimeout(refresh, 600); }}><Play size={13} /> Resume</button>}
                  <button className="btn sm" onClick={() => runGoal(task.goal)}><Repeat size={13} /> Run again</button>
                  <button className="btn sm" onClick={() => setSaveAs(task)}><BookmarkPlus size={13} /> Save as playbook</button>
                  {task.undoable > 0 && <button className="btn sm" onClick={() => setConfirm({ kind: "undo", id: task.id, label: `${task.undoable} file change(s)` })}><Undo2 size={13} /> Undo file changes ({task.undoable})</button>}
                  <button className="btn sm" onClick={async () => { const p = await api.exportTask(task.id); toast(`Exported to ${p}`, "ok"); api.reveal(p); }}><Download size={13} /> Export</button>
                  <span className="grow" />
                  <button className="icon-btn danger" title="Delete log" aria-label="Delete log" onClick={() => setConfirm({ kind: "delete", id: task.id, label: task.goal })}><Trash2 size={15} /></button>
                </div>
                {task.summary && <div style={{ marginBottom: 12 }}><Callout kind={task.status === "done" ? "ok" : "warn"}>{task.summary}</Callout></div>}
                <div className="input-group" style={{ marginBottom: 6 }}><Search size={15} className="lead" /><input className="input" placeholder="Search steps" value={q} onChange={(e) => setQ(e.target.value)} /></div>
                <div className="timeline">{steps.map((s) => <StepRow key={s.id} s={s} onZoom={setBig} />)}</div>
                {steps.length === 0 && <p className="muted">No steps recorded.</p>}
              </Card>
            ) : (
              tasks && tasks.length > 0 && <Card><Empty icon={Eye} title="Select a task">See every step with the screenshot the agent saw.</Empty></Card>
            )}
          </div>
        </div>
      </div>
      {big && <div className="lightbox" onClick={() => setBig(null)}><img src={big} alt="Screenshot" /></div>}
      {saveAs && (
        <PromptDialog title="Save as playbook" label="Playbook name" initial={saveAs.goal.slice(0, 40)} hint="Appears in the command palette and on new chats."
          validate={(x) => (!x.trim() ? "Enter a name" : x.length > 80 ? "Up to 80 characters" : playbooks.some((p) => p.name.toLowerCase() === x.trim().toLowerCase()) ? "That name is taken" : undefined)}
          onSubmit={async (name) => { await api.savePlaybook(name, saveAs.goal); await refresh(); onPlaybooksChanged(); toast("Playbook saved", "ok"); }}
          onClose={() => setSaveAs(null)} />
      )}
      {confirm && (
        <ConfirmDialog
          title={confirm.kind === "undo" ? "Undo file changes?" : confirm.kind === "delete" ? "Delete this task log?" : "Delete playbook?"}
          confirmLabel={confirm.kind === "undo" ? "Undo changes" : "Delete"}
          danger={confirm.kind !== "undo"}
          onClose={() => setConfirm(null)}
          onConfirm={async () => {
            if (confirm.kind === "undo") {
              const r = await api.undoTask(confirm.id);
              toast(`Restored ${r.restored} change(s)${r.problems.length ? `; ${r.problems.length} could not be undone: ${r.problems[0]}` : ""}`, r.problems.length ? "bad" : "ok");
            } else if (confirm.kind === "delete") {
              await api.deleteTask(confirm.id);
              setSel(null);
            } else {
              await api.deletePlaybook(confirm.id);
              onPlaybooksChanged();
            }
            refresh();
          }}
        >
          {confirm.kind === "undo" ? `${confirm.label} will be reversed, newest first. Files changed since then are left in place.` : confirm.kind === "delete" ? "The log and its screenshots are removed. File changes are not undone." : `“${confirm.label}” will be removed.`}
        </ConfirmDialog>
      )}
      {bulkDel && (
        <ConfirmDialog title={`Delete ${picked.size} task${picked.size === 1 ? "" : "s"}?`} onClose={() => setBulkDel(false)} onConfirm={async () => {
          try {
            for (const id of picked) await api.deleteTask(id);
            if (sel && picked.has(sel)) setSel(null);
            toast(`${picked.size} task${picked.size === 1 ? "" : "s"} deleted`, "ok");
            setPicked(new Set());
            refresh();
          } catch (e) { toast(errText(e), "bad"); }
        }}>
          Their steps and screenshots are removed from this computer. The audit log keeps its record.
        </ConfirmDialog>
      )}
    </div>
  );
}

function StepRow({ s, onZoom }: { s: Step; onZoom: (src: string) => void }) {
  const [img, setImg] = useState<string | null>(null);
  const [show, setShow] = useState(false);
  useEffect(() => {
    if (show && s.screenshot && !img) api.readScreenshot(s.screenshot).then((b) => setImg(`data:image/jpeg;base64,${b}`)).catch(() => setImg(""));
  }, [show]);
  const riskBadge = s.risk === "gate" || s.risk === "blocked" ? "bad" : s.risk === "risky" ? "warn" : "";
  return (
    <div className="step">
      <div className="n small">{s.idx}</div>
      <div>
        <div className="row wrap">
          <b className="grow">{s.title}</b>
          <span className="badge">{s.kind}</span>
          {s.risk && s.risk !== "routine" && s.risk !== "observe" && <span className={"badge " + riskBadge}><ShieldAlert size={11} /> {s.risk}</span>}
          {s.cost > 0 && <span className="badge">{money(s.cost)}</span>}
        </div>
        {s.detail && <div className="small muted" style={{ whiteSpace: "pre-wrap", marginTop: 4 }}>{s.detail}</div>}
        {s.result && <div className="small" style={{ marginTop: 4 }}>→ {s.result}</div>}
        {s.screenshot && (
          <div>
            <button className="btn sm subtle" style={{ marginTop: 4, paddingLeft: 0 }} onClick={() => setShow(!show)}>{show ? <EyeOff size={13} /> : <ImageIcon size={13} />} {show ? "Hide" : "Show"} screenshot</button>
            {show && img && <img className="shot" src={img} alt={`What the agent saw at step ${s.idx}`} onClick={() => onZoom(img)} />}
            {show && img === "" && <div className="small muted">Screenshot expired (retention setting).</div>}
          </div>
        )}
      </div>
    </div>
  );
}
