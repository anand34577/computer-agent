import { Children, createContext, isValidElement, useContext, useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactElement, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { AlertCircle, AudioLines, Brain, Check, CheckCircle2, ChevronDown, ChevronLeft, ChevronRight, Cloud, Database, Eye, HardDrive, Info, Loader2, Minus, Plus, Search, TriangleAlert, Wrench, X, type LucideIcon } from "lucide-react";
import type { Model, Settings } from "../api";

/* ---------- App context ---------- */

export type Toast = { id: number; text: string; kind?: "ok" | "bad" | "info" };

export type AppCtx = {
  settings: Settings;
  setSettings: (s: Settings) => Promise<boolean>;
  models: Model[];
  refreshModels: () => Promise<void>;
  toast: (text: string, kind?: Toast["kind"]) => void;
};

export const Ctx = createContext<AppCtx>(null as unknown as AppCtx);
export const useApp = () => useContext(Ctx);

/* ---------- Form controls ---------- */

export function Field({ label, hint, error, required, children, htmlFor }: { label: ReactNode; hint?: ReactNode; error?: string; required?: boolean; children: ReactNode; htmlFor?: string }) {
  return (
    <div className="field">
      <label className="field-label" htmlFor={htmlFor}>
        {label}
        {required && <span className="req" aria-hidden>*</span>}
      </label>
      {children}
      {error ? (
        <div className="field-error" role="alert">
          <AlertCircle size={13} className="icon" /> {error}
        </div>
      ) : hint ? (
        <div className="field-hint">{hint}</div>
      ) : null}
    </div>
  );
}

let uid = 0;
export const useId = (p = "f") => useState(() => `${p}${++uid}`)[0];

export function TextField({ label, hint, error, required, value, onChange, onBlur, placeholder, type = "text", autoFocus, disabled, mono }: {
  label: ReactNode; hint?: ReactNode; error?: string; required?: boolean; value: string; onChange: (v: string) => void; onBlur?: () => void;
  placeholder?: string; type?: string; autoFocus?: boolean; disabled?: boolean; mono?: boolean;
}) {
  const id = useId();
  return (
    <Field label={label} hint={hint} error={error} required={required} htmlFor={id}>
      <input id={id} className={"input" + (error ? " invalid" : "") + (mono ? " mono" : "")} type={type} value={value} placeholder={placeholder} autoFocus={autoFocus} disabled={disabled}
        aria-invalid={!!error} onChange={(e) => onChange(e.target.value)} onBlur={onBlur} autoComplete="off" spellCheck={false} />
    </Field>
  );
}

/** Number input that keeps the raw text while typing and reports a validation message. */
export function NumberField({ label, hint, value, onCommit, min, max, step = 1, suffix, validate }: {
  label: ReactNode; hint?: ReactNode; value: number; onCommit: (v: number) => void; min: number; max: number; step?: number; suffix?: string;
  validate?: (v: number) => string | undefined;
}) {
  const id = useId();
  const [raw, setRaw] = useState(String(value));
  useEffect(() => setRaw(String(value)), [value]);
  const n = raw.trim() === "" ? NaN : Number(raw);
  const err = !Number.isFinite(n) ? "Enter a number" : n < min || n > max ? `Must be between ${min} and ${max}` : step >= 1 && !Number.isInteger(n) ? "Use a whole number" : validate?.(n);
  return (
    <Field label={label} hint={hint ?? (suffix ? `${min}–${max} ${suffix}` : undefined)} error={err} htmlFor={id}>
      <Stepper id={id} raw={raw} setRaw={setRaw} invalid={!!err} min={min} max={max} step={step}
        commit={(v) => { if (v !== value) onCommit(v); }} canCommit={(v) => Number.isFinite(v) && v >= min && v <= max && !(step >= 1 && !Number.isInteger(v)) && !validate?.(v)} />
    </Field>
  );
}

function Stepper({ id, raw, setRaw, invalid, min, max, step, commit, canCommit }: {
  id: string; raw: string; setRaw: (v: string) => void; invalid: boolean; min: number; max: number; step: number; commit: (v: number) => void; canCommit: (v: number) => boolean;
}) {
  const nudge = (dir: 1 | -1) => {
    const cur = Number(raw);
    const base = Number.isFinite(cur) ? cur : min;
    const next = Math.min(max, Math.max(min, Math.round((base + dir * step) * 1e6) / 1e6));
    setRaw(String(next));
    if (canCommit(next)) commit(next);
  };
  return (
    <div className={"stepper" + (invalid ? " invalid" : "")}>
      <button type="button" className="step-btn" aria-label="Decrease" tabIndex={-1} onClick={() => nudge(-1)} disabled={Number(raw) <= min}><Minus size={14} /></button>
      <input id={id} className="input" type="text" inputMode="decimal" value={raw} aria-invalid={invalid} role="spinbutton" aria-valuemin={min} aria-valuemax={max} aria-valuenow={Number(raw) || 0}
        onChange={(e) => setRaw(e.target.value.replace(/[^0-9.-]/g, ""))}
        onBlur={() => { const v = Number(raw); if (raw.trim() !== "" && canCommit(v)) commit(v); }}
        onKeyDown={(e) => {
          if (e.key === "Enter") (e.target as HTMLInputElement).blur();
          if (e.key === "ArrowUp") { e.preventDefault(); nudge(1); }
          if (e.key === "ArrowDown") { e.preventDefault(); nudge(-1); }
        }} />
      <button type="button" className="step-btn" aria-label="Increase" tabIndex={-1} onClick={() => nudge(1)} disabled={Number(raw) >= max}><Plus size={14} /></button>
    </div>
  );
}

/* ---------- Custom select (no native dropdowns) ---------- */

export type Opt = { value: string; label: ReactNode; text?: string; hint?: ReactNode; icon?: LucideIcon; disabled?: boolean };

function textOf(n: ReactNode): string {
  if (n == null || typeof n === "boolean") return "";
  if (typeof n === "string" || typeof n === "number") return String(n);
  if (Array.isArray(n)) return n.map(textOf).join("");
  if (isValidElement(n)) return textOf((n.props as { children?: ReactNode }).children);
  return "";
}

/** Reads <option> children so existing markup keeps working with the custom list. */
function optionsFrom(children: ReactNode): Opt[] {
  const out: Opt[] = [];
  Children.toArray(children).forEach((c) => {
    if (!isValidElement(c)) return;
    const el = c as ReactElement<{ value?: string; children?: ReactNode; disabled?: boolean }>;
    if (el.type === "option") out.push({ value: String(el.props.value ?? textOf(el.props.children)), label: textOf(el.props.children), disabled: el.props.disabled });
    else if (el.props.children) out.push(...optionsFrom(el.props.children));
  });
  return out;
}

type Pos = { left: number; top: number; width: number; up: boolean; maxH: number };

/** Positions a floating panel under (or above) an anchor and keeps it inside the window. */
function usePopover(open: boolean, anchor: React.RefObject<HTMLElement | null>, width?: number, alignRight?: boolean): Pos {
  const [pos, setPos] = useState<Pos>({ left: 0, top: 0, width: 0, up: false, maxH: 320 });
  useLayoutEffect(() => {
    if (!open) return;
    const place = () => {
      const r = anchor.current?.getBoundingClientRect();
      if (!r) return;
      const w = Math.max(width ?? 0, alignRight ? 0 : r.width);
      const below = window.innerHeight - r.bottom - 12;
      const up = below < 220 && r.top > below;
      const want = alignRight ? r.right - w : r.left;
      const left = Math.min(Math.max(8, want), window.innerWidth - w - 8);
      setPos({ left, top: up ? r.top - 6 : r.bottom + 6, width: w, up, maxH: Math.max(160, Math.min(360, up ? r.top - 16 : below)) });
    };
    place();
    window.addEventListener("resize", place);
    window.addEventListener("scroll", place, true);
    return () => { window.removeEventListener("resize", place); window.removeEventListener("scroll", place, true); };
  }, [open, anchor, width, alignRight]);
  return pos;
}

const popStyle = (p: Pos, maxH = true): React.CSSProperties => ({
  left: p.left, width: p.width, top: p.up ? undefined : p.top, bottom: p.up ? window.innerHeight - p.top : undefined, maxHeight: maxH ? p.maxH : undefined,
});

/** Closes a popover on an outside click. */
function useOutside(open: boolean, refs: React.RefObject<HTMLElement | null>[], close: () => void) {
  useEffect(() => {
    if (!open) return;
    const h = (e: MouseEvent) => { if (!refs.some((r) => r.current?.contains(e.target as Node))) close(); };
    window.addEventListener("mousedown", h);
    return () => window.removeEventListener("mousedown", h);
  }, [open]);
}

export function Select({ value, onChange, options, children, placeholder = "Choose…", disabled, invalid, id, label, width, searchable }: {
  value: string; onChange: (v: string) => void; options?: Opt[]; children?: ReactNode; placeholder?: string; disabled?: boolean; invalid?: boolean; id?: string; label?: string; width?: number | string; searchable?: boolean;
}) {
  const opts = useMemo(() => options ?? optionsFrom(children), [options, children]);
  const [open, setOpen] = useState(false);
  const [q, setQ] = useState("");
  const [hi, setHi] = useState(0);
  const btn = useRef<HTMLButtonElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const pos = usePopover(open, btn);
  const showSearch = searchable ?? opts.length > 8;
  const shown = q ? opts.filter((o) => (o.text ?? textOf(o.label)).toLowerCase().includes(q.toLowerCase())) : opts;
  const cur = opts.find((o) => o.value === value);
  useOutside(open, [btn, listRef], () => setOpen(false));
  useEffect(() => {
    if (!open) return;
    setQ("");
    setHi(Math.max(0, opts.findIndex((o) => o.value === value)));
  }, [open]);
  useEffect(() => { listRef.current?.querySelector<HTMLElement>(`[data-i="${hi}"]`)?.scrollIntoView({ block: "nearest" }); }, [hi, open]);
  const pick = (o?: Opt) => { if (o && !o.disabled) { onChange(o.value); setOpen(false); btn.current?.focus(); } };
  const onKey = (e: React.KeyboardEvent) => {
    if (!open && ["ArrowDown", "ArrowUp", "Enter", " "].includes(e.key)) { e.preventDefault(); setOpen(true); return; }
    if (!open) return;
    if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); setOpen(false); btn.current?.focus(); }
    else if (e.key === "ArrowDown") { e.preventDefault(); setHi((h) => Math.min(shown.length - 1, h + 1)); }
    else if (e.key === "ArrowUp") { e.preventDefault(); setHi((h) => Math.max(0, h - 1)); }
    else if (e.key === "Enter") { e.preventDefault(); pick(shown[hi]); }
    else if (e.key === "Tab") setOpen(false);
  };
  const CurIcon = cur?.icon;
  return (
    <>
      <button ref={btn} id={id} type="button" className={"input select" + (invalid ? " invalid" : "") + (open ? " open" : "")} style={{ width }} disabled={disabled}
        aria-haspopup="listbox" aria-expanded={open} aria-label={label} onClick={() => setOpen((o) => !o)} onKeyDown={onKey}>
        {CurIcon && <CurIcon size={15} className="icon" />}
        <span className={"select-val" + (cur ? "" : " placeholder")}>{cur ? cur.label : placeholder}</span>
        <ChevronDown size={15} className="icon chev" />
      </button>
      {open && createPortal(
        <div ref={listRef} className={"float listbox" + (pos.up ? " up" : "")} role="listbox" style={popStyle(pos)} onKeyDown={onKey}>
          {showSearch && (
            <div className="listbox-search"><Search size={14} className="icon" /><input autoFocus value={q} placeholder="Search" onChange={(e) => { setQ(e.target.value); setHi(0); }} aria-label="Search options" /></div>
          )}
          <div className="listbox-items">
            {shown.map((o, i) => {
              const I = o.icon;
              return (
                <div key={o.value} data-i={i} role="option" aria-selected={o.value === value} aria-disabled={o.disabled}
                  className={"opt" + (i === hi ? " hi" : "") + (o.value === value ? " sel" : "") + (o.disabled ? " dis" : "")}
                  onMouseEnter={() => setHi(i)} onMouseDown={(e) => e.preventDefault()} onClick={() => pick(o)}>
                  {I && <I size={15} className="icon" />}
                  <div className="grow"><div>{o.label}</div>{o.hint && <small>{o.hint}</small>}</div>
                  {o.value === value && <Check size={14} className="icon" />}
                </div>
              );
            })}
            {shown.length === 0 && <div className="opt dis">No matches</div>}
          </div>
        </div>,
        document.body,
      )}
    </>
  );
}

