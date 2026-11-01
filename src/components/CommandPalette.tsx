import { useEffect, useMemo, useRef, useState } from "react";
import { CornerDownLeft, Search, type LucideIcon } from "lucide-react";

export type Command = { id: string; label: string; group: string; icon: LucideIcon; hint?: string; keywords?: string; run: () => void };

/** Keyboard-first launcher (Ctrl+K): fuzzy search over chats, pages, settings and actions. */
export default function CommandPalette({ commands, onClose }: { commands: Command[]; onClose: () => void }) {
  const [q, setQ] = useState("");
  const [sel, setSel] = useState(0);
  const listRef = useRef<HTMLDivElement>(null);

  const results = useMemo(() => {
    const terms = q.toLowerCase().split(/\s+/).filter(Boolean);
    const scored = commands
      .map((c) => {
        const hay = `${c.label} ${c.group} ${c.keywords ?? ""}`.toLowerCase();
        if (!terms.every((t) => hay.includes(t))) return null;
        const score = terms.length === 0 ? 0 : c.label.toLowerCase().startsWith(terms[0]) ? 2 : 1;
        return { c, score };
      })
      .filter(Boolean) as { c: Command; score: number }[];
    scored.sort((a, b) => b.score - a.score);
    return scored.map((x) => x.c).slice(0, 60);
  }, [q, commands]);

  useEffect(() => setSel(0), [q]);
  useEffect(() => {
    listRef.current?.querySelector(".palette-item.on")?.scrollIntoView({ block: "nearest" });
  }, [sel]);

  const run = (c?: Command) => {
    if (!c) return;
    onClose();
    c.run();
  };

  let lastGroup = "";
  return (
    <div className="palette-bg" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="palette" role="dialog" aria-label="Command palette">
        <div className="palette-input">
          <Search size={17} className="muted" />
          <input
            autoFocus
            value={q}
            onChange={(e) => setQ(e.target.value)}
            placeholder="Type a command, page, setting or chat…"
            aria-label="Search commands"
            onKeyDown={(e) => {
              if (e.key === "ArrowDown") { e.preventDefault(); setSel((s) => Math.min(s + 1, results.length - 1)); }
              if (e.key === "ArrowUp") { e.preventDefault(); setSel((s) => Math.max(s - 1, 0)); }
              if (e.key === "Enter") run(results[sel]);
              if (e.key === "Escape") onClose();
            }}
          />
        </div>
        <div className="palette-list" ref={listRef} role="listbox">
          {results.length === 0 && <div className="empty" style={{ padding: 28 }}>No matches</div>}
          {results.map((c, i) => {
            const head = c.group !== lastGroup ? <div className="palette-group">{c.group}</div> : null;
            lastGroup = c.group;
            return (
              <div key={c.id}>
                {head}
                <button className={"palette-item " + (i === sel ? "on" : "")} role="option" aria-selected={i === sel} onMouseMove={() => setSel(i)} onClick={() => run(c)}>
                  <c.icon size={16} className="icon" />
                  <span className="ellipsis">{c.label}</span>
                  {c.hint && <span className="hint">{c.hint}</span>}
                </button>
              </div>
            );
          })}
        </div>
        <div className="palette-foot">
          <span><span className="kbd">↑↓</span> navigate</span>
          <span><CornerDownLeft size={11} /> run</span>
          <span><span className="kbd">Esc</span> close</span>
        </div>
      </div>
    </div>
  );
}
