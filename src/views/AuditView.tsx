import { useEffect, useMemo, useState } from "react";
import { Bot, Download, FileJson, MessageSquare, RefreshCw, ScrollText, Search, ShieldCheck, ShieldX, User, Wrench, X } from "lucide-react";
import { api, errText, type AuditEntry, type AuditPage, type AuditQuery } from "../api";
import { Callout, Card, Empty, Menu, Pagination, Select, Skeleton, Spinner, useApp } from "../components/common";
import { saveExport } from "../exporters";

const ACTORS = [
  { value: "", label: "Everyone" },
  { value: "user", label: "You", icon: User },
  { value: "ai", label: "AI", icon: Bot },
  { value: "system", label: "App", icon: Wrench },
];
const OUTCOMES = [
  { value: "", label: "Any result" },
  { value: "ok", label: "Succeeded" },
  { value: "error", label: "Failed" },
  { value: "denied", label: "Denied by you" },
  { value: "blocked", label: "Blocked" },
  { value: "stopped", label: "Stopped" },
  { value: "requested", label: "Requested" },
];
const RANGES = [
  { value: "all", label: "All time" },
  { value: "1h", label: "Last hour" },
  { value: "24h", label: "Last 24 hours" },
  { value: "7d", label: "Last 7 days" },
  { value: "30d", label: "Last 30 days" },
];
const SPAN: Record<string, number> = { "1h": 3_600_000, "24h": 86_400_000, "7d": 7 * 86_400_000, "30d": 30 * 86_400_000 };

const tone = (o: string) => (o === "ok" ? "ok" : o === "error" || o === "blocked" ? "bad" : o === "denied" || o === "stopped" ? "warn" : "");
const fmtTime = (ms: number) => new Date(ms).toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit", second: "2-digit" });

/** Every action by you, the AI and the app, with search, filters, pages, export and an integrity check. */
export default function AuditView({ openChat }: { openChat: (id: string) => void }) {
  const { toast } = useApp();
  const [search, setSearch] = useState("");
  const [debounced, setDebounced] = useState("");
  const [category, setCategory] = useState("");
  const [actor, setActor] = useState("");
  const [outcome, setOutcome] = useState("");
  const [range, setRange] = useState("all");
  const [session, setSession] = useState("");
  const [page, setPage] = useState(0);
  const [size, setSize] = useState(50);
  const [data, setData] = useState<AuditPage | null>(null);
  const [loading, setLoading] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [open, setOpen] = useState<number | null>(null);
  const [verify, setVerify] = useState<{ ok: boolean; checked: number; broken_at?: number; reason?: string } | "running" | null>(null);

  useEffect(() => {
    const t = setTimeout(() => setDebounced(search), 250);
    return () => clearTimeout(t);
  }, [search]);
  useEffect(() => setPage(0), [debounced, category, actor, outcome, range, session, size]);

  const query = useMemo<AuditQuery>(() => ({
    search: debounced, category, actor, outcome, session, page, page_size: size,
    from: range === "all" ? null : Date.now() - SPAN[range], to: null,
  }), [debounced, category, actor, outcome, range, session, page, size]);

  const load = async () => {
    setLoading(true);
    setErr(null);
    try {
      setData(await api.auditQuery(query));
    } catch (e) {
      setErr(errText(e));
    }
    setLoading(false);
  };
  useEffect(() => { load(); }, [query]);

  const exportAs = async (fmt: "csv" | "json") => {
    try {
      const text = await api.auditExport({ ...query, page: 0, page_size: 500 }, fmt);
      const path = await saveExport(`audit-log-${new Date().toISOString().slice(0, 10)}`, fmt, text);
      if (path) toast(`Saved to ${path}`, "ok");
    } catch (e) {
      toast(errText(e), "bad");
    }
  };

  const runVerify = async () => {
    setVerify("running");
    try {
      setVerify(await api.auditVerify());
    } catch (e) {
      toast(errText(e), "bad");
      setVerify(null);
    }
  };

  const filtered = !!(debounced || category || actor || outcome || session || range !== "all");
  const clear = () => { setSearch(""); setCategory(""); setActor(""); setOutcome(""); setRange("all"); setSession(""); };
  const cats = [{ value: "", label: "All areas" }, ...(data?.categories ?? []).map((c) => ({ value: c, label: c[0].toUpperCase() + c.slice(1) }))];

  return (
    <div className="page">
      <div className="page-inner" style={{ maxWidth: 1240 }}>
        <div className="page-head">
          <div className="grow">
            <h1>Audit log</h1>
            <p>Every action you, the AI and the app take is recorded here: chats, tool calls, agent steps, approvals, settings and extensions. Secrets are never stored.</p>
          </div>
          <button className="btn" onClick={runVerify} disabled={verify === "running"} title="Check that no record was changed or removed">
            {verify === "running" ? <Spinner /> : <ShieldCheck size={15} />} Verify integrity
          </button>
          <Menu label="Export" trigger={() => <button className="btn"><Download size={15} /> Export</button>}
            items={[
              { label: "CSV (spreadsheet)", icon: Download, onClick: () => exportAs("csv") },
              { label: "JSON", icon: FileJson, onClick: () => exportAs("json") },
            ]} />
        </div>

        {verify && verify !== "running" && (
          <div style={{ marginBottom: 14 }}>
            {verify.ok ? (
              <Callout kind="ok" action={<button className="icon-btn" onClick={() => setVerify(null)} aria-label="Dismiss"><X size={15} /></button>}>
                All {verify.checked.toLocaleString()} records are intact. Each one is chained to the one before it with a SHA-256 hash.
              </Callout>
            ) : (
              <Callout kind="bad" action={<button className="icon-btn" onClick={() => setVerify(null)} aria-label="Dismiss"><X size={15} /></button>}>
                <ShieldX size={14} style={{ verticalAlign: -2 }} /> Record #{verify.broken_at}: {verify.reason} {verify.checked.toLocaleString()} records before it are intact.
              </Callout>
            )}
          </div>
        )}

        <Card flush>
          <div style={{ padding: "16px 16px 0" }}>
            <div className="filters">
              <div className="input-group"><Search size={15} className="lead" /><input className="input" placeholder="Search actions, tools, files, models…" value={search} onChange={(e) => setSearch(e.target.value)} aria-label="Search the audit log" /></div>
              <Select value={category} onChange={setCategory} options={cats} label="Area" />
              <Select value={actor} onChange={setActor} options={ACTORS} label="Who" />
              <Select value={outcome} onChange={setOutcome} options={OUTCOMES} label="Result" />
              <Select value={range} onChange={setRange} options={RANGES} label="Time" />
              {filtered && <button className="btn subtle" onClick={clear}><X size={14} /> Clear</button>}
              <button className="icon-btn" onClick={load} aria-label="Refresh" title="Refresh">{loading ? <Spinner /> : <RefreshCw size={15} />}</button>
            </div>
            {session && (
              <div className="row" style={{ marginBottom: 10 }}>
                <span className="chip on">Session {session.slice(0, 8)} <X size={12} style={{ cursor: "pointer" }} onClick={() => setSession("")} /></span>
              </div>
            )}
          </div>
          {err && <div style={{ padding: 16 }}><Callout kind="bad">{err}</Callout></div>}
          {!data && !err && <div style={{ padding: 16 }}><Skeleton lines={8} /></div>}
          {data && data.items.length === 0 && (
            <Empty icon={ScrollText} title={filtered ? "Nothing matches these filters" : "No activity recorded yet"} action={filtered ? <button className="btn" onClick={clear}>Clear filters</button> : undefined}>
              {filtered ? "Try a wider time range or another search." : "Actions appear here as you use the app."}
            </Empty>
          )}
          {data && data.items.length > 0 && (
            <div style={{ overflowX: "auto", opacity: loading ? 0.6 : 1, transition: "opacity .15s" }}>
              <table className="audit-table">
                <thead><tr><th style={{ width: 150 }}>Time</th><th style={{ width: 80 }}>Who</th><th>Action</th><th>Subject</th><th style={{ width: 110 }}>Result</th></tr></thead>
                <tbody>
                  {data.items.map((r) => <Row key={r.id} r={r} open={open === r.id} onToggle={() => setOpen(open === r.id ? null : r.id)} onSession={setSession} openChat={openChat} />)}
                </tbody>
              </table>
            </div>
          )}
          {data && (
            <div style={{ padding: "0 16px 14px" }}>
              <Pagination page={page} pages={Math.max(1, Math.ceil(data.total / size))} total={data.total} pageSize={size} onPage={setPage} onPageSize={setSize} sizes={[25, 50, 100, 200]} />
            </div>
          )}
        </Card>
      </div>
    </div>
  );
}