export function SelectField({ label, hint, error, value, onChange, children, options, placeholder, disabled }: { label: ReactNode; hint?: ReactNode; error?: string; value: string; onChange: (v: string) => void; children?: ReactNode; options?: Opt[]; placeholder?: string; disabled?: boolean }) {
  const id = useId();
  return (
    <Field label={label} hint={hint} error={error} htmlFor={id}>
      <Select id={id} value={value} onChange={onChange} options={options} invalid={!!error} placeholder={placeholder} disabled={disabled}>{children}</Select>
    </Field>
  );
}

/* ---------- Menus ---------- */

export type MenuItem = { label: string; icon?: LucideIcon; onClick: () => void; danger?: boolean; disabled?: boolean; hint?: string } | "sep";

export function Menu({ trigger, items, label }: { trigger: (open: boolean) => ReactNode; items: MenuItem[]; label: string }) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLSpanElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const pos = usePopover(open, ref, 224, true);
  const real = items.filter((i): i is Exclude<MenuItem, "sep"> => i !== "sep");
  const [hi, setHi] = useState(-1);
  useOutside(open, [ref, panel], () => setOpen(false));
  useEffect(() => {
    if (!open) return;
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") { e.stopPropagation(); setOpen(false); }
      if (e.key === "ArrowDown") { e.preventDefault(); setHi((h) => (h + 1) % real.length); }
      if (e.key === "ArrowUp") { e.preventDefault(); setHi((h) => (h <= 0 ? real.length - 1 : h - 1)); }
      if (e.key === "Enter" && hi >= 0) { e.preventDefault(); const it = real[hi]; if (!it.disabled) { setOpen(false); it.onClick(); } }
    };
    window.addEventListener("keydown", key, true);
    return () => window.removeEventListener("keydown", key, true);
  }, [open, hi]);
  useEffect(() => { if (open) setHi(-1); }, [open]);
  let idx = -1;
  return (
    <span ref={ref} className="menu-anchor" onClick={(e) => { e.stopPropagation(); setOpen((o) => !o); }} aria-haspopup="menu" aria-expanded={open} aria-label={label}>
      {trigger(open)}
      {open && createPortal(
        <div ref={panel} className={"float menu" + (pos.up ? " up" : "")} role="menu" onClick={(e) => e.stopPropagation()} style={popStyle(pos, false)}>
          {items.map((it, i) => {
            if (it === "sep") return <div key={i} className="menu-sep" />;
            idx++;
            const I = it.icon;
            const me = idx;
            return (
              <button key={i} role="menuitem" className={"menu-item" + (it.danger ? " danger" : "") + (me === hi ? " hi" : "")} disabled={it.disabled} onMouseEnter={() => setHi(me)}
                onClick={() => { setOpen(false); it.onClick(); }}>
                {I && <I size={15} className="icon" />}<span className="grow">{it.label}</span>{it.hint && <kbd>{it.hint}</kbd>}
              </button>
            );
          })}
        </div>,
        document.body,
      )}
    </span>
  );
}

