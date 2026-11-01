import { useEffect, useMemo, useState } from "react";
import { Activity, ArrowDown, ArrowUp, BarChart3, Bot, Clock, Coins, Cpu, FileText, Gauge, MessageSquare, RefreshCw, Search, Wrench, Zap } from "lucide-react";
import { api, errText, money, type ModelStat, type Stats } from "../api";
import { Callout, Card, Empty, Seg, Skeleton, Spinner, timeAgo, useApp } from "../components/common";

const RANGES = [{ value: "7", label: "7 days" }, { value: "30", label: "30 days" }, { value: "90", label: "90 days" }, { value: "0", label: "All time" }];
const num = (n: number) => (n >= 1e6 ? `${(n / 1e6).toFixed(1)}M` : n >= 1e4 ? `${Math.round(n / 1e3)}K` : n >= 1e3 ? `${(n / 1e3).toFixed(1)}K` : String(Math.round(n)));
const secs = (ms: number) => (!ms ? "—" : ms < 1000 ? `${Math.round(ms)} ms` : `${(ms / 1000).toFixed(1)} s`);

type SortKey = "replies" | "output" | "avg_tps" | "avg_ttft_ms" | "cost" | "last";

/** Usage, speed and model statistics from your own replies. Nothing leaves this computer. */
export default function StatsView() {
  const { models, settings } = useApp();
  const [range, setRange] = useState("30");
  const [data, setData] = useState<Stats | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [sort, setSort] = useState<{ key: SortKey; desc: boolean }>({ key: "replies", desc: true });
  const [q, setQ] = useState("");

  const load = async () => {
    setLoading(true);
    try {
      setData(await api.stats(Number(range)));
      setErr(null);
    } catch (e) {
      setErr(errText(e));
    }
    setLoading(false);
  };
  useEffect(() => { load(); }, [range]);

  const days = useMemo(() => {
    if (!data) return [];
    // Fill gaps so quiet days show as empty bars.
    const n = range === "0" ? Math.min(180, Math.max(data.daily.length, 7)) : Number(range);
    const map = new Map(data.daily.map((d) => [d.day, d]));
    const out = [];
    const start = new Date();
    start.setHours(0, 0, 0, 0);
    for (let i = n - 1; i >= 0; i--) {
      const d = new Date(start.getTime() - i * 86_400_000);
      const key = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
      out.push({ day: key, date: d, ...(map.get(key) ?? { replies: 0, input: 0, output: 0, cost: 0 }) });
    }
    return out;
  }, [data, range]);

  const rows = useMemo(() => {
    const list = (data?.by_model ?? []).filter((m) => `${m.model} ${m.provider}`.toLowerCase().includes(q.toLowerCase()));
    return [...list].sort((a, b) => (sort.desc ? 1 : -1) * ((b[sort.key] as number) - (a[sort.key] as number)));
  }, [data, sort, q]);

  if (err) return <div className="page"><div className="page-inner wide"><Callout kind="bad">{err}</Callout></div></div>;
  const t = data?.totals;
  const maxTps = Math.max(1, ...(data?.by_model ?? []).map((m) => m.avg_tps));
  const tasksDone = data?.tasks.done ?? 0;
  const tasksAll = Object.values(data?.tasks ?? {}).reduce((a, b) => a + b, 0);
  const local = (id: string) => models.find((m) => m.id === id)?.local;
  const head = (key: SortKey, label: string) => (
    <th className="num sortable" aria-sort={sort.key === key ? (sort.desc ? "descending" : "ascending") : "none"}>
      <button onClick={() => setSort((s) => ({ key, desc: s.key === key ? !s.desc : true }))}>{label}{sort.key === key && (sort.desc ? <ArrowDown size={12} /> : <ArrowUp size={12} />)}</button>
    </th>
  );

  return (
    <div className="page">
      <div className="page-inner wide">
        <div className="page-head">
          <div className="grow">
            <h1>Stats</h1>
            <p>How you use your models: replies, tokens, speed and cost, measured on this computer.</p>
          </div>
          <Seg value={range} onChange={setRange} label="Time range" options={RANGES} />
          <button className="btn" onClick={load} disabled={loading} aria-label="Refresh">{loading ? <Spinner size={14} /> : <RefreshCw size={15} />}</button>
        </div>
        {!data ? <Card><Skeleton lines={6} /></Card> : (
          <div className="stack" style={{ gap: 16 }}>
            <div className="kpis">
              <Kpi icon={MessageSquare} label="Replies" value={num(t!.replies)} sub={`${num(data.messages)} messages · ${num(data.chats)} new chats`} />
              <Kpi icon={Activity} label="Tokens" value={num(t!.input + t!.output)} sub={`${num(t!.input)} in · ${num(t!.output)} out`} />
              <Kpi icon={Gauge} label="Average speed" value={t!.avg_tps ? `${t!.avg_tps.toFixed(1)}` : "—"} unit="tok/s" sub="while generating" />
              <Kpi icon={Clock} label="First token" value={secs(t!.avg_ttft_ms)} sub="average wait before the reply starts" />
              {settings.show_prices && <Kpi icon={Coins} label="Cost" value={money(t!.cost)} sub="cloud models; local is free" />}
              <Kpi icon={Bot} label="Agent tasks" value={String(tasksAll)} sub={tasksAll ? `${tasksDone} done · ${num(data.agent_steps)} steps` : "none in this range"} />
              <Kpi icon={FileText} label="Documents" value={String(data.documents)} sub="attached to chats" />
              <Kpi icon={Zap} label="Problems" value={String(t!.errors)} sub={`${t!.stopped} stopped by you`} tone={t!.errors ? "warn" : undefined} />
            </div>

            {t!.replies === 0 ? (
              <Card><Empty icon={BarChart3} title="No replies in this range">Chat with any model and its speed and usage show up here. Stats start from this version of the app.</Empty></Card>
            ) : (
              <>
                <div className="stats-grid">
                  <Card title="Tokens per day" icon={BarChart3} subtitle="Output (solid) and input (light)">
                    <Bars data={days.map((d) => ({ key: d.day, label: d.date.toLocaleDateString([], { month: "short", day: "numeric" }), a: d.output, b: d.input, tip: `${d.date.toLocaleDateString([], { weekday: "short", month: "short", day: "numeric" })}: ${d.replies} replies · ${d.output.toLocaleString()} out · ${d.input.toLocaleString()} in${d.cost ? ` · ${money(d.cost)}` : ""}` }))} />
                  </Card>
                  <Card title="When you chat" icon={Clock} subtitle="Replies by hour of day">
                    <Bars data={data.hours.map((n, h) => ({ key: String(h), label: h % 6 === 0 ? `${String(h).padStart(2, "0")}:00` : "", a: n, b: 0, tip: `${String(h).padStart(2, "0")}:00–${String(h).padStart(2, "0")}:59: ${n} replies` }))} every={1} />
                  </Card>
                </div>

                <Card title="Models" icon={Cpu} flush actions={
                  <div className="search-box" style={{ flex: "0 1 220px" }}><Search size={15} /><input className="input" placeholder="Filter models" value={q} onChange={(e) => setQ(e.target.value)} aria-label="Filter models" /></div>
                }>
                  <table className="list stats-table">
                    <thead>
                      <tr>
                        <th>Model</th>
                        {head("replies", "Replies")}
                        {head("output", "Tokens out")}
                        {head("avg_tps", "Speed")}
                        {head("avg_ttft_ms", "First token")}
                        {settings.show_prices && head("cost", "Cost")}
                        {head("last", "Last used")}
                      </tr>
                    </thead>
                    <tbody>
                      {rows.map((m: ModelStat) => (
                        <tr key={m.model_id}>
                          <td>
                            <b>{m.model}</b>
                            <div className="small muted">{m.provider}{local(m.model_id) === false ? " · cloud" : local(m.model_id) ? " · local" : ""}{m.errors ? ` · ${m.errors} failed` : ""}</div>
                          </td>
                          <td className="num">{m.replies.toLocaleString()}</td>
                          <td className="num">{num(m.output)}<div className="small muted">{num(m.input)} in</div></td>
                          <td className="num">
                            <div className="speed-cell"><span className="speed-bar"><i style={{ width: `${(m.avg_tps / maxTps) * 100}%` }} /></span><b>{m.avg_tps ? m.avg_tps.toFixed(1) : "—"}</b></div>
                            <div className="small muted">peak {m.max_tps.toFixed(1)} tok/s</div>
                          </td>
                          <td className="num">{secs(m.avg_ttft_ms)}</td>
                          {settings.show_prices && <td className="num">{m.cost ? money(m.cost) : <span className="muted">Free</span>}</td>}
                          <td className="num small muted">{timeAgo(m.last)}</td>
                        </tr>
                      ))}
                      {rows.length === 0 && <tr><td colSpan={7} className="muted" style={{ textAlign: "center", padding: 24 }}>No models match.</td></tr>}
                    </tbody>
                  </table>
                </Card>

                {data.tools.length > 0 && (
                  <Card title="Tools and sub-agents" icon={Wrench} subtitle="How often the assistant used each tool in chats">
                    <div className="tool-bars">
                      {data.tools.map((x) => (
                        <div key={x.name} className="tool-bar" title={`${x.calls} calls${x.failed ? `, ${x.failed} failed or declined` : ""}`}>
                          <span className="mono ellipsis">{x.name}</span>
                          <span className="speed-bar"><i style={{ width: `${(x.calls / data.tools[0].calls) * 100}%` }} /></span>
                          <span className="num">{x.calls}{x.failed ? <span className="muted"> · {x.failed} failed</span> : null}</span>
                        </div>
                      ))}
                    </div>
                  </Card>
                )}
              </>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

function Kpi({ icon: Icon, label, value, unit, sub, tone }: { icon: typeof Gauge; label: string; value: string; unit?: string; sub: string; tone?: "warn" }) {
  return (
    <div className={"kpi" + (tone ? " " + tone : "")}>
      <div className="kpi-label"><Icon size={14} /> {label}</div>
      <div className="kpi-value">{value}{unit && <small> {unit}</small>}</div>
      <div className="kpi-sub">{sub}</div>
    </div>
  );
}

/** Small bar chart: `a` solid, `b` stacked light on top. Labels shown sparsely. */
function Bars({ data, every }: { data: { key: string; label: string; a: number; b: number; tip: string }[]; every?: number }) {
  const max = Math.max(1, ...data.map((d) => d.a + d.b));
  const step = every ?? Math.ceil(data.length / 8);
  return (
    <div className="bars" role="img" aria-label="Bar chart">
      <div className="bars-plot">
        {data.map((d) => (
          <div key={d.key} className="cbar" title={d.tip}>
            <i className="b" style={{ height: `${(d.b / max) * 100}%` }} />
            <i className="a" style={{ height: `${(d.a / max) * 100}%` }} />
          </div>
        ))}
      </div>
      <div className="bars-axis">
        {data.map((d, i) => <span key={d.key}>{every ? d.label : i % step === 0 && data.length - i > step / 2 ? d.label : ""}</span>)}
      </div>
      <div className="bars-max small muted">{max.toLocaleString()}</div>
    </div>
  );
}