function Row({ r, open, onToggle, onSession, openChat }: { r: AuditEntry; open: boolean; onToggle: () => void; onSession: (s: string) => void; openChat: (id: string) => void }) {
  const Who = r.actor === "ai" ? Bot : r.actor === "user" ? User : Wrench;
  const isChat = r.category === "chat" && r.session;
  return (
    <>
      <tr className={"audit-row" + (open ? " open" : "")} onClick={onToggle} aria-expanded={open}>
        <td className="small muted" style={{ whiteSpace: "nowrap", fontVariantNumeric: "tabular-nums" }}>{fmtTime(r.ts)}</td>
        <td><span className="row small" style={{ gap: 5 }}><Who size={13} /> {r.actor === "ai" ? "AI" : r.actor === "user" ? "You" : "App"}</span></td>
        <td><span className="mono small">{r.action}</span></td>
        <td className="small ellipsis" style={{ maxWidth: 380 }} title={r.subject}>{r.subject}</td>
        <td className="small"><span className={"dot " + tone(r.outcome)} />{r.outcome}</td>
      </tr>
      {open && (
        <tr>
          <td colSpan={5} style={{ background: "var(--surface-2)" }}>
            <div className="row wrap small" style={{ gap: 14, marginBottom: 8 }}>
              <span className="muted">Record #{r.id}</span>
              <span className="muted">{new Date(r.ts).toLocaleString()}</span>
              {r.session && <button className="btn sm subtle" onClick={() => onSession(r.session)}>Show this session only</button>}
              {isChat && <button className="btn sm subtle" onClick={() => openChat(r.session)}><MessageSquare size={13} /> Open chat</button>}
            </div>
            <pre className="audit-detail mono">{JSON.stringify(r.details, null, 2)}</pre>
          </td>
        </tr>
      )}
    </>
  );
}