/* ---------- Small controls ---------- */

export function Checkbox({ checked, onChange, children, disabled }: { checked: boolean; onChange: (v: boolean) => void; children?: ReactNode; disabled?: boolean }) {
  return (
    <label className={"checkbox" + (disabled ? " dis" : "")} onClick={(e) => { e.preventDefault(); if (!disabled) onChange(!checked); }}>
      <button type="button" role="checkbox" aria-checked={checked} disabled={disabled} className={"box" + (checked ? " on" : "")}
        onKeyDown={(e) => { if (e.key === " ") { e.preventDefault(); onChange(!checked); } }}>
        {checked && <Check size={12} strokeWidth={3} />}
      </button>
      {children && <span>{children}</span>}
    </label>
  );
}

export function Slider({ value, min, max, step = 1, onChange, label, format }: { value: number; min: number; max: number; step?: number; onChange: (v: number) => void; label: string; format?: (v: number) => string }) {
  const track = useRef<HTMLDivElement>(null);
  const pct = ((value - min) / (max - min)) * 100;
  const fromX = (x: number) => {
    const r = track.current!.getBoundingClientRect();
    const raw = min + ((x - r.left) / r.width) * (max - min);
    return Math.min(max, Math.max(min, Math.round(raw / step) * step));
  };
  return (
    <div className="slider">
      <div ref={track} className="slider-track"
        onPointerDown={(e) => { (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId); onChange(fromX(e.clientX)); }}
        onPointerMove={(e) => { if (e.buttons === 1) onChange(fromX(e.clientX)); }}>
        <div className="slider-fill" style={{ width: `${pct}%` }} />
        <div className="slider-thumb" style={{ left: `${pct}%` }} role="slider" tabIndex={0} aria-label={label} aria-valuemin={min} aria-valuemax={max} aria-valuenow={value}
          onKeyDown={(e) => {
            if (e.key === "ArrowRight" || e.key === "ArrowUp") { e.preventDefault(); onChange(Math.min(max, value + step)); }
            if (e.key === "ArrowLeft" || e.key === "ArrowDown") { e.preventDefault(); onChange(Math.max(min, value - step)); }
          }} />
      </div>
      <span className="slider-val">{format ? format(value) : value}</span>
    </div>
  );
}

