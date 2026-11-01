import { useEffect, useRef, useState } from "react";
import { Check, ChevronDown, Cloud, Cpu, Lock, Plus } from "lucide-react";
import type { Model } from "../api";
import { Caps } from "./common";

/** Model chooser that shows capabilities and where data goes before you pick. */
export default function ModelPicker({ models, value, onChange, onManage, up }: { models: Model[]; value: string | null; onChange: (id: string) => void; onManage: () => void; up?: boolean }) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const cur = models.find((m) => m.id === value);
  useEffect(() => {
    if (!open) return;
    const h = (e: MouseEvent) => ref.current && !ref.current.contains(e.target as Node) && setOpen(false);
    const k = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    document.addEventListener("mousedown", h);
    document.addEventListener("keydown", k);
    return () => {
      document.removeEventListener("mousedown", h);
      document.removeEventListener("keydown", k);
    };
  }, [open]);
  return (
    <div className="popover-wrap" ref={ref}>
      <button className="picker-btn" onClick={() => setOpen(!open)} aria-haspopup="listbox" aria-expanded={open}>
        {cur ? (cur.local ? <Lock size={14} className="icon" /> : <Cloud size={14} className="icon" />) : <Cpu size={14} className="icon" />}
        <span className="ellipsis">{cur ? cur.label : "Choose a model"}</span>
        <ChevronDown size={14} className="icon muted" />
      </button>
      {open && (
        <div className={"popover" + (up ? " up" : "")} role="listbox">
          {models.map((m) => (
            <button key={m.id} className={"menu-item " + (m.id === value ? "on" : "")} role="option" aria-selected={m.id === value} onClick={() => { onChange(m.id); setOpen(false); }}>
              <div className="grow" style={{ display: "grid", gap: 4 }}>
                <div className="row"><b className="ellipsis">{m.label}</b><span className="small muted ellipsis">{m.provider_name}</span></div>
                <Caps m={m} />
              </div>
              {m.id === value && <Check size={15} className="icon" />}
            </button>
          ))}
          {models.length > 0 && <div className="menu-sep" />}
          <button className="menu-item" onClick={() => { setOpen(false); onManage(); }}><Plus size={15} className="icon" /> Add or manage models</button>
        </div>
      )}
    </div>
  );
}