const SWATCHES = ["#4f6bed", "#2563eb", "#0891b2", "#0d9488", "#16a34a", "#65a30d", "#ca8a04", "#ea580c", "#dc2626", "#db2777", "#c026d3", "#7c3aed", "#475569", "#1f2937"];

/** Colour picker: swatches plus a validated hex field, instead of the native colour dialog. */
export function ColorField({ value, onChange, label }: { value: string; onChange: (v: string) => void; label: string }) {
  const [open, setOpen] = useState(false);
  const [hex, setHex] = useState(value);
  const ref = useRef<HTMLButtonElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const pos = usePopover(open, ref, 232);
  useEffect(() => setHex(value), [value]);
  useOutside(open, [ref, panel], () => setOpen(false));
  const valid = /^#[0-9a-fA-F]{6}$/.test(hex);
  return (
    <>
      <button ref={ref} type="button" className="color-btn" aria-label={label} aria-expanded={open} onClick={() => setOpen((o) => !o)}><span style={{ background: value }} /></button>
      {open && createPortal(
        <div ref={panel} className="float color-pop" style={popStyle(pos, false)}>
          <div className="swatches">
            {SWATCHES.map((c) => (
              <button key={c} type="button" className={"swatch" + (c.toLowerCase() === value.toLowerCase() ? " on" : "")} style={{ background: c }} aria-label={c} onClick={() => { onChange(c); setOpen(false); }}>
                {c.toLowerCase() === value.toLowerCase() && <Check size={13} color="#fff" />}
              </button>
            ))}
          </div>
          <form className="row" style={{ gap: 6, marginTop: 10 }} onSubmit={(e) => { e.preventDefault(); if (valid) { onChange(hex.toLowerCase()); setOpen(false); } }}>
            <span className="color-preview" style={{ background: valid ? hex : "transparent" }} />
            <input className={"input mono" + (valid ? "" : " invalid")} style={{ height: 30 }} value={hex} maxLength={7} onChange={(e) => setHex(e.target.value.startsWith("#") ? e.target.value : "#" + e.target.value)} aria-label="Hex colour" />
            <button className="btn sm primary" disabled={!valid}>Use</button>
          </form>
        </div>,
        document.body,
      )}
    </>
  );
}

export function Switch({ checked, onChange, label, disabled }: { checked: boolean; onChange: (v: boolean) => void; label: string; disabled?: boolean }) {
  return (
    <button type="button" role="switch" aria-checked={checked} aria-label={label} disabled={disabled} className={"switch" + (checked ? " on" : "")} onClick={() => onChange(!checked)}>
      <span className="knob" />
    </button>
  );
}

/* ---------- Progress and loading ---------- */

export function Spinner({ size = 16, label }: { size?: number; label?: string }) {
  return <Loader2 size={size} className="spin icon" aria-label={label ?? "Loading"} role="status" />;
}

/** Determinate when `value` is given (0–100), otherwise an indeterminate bar. */
export function Progress({ value, label, small }: { value?: number; label?: ReactNode; small?: boolean }) {
  return (
    <div className={"progress" + (small ? " small" : "")}>
      {label && <div className="progress-label"><span>{label}</span>{value !== undefined && <span>{Math.round(value)}%</span>}</div>}
      <div className="progress-track" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={value}>
        <div className={"progress-bar" + (value === undefined ? " indeterminate" : "")} style={value !== undefined ? { width: `${Math.max(2, Math.min(100, value))}%` } : undefined} />
      </div>
    </div>
  );
}

export function Skeleton({ lines = 3 }: { lines?: number }) {
  return (
    <div className="skeleton" aria-busy="true" aria-label="Loading">
      {Array.from({ length: lines }, (_, i) => <div key={i} className="sk-line" style={{ width: `${90 - ((i * 17) % 40)}%` }} />)}
    </div>
  );
}

/* ---------- Pagination ---------- */

export function usePaged<T>(items: T[], initialSize = 25, resetKey?: unknown) {
  const [page, setPage] = useState(0);
  const [pageSize, setPageSize] = useState(initialSize);
  const pages = Math.max(1, Math.ceil(items.length / pageSize));
  useEffect(() => setPage(0), [resetKey, pageSize]);
  useEffect(() => { if (page > pages - 1) setPage(pages - 1); }, [pages]);
  return { page, setPage, pages, slice: items.slice(page * pageSize, page * pageSize + pageSize), total: items.length, pageSize, setPageSize };
}

export function Pagination({ page, pages, total, pageSize, onPage, onPageSize, sizes = [10, 25, 50, 100] }: {
  page: number; pages: number; total: number; pageSize: number; onPage: (p: number) => void; onPageSize?: (n: number) => void; sizes?: number[];
}) {
  if (total === 0) return null;
  const from = page * pageSize + 1;
  const to = Math.min(total, from + pageSize - 1);
  const nums: (number | "…")[] = [];
  for (let i = 0; i < pages; i++) {
    if (i === 0 || i === pages - 1 || Math.abs(i - page) <= 1) nums.push(i);
    else if (nums[nums.length - 1] !== "…") nums.push("…");
  }
  return (
    <nav className="pager" aria-label="Pages">
      <span className="muted">{from}–{to} of {total.toLocaleString()}</span>
      <div className="grow" />
      {onPageSize && (
        <Select value={String(pageSize)} onChange={(v) => onPageSize(Number(v))} width={116} label="Rows per page" options={sizes.map((n) => ({ value: String(n), label: `${n} / page` }))} />
      )}
      <button className="icon-btn" disabled={page === 0} onClick={() => onPage(page - 1)} aria-label="Previous page"><ChevronLeft size={16} /></button>
      {nums.map((n, i) => n === "…" ? <span key={i} className="muted pager-gap">…</span> : (
        <button key={i} className={"pager-num" + (n === page ? " on" : "")} aria-current={n === page ? "page" : undefined} onClick={() => onPage(n)}>{n + 1}</button>
      ))}
      <button className="icon-btn" disabled={page >= pages - 1} onClick={() => onPage(page + 1)} aria-label="Next page"><ChevronRight size={16} /></button>
    </nav>
  );
}


export function SettingRow({ label, hint, children }: { label: ReactNode; hint?: ReactNode; children: ReactNode }) {
  return (
    <div className="setting">
      <div className="lbl">
        <span>{label}</span>
        {hint && <small>{hint}</small>}
      </div>
      {children}
    </div>
  );
}

export function ToggleRow({ label, hint, checked, onChange }: { label: string; hint?: ReactNode; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <SettingRow label={label} hint={hint}>
      <Switch checked={checked} onChange={onChange} label={label} />
    </SettingRow>
  );
}

export function Seg<T extends string>({ value, options, onChange, label }: { value: T; options: { value: T; label: string; icon?: LucideIcon }[]; onChange: (v: T) => void; label?: string }) {
  return (
    <div className="seg" role="radiogroup" aria-label={label}>
      {options.map((o) => (
        <button key={o.value} type="button" className={o.value === value ? "on" : ""} role="radio" aria-checked={o.value === value} onClick={() => onChange(o.value)}>
          {o.icon && <o.icon size={14} className="icon" />}
          {o.label}
        </button>
      ))}
    </div>
  );
}

/* ---------- Surfaces ---------- */

export function Modal({ title, subtitle, icon: Icon, children, onClose, footer, wide }: { title: ReactNode; subtitle?: ReactNode; icon?: LucideIcon; children: ReactNode; onClose: () => void; footer?: ReactNode; wide?: boolean }) {
  useEffect(() => {
    const h = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  }, [onClose]);
  // Rendered at the top level so no scrolling or animated container can clip it.
  return createPortal(
    <div className="modal-bg" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className={"modal" + (wide ? " wide" : "")} role="dialog" aria-modal="true">
        <div className="modal-head">
          {Icon && <div className="card-icon"><Icon size={17} /></div>}
          <div className="grow">
            <h2>{title}</h2>
            {subtitle && <p>{subtitle}</p>}
          </div>
          <button className="icon-btn" onClick={onClose} aria-label="Close"><X size={16} /></button>
        </div>
        <div className="modal-body">{children}</div>
        {footer && <div className="modal-foot">{footer}</div>}
      </div>
    </div>,
    document.body,
  );
}

export function Card({ title, subtitle, icon: Icon, actions, children, flush }: { title?: ReactNode; subtitle?: ReactNode; icon?: LucideIcon; actions?: ReactNode; children: ReactNode; flush?: boolean }) {
  return (
    <section className={"card" + (flush ? " flush" : "")}>
      {title && (
        <div className="card-head" style={flush ? { padding: "18px 20px 0" } : undefined}>
          {Icon && <div className="card-icon"><Icon size={17} /></div>}
          <div className="grow">
            <h2>{title}</h2>
            {subtitle && <p>{subtitle}</p>}
          </div>
          {actions}
        </div>
      )}
      {children}
    </section>
  );
}

const CALLOUT_ICON = { info: Info, warn: TriangleAlert, bad: AlertCircle, ok: CheckCircle2 };
export function Callout({ kind = "info", children, action }: { kind?: "info" | "warn" | "bad" | "ok"; children: ReactNode; action?: ReactNode }) {
  const I = CALLOUT_ICON[kind];
  return (
    <div className={"callout " + kind} role={kind === "bad" ? "alert" : undefined}>
      <I size={16} className="icon" />
      <div className="grow">{children}</div>
      {action}
    </div>
  );
}

export function Empty({ icon: Icon, title, children, action }: { icon: LucideIcon; title: ReactNode; children?: ReactNode; action?: ReactNode }) {
  return (
    <div className="empty">
      <div className="empty-icon"><Icon size={20} /></div>
      <b>{title}</b>
      {children && <div style={{ maxWidth: 420 }}>{children}</div>}
      {action}
    </div>
  );
}

export const CAPS: { key: "vision" | "tools" | "reasoning" | "audio" | "embedding"; label: string; icon: LucideIcon; on: string; off: string }[] = [
  { key: "vision", label: "Vision", icon: Eye, on: "Reads images and screenshots", off: "Cannot read images" },
  { key: "tools", label: "Tools", icon: Wrench, on: "Native tool calling", off: "No native tool calling (uses JSON action mode)" },
  { key: "reasoning", label: "Reasoning", icon: Brain, on: "Thinks before answering", off: "No reasoning mode" },
  { key: "audio", label: "Audio", icon: AudioLines, on: "Understands audio", off: "No audio" },
  { key: "embedding", label: "Embedding", icon: Database, on: "Embedding model for document search", off: "Not an embedding model" },
];

/** Capabilities as a row of icons: lit when the model has it, faint when not. */
export function Caps({ m, compact }: { m: Model; compact?: boolean }) {
  const e = m.effective;
  const list = e.embedding ? CAPS.filter((c) => c.key === "embedding") : CAPS.filter((c) => c.key !== "embedding" && (!compact || e[c.key]));
  return (
    <span className="caps">
      {list.map((c) => {
        const has = !!e[c.key];
        const forced = m.overrides[c.key] != null;
        return (
          <span key={c.key} className={"cap " + c.key + (has ? " on" : "")} title={`${c.label}: ${has ? c.on : c.off}${forced ? " (set by you)" : m.profile.tested ? " (tested)" : " (guessed from the name)"}`} aria-label={`${c.label}: ${has ? "yes" : "no"}`}>
            <c.icon size={13} />
          </span>
        );
      })}
      {!compact && (
        <span className={"cap where " + (m.local ? "local" : "cloud")} title={m.local ? "Runs on this computer or your network" : "Cloud service"} aria-label={m.local ? "Local" : "Cloud"}>
          {m.local ? <HardDrive size={13} /> : <Cloud size={13} />}
        </span>
      )}
      {!compact && !e.tested && !e.embedding && <span className="cap untested" title="Not tested yet: capabilities are guessed from the name. Run the test.">?</span>}
    </span>
  );
}

export function Toasts({ toasts }: { toasts: Toast[] }) {
  return (
    <div className="toast-wrap" role="status" aria-live="polite">
      {toasts.map((t) => {
        const I = t.kind === "bad" ? AlertCircle : t.kind === "ok" ? CheckCircle2 : Info;
        return (
          <div key={t.id} className={"toast " + (t.kind ?? "")}>
            <I size={16} className="icon" />
            <div>{t.text}</div>
          </div>
        );
      })}
    </div>
  );
}

/* ---------- Helpers ---------- */

export function timeAgo(ms: number) {
  const s = (Date.now() - ms) / 1000;
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)}m ago`;
  if (s < 86400) return `${Math.floor(s / 3600)}h ago`;
  return new Date(ms).toLocaleDateString();
}

/** Asks for a short text value with validation, replacing window.prompt. */
export function PromptDialog({ title, label, initial = "", validate, confirmLabel = "Save", onSubmit, onClose, hint }: {
  title: string; label: string; initial?: string; validate: (v: string) => string | undefined; confirmLabel?: string; hint?: string;
  onSubmit: (v: string) => Promise<void> | void; onClose: () => void;
}) {
  const [v, setV] = useState(initial);
  const [touched, setTouched] = useState(false);
  const [serverErr, setServerErr] = useState<string>();
  const err = serverErr ?? (touched ? validate(v) : undefined);
  const submit = async () => {
    setTouched(true);
    if (validate(v)) return;
    try {
      await onSubmit(v.trim());
      onClose();
    } catch (e) {
      setServerErr(String(e).replace(/^[A-Za-z ]+: /, ""));
    }
  };
  return (
    <Modal title={title} onClose={onClose} footer={<><button className="btn" onClick={onClose}>Cancel</button><button className="btn primary" onClick={submit}>{confirmLabel}</button></>}>
      <form onSubmit={(e) => { e.preventDefault(); submit(); }}>
        <TextField label={label} hint={hint} value={v} onChange={(x) => { setV(x); setServerErr(undefined); }} onBlur={() => setTouched(true)} error={err} autoFocus />
      </form>
    </Modal>
  );
}

/** Confirmation dialog for destructive actions, replacing window.confirm. */
export function ConfirmDialog({ title, children, confirmLabel = "Delete", danger = true, onConfirm, onClose }: { title: string; children: ReactNode; confirmLabel?: string; danger?: boolean; onConfirm: () => Promise<void> | void; onClose: () => void }) {
  const [busy, setBusy] = useState(false);
  return (
    <Modal title={title} onClose={onClose} footer={<><button className="btn" onClick={onClose}>Cancel</button><button className={"btn " + (danger ? "danger" : "primary")} disabled={busy} onClick={async () => { setBusy(true); try { await onConfirm(); } finally { setBusy(false); onClose(); } }}>{confirmLabel}</button></>}>
      <div style={{ color: "var(--text-2)" }}>{children}</div>
    </Modal>
  );
}
