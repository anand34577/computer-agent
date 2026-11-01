import { isValidElement, memo, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import rehypeKatex from "rehype-katex";
import rehypeHighlight from "rehype-highlight";
import {
  AlertTriangle, ArrowDown, ArrowUp, Bot, Brain, Camera, Check, ChevronDown, ClipboardCopy, Copy, Database, FileCode, FileJson, FileText, Folder, GitBranch, GitFork,
  Gauge, ListChecks, MoreHorizontal, MousePointerClick, Paperclip, Pause, Pencil, Pin, PinOff, Play, Printer, RefreshCw, Search, ShieldAlert, Square, SlidersHorizontal,
  Sparkles, ThumbsDown, ThumbsUp, Trash2, Volume2, VolumeX, WrapText, Wrench, X,
} from "lucide-react";
import { api, errText, fileToAttachment, money, on, type Attachment, type Conversation, type DocRef, type LlamaStatus, type Message } from "../api";
import { Callout, Checkbox, ConfirmDialog, Menu, Modal, NumberField, PromptDialog, Seg, Slider, Spinner, useApp } from "../components/common";
import { printPdf, saveExport, toHTML, toJSON, toMarkdown } from "../exporters";
import ModelPicker from "../components/ModelPicker";
import { live, useLive, type LiveAgent, type LiveTool } from "../live";

/** Copies text and briefly shows a check mark. */
function useCopy(): [string | null, (key: string, text: string) => void] {
  const [done, setDone] = useState<string | null>(null);
  return [done, (key, text) => {
    navigator.clipboard.writeText(text).then(() => {
      setDone(key);
      setTimeout(() => setDone((d) => (d === key ? null : d)), 1400);
    });
  }];
}

/** Plain text of rendered React nodes (highlighted code is a tree of spans). */
function nodeText(n: ReactNode): string {
  if (n == null || typeof n === "boolean") return "";
  if (typeof n === "string" || typeof n === "number") return String(n);
  if (Array.isArray(n)) return n.map(nodeText).join("");
  if (isValidElement(n)) return nodeText((n.props as { children?: ReactNode }).children);
  return "";
}

/** Fenced code with a language label, line wrapping and a copy button. */
function CodeBlock({ children }: { children?: ReactNode }) {
  const [done, copy] = useCopy();
  const [wrap, setWrap] = useState(false);
  const code = isValidElement(children) ? (children.props as { className?: string; children?: ReactNode }) : undefined;
  const lang = (code?.className ?? "").split(" ").find((c) => c.startsWith("language-"))?.replace("language-", "") ?? "";
  const text = nodeText(code?.children ?? children).replace(/\n$/, "");
  const lines = text.split("\n").length;
  return (
    <div className="code-wrap">
      <div className="code-head">
        <span className="code-lang">{lang || "text"}</span>
        <span className="muted">{lines} line{lines === 1 ? "" : "s"}</span>
        <span className="grow" />
        <button className={"icon-btn" + (wrap ? " active" : "")} onClick={() => setWrap(!wrap)} aria-pressed={wrap} aria-label="Wrap long lines" title="Wrap long lines"><WrapText size={13} /></button>
        <button className={"icon-btn" + (done ? " copied" : "")} onClick={() => copy("c", text)} aria-label="Copy code" title="Copy code">{done ? <Check size={13} /> : <Copy size={13} />}</button>
      </div>
      <pre className={wrap ? "wrap" : undefined}>{children}</pre>
    </div>
  );
}

const MD_REMARK = [remarkGfm, remarkMath];
const MD_REHYPE = [rehypeKatex, [rehypeHighlight, { detect: true }]] as never;
const MD = memo(function MD({ text }: { text: string }) {
  return (
    <ReactMarkdown remarkPlugins={MD_REMARK} rehypePlugins={MD_REHYPE}
      components={{
        pre: CodeBlock,
        a: ({ href, children }) => <a href={href} onClick={(e) => { e.preventDefault(); if (href && /^https?:/i.test(href)) api.openLink(href); }}>{children}</a>,
      }}>
      {text}
    </ReactMarkdown>
  );
});

/** Live or saved reasoning. Open while the model thinks, folds away when the answer starts. */
function Thinking({ text, live: isLive, seconds }: { text: string; live: boolean; seconds?: number }) {
  const [open, setOpen] = useState(isLive);
  const body = useRef<HTMLDivElement>(null);
  useEffect(() => { if (!isLive) setOpen(false); }, [isLive]);
  useEffect(() => { if (isLive && body.current) body.current.scrollTop = body.current.scrollHeight; }, [text, isLive]);
  return (
    <details className={"thinking" + (isLive ? " live" : "")} open={open} onToggle={(e) => setOpen((e.target as HTMLDetailsElement).open)}>
      <summary>
        {isLive ? <Spinner size={14} /> : <Brain size={14} />}
        <span className={isLive ? "shimmer-text" : undefined}>{isLive ? "Thinking" : seconds ? `Thought for ${seconds}s` : "Reasoning"}{isLive && seconds ? ` · ${seconds}s` : ""}</span>
        <ChevronDown size={14} className="chev" />
      </summary>
      <div className="thought" ref={body}>{text}</div>
    </details>
  );
}

function ToolDrawer({ t }: { t: LiveTool }) {
  const running = t.status === "running";
  const isDoc = t.name === "document search";
  const isSub = t.name.startsWith("sub-agent");
  const Icon = isDoc ? Search : isSub ? Bot : Wrench;
  const args = t.args && typeof t.args === "object" && Object.keys(t.args as object).length ? "```json\n" + JSON.stringify(t.args, null, 2) + "\n```" : "";
  const failed = (t.result ?? "").startsWith("Error");
  return (
    <details className={"drawer tool" + (failed ? " failed" : "")}>
      <summary>{running ? <Spinner size={13} /> : <Icon size={13} />} <span className="tool-name">{t.name}</span> <span className="muted">· {running ? "running…" : failed ? "failed" : "done"}</span><ChevronDown size={13} className="chev" /></summary>
      <div className="tool-body">
        {args && <div className="md"><MD text={args} /></div>}
        {t.result && <div className="tool-result">{t.result}</div>}
      </div>
    </details>
  );
}

function useTicker(active: boolean, ms = 1000) {
  const [, set] = useState(0);
  useEffect(() => {
    if (!active) return;
    const t = setInterval(() => set((x) => x + 1), ms);
    return () => clearInterval(t);
  }, [active, ms]);
}

const MAX_IMAGES = 6;
const MAX_IMAGE_MB = 10;
const MAX_DOC_MB = 25;
const MAX_CHARS = 100_000;
const DOC_EXT = /\.(pdf|docx|pptx|xlsx|txt|md|markdown|rst|csv|tsv|json|jsonl|ya?ml|toml|ini|cfg|conf|log|xml|html?|rs|py|m?js|cjs|tsx?|jsx|java|kt|c|h|cpp|hpp|cc|cs|go|rb|php|swift|scala|sh|bash|ps1|bat|sql|r|lua|dart|vue|svelte|css|scss|less|tex|srt|vtt)$/i;

function splitThink(text: string): { think: string; body: string } {
  const m = text.match(/^\s*<think>([\s\S]*?)(<\/think>|$)/);
  if (!m) return { think: "", body: text };
  return { think: m[1].trim(), body: text.slice(m[0].length).trim() };
}

const fmtK = (n: number) => (n >= 1000 ? `${(n / 1000).toFixed(n >= 10000 ? 0 : 1)}K` : String(n));

const STARTERS = [
  { t: "Organise my Downloads folder by file type", agent: true },
  { t: "Open Notepad and write a short to-do list for today", agent: true },
  { t: "Find all PDFs in Documents modified this month", agent: false },
  { t: "Summarise the text on my clipboard", agent: false },
];

const SLASH = [
  { cmd: "/agent", desc: "Run the rest of the message as a computer task" },
  { cmd: "/chat", desc: "Send as a normal chat message" },
  { cmd: "/new", desc: "Start a new chat" },
  { cmd: "/settings", desc: "Instructions, temperature and reply length for this chat" },
];

type PendingDoc = { key: string; name: string; status: "uploading" | "ready" | "error"; ref?: DocRef; error?: string; mode: "embed" | "direct" };

export default function ChatView({ convId, setConvId, refreshConvs, agentRunning, goModels, playbooks }: {
  convId: string | null; setConvId: (id: string | null) => void; refreshConvs: () => Promise<void>; agentRunning: boolean; goModels: () => void;
  playbooks: { id: string; name: string; goal: string }[];
}) {
  const { settings, models, toast, setSettings } = useApp();
  const [conv, setConv] = useState<Conversation | null>(null);
  const lv = useLive(convId);
  const reply = lv.reply;
  const [notices, setNotices] = useState<string[]>([]);
  const [input, setInput] = useState("");
  const [atts, setAtts] = useState<Attachment[]>([]);
  const [docs, setDocs] = useState<PendingDoc[]>([]);
  const [docAsk, setDocAsk] = useState<File[] | null>(null);
  const [agentMode, setAgentMode] = useState(false);
  const [modelId, setModelId] = useState<string | null>(settings.default_model);
  const [editing, setEditing] = useState<{ id: string; text: string } | null>(null);
  const [editTitle, setEditTitle] = useState(false);
  const [chatSettings, setChatSettings] = useState(false);
  const [folderDlg, setFolderDlg] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [confirmFull, setConfirmFull] = useState(false);
  const [thinkOn, setThinkOn] = useState(() => { try { return localStorage.getItem("think") !== "off"; } catch { return true; } });
  const [atBottom, setAtBottom] = useState(true);
  const [speaking, setSpeaking] = useState<string | null>(null);
  const [engine, setEngine] = useState<LlamaStatus | null>(null);
  const scroller = useRef<HTMLDivElement>(null);
  const stick = useRef(true);
  const lastScroll = useRef({ top: 0, height: 0 });
  const fileRef = useRef<HTMLInputElement>(null);
  const taRef = useRef<HTMLTextAreaElement>(null);
  const convRef = useRef<string | null>(convId);
  convRef.current = convId;
  const createdHere = useRef<string | null>(null);

  const chatModels = useMemo(() => models.filter((m) => !m.effective.embedding), [models]);
  const model = chatModels.find((m) => m.id === modelId) ?? null;
  // The thinking switch only exists for models that can reason; null leaves the provider default.
  const canThink = !!model?.effective.reasoning;
  const think = canThink ? thinkOn : null;
  const toggleThink = () => {
    const next = !thinkOn;
    setThinkOn(next);
    try { localStorage.setItem("think", next ? "on" : "off"); } catch { /* storage unavailable */ }
  };

  const load = useCallback(async (id: string) => {
    try {
      const c = await api.getConversation(id);
      if (convRef.current !== id) return;
      setConv(c);
      if (c.model_id && models.some((m) => m.id === c.model_id && !m.effective.embedding)) setModelId(c.model_id);
    } catch {
      setConv(null);
    }
  }, [models]);

  useEffect(() => {
    if (convId && convId === createdHere.current) {
      createdHere.current = null;
      load(convId);
      return;
    }
    setNotices([]);
    setEditing(null);
    setDocs([]);
    stick.current = true;
    if (convId) load(convId);
    else setConv(null);
    taRef.current?.focus();
  }, [convId]);

  useEffect(() => {
    if (!modelId || !chatModels.some((m) => m.id === modelId)) setModelId(settings.default_model && chatModels.some((m) => m.id === settings.default_model) ? settings.default_model : chatModels[0]?.id ?? null);
  }, [settings.default_model, chatModels]);

  // A reply or task finished: show the saved message, then drop the live copy (no flash in between).
  useEffect(() => {
    if (!convId || !lv.done) return;
    load(convId).then(() => { live.clearReply(convId); refreshConvs(); });
  }, [lv.done]);

  useEffect(() => {
    api.llamaStatus().then(setEngine).catch(() => {});
    return on<LlamaStatus>("llama-status", setEngine);
  }, []);

  // Follow the reply only while the reader is at the bottom. Scrolling up, by any means, stops it.
  const onScroll = () => {
    const el = scroller.current;
    if (!el) return;
    const dist = el.scrollHeight - el.scrollTop - el.clientHeight;
    const prev = lastScroll.current;
    if (el.scrollTop < prev.top - 2 && el.scrollHeight >= prev.height) stick.current = false;
    if (dist < 6) stick.current = true;
    lastScroll.current = { top: el.scrollTop, height: el.scrollHeight };
    setAtBottom(dist < 120);
  };
  useLayoutEffect(() => {
    const el = scroller.current;
    if (el && stick.current) {
      el.scrollTop = el.scrollHeight;
      lastScroll.current = { top: el.scrollTop, height: el.scrollHeight };
    }
  });
  useEffect(() => () => window.speechSynthesis?.cancel(), [convId]);
  const waiting = !!reply && !reply.pieces;
  useTicker(!!reply || !!lv.agent, waiting ? 1000 : 500);
  const toBottom = () => {
    stick.current = true;
    scroller.current?.scrollTo({ top: scroller.current.scrollHeight, behavior: "smooth" });
  };

  const switchModel = (id: string) => {
    const next = models.find((m) => m.id === id);
    const hasImages = conv?.messages.some((m) => m.images?.length);
    if (model && next && model.id !== next.id) {
      if (model.effective.vision && !next.effective.vision && hasImages) setNotices((n) => [...n, `${next.label} cannot read images, so earlier images will be left out of its context.`]);
      if (model.effective.tools && !next.effective.tools && settings.chat_tools) setNotices((n) => [...n, `${next.label} has no tool calling, so file and web tools are off for its replies.`]);
    }
    setModelId(id);
    if (conv) api.updateConversation(conv.id, { model_id: id });
  };

  const inputError = input.length > MAX_CHARS ? `Message is too long (${input.length.toLocaleString()} of ${MAX_CHARS.toLocaleString()} characters)` : undefined;
  const docsBusy = docs.some((d) => d.status === "uploading");

  const ensureConv = async (): Promise<string | null> => {
    if (convId) return convId;
    if (!modelId) { toast("Add a model first.", "bad"); goModels(); return null; }
    const id = await api.createConversation(modelId);
    convRef.current = id;
    createdHere.current = id;
    setConvId(id);
    refreshConvs();
    return id;
  };

  const send = async (raw = input) => {
    let text = raw;
    let asAgent = agentMode;
    const slash = text.trim().match(/^\/(\w+)\s*([\s\S]*)$/);
    if (slash) {
      const [, cmd, rest] = slash;
      if (cmd === "new") { setInput(""); setConvId(null); return; }
      if (cmd === "settings" || cmd === "persona") { setInput(""); if (conv) setChatSettings(true); else toast("Send a message first, then change this chat's settings.", "info"); return; }
      if (cmd === "agent") { asAgent = true; text = rest; }
      else if (cmd === "chat") { asAgent = false; text = rest; }
    }
    const readyDocs = docs.filter((d) => d.status === "ready" && d.ref).map((d) => d.ref!);
    if (!text.trim() && atts.length === 0 && readyDocs.length === 0) return;
    if (inputError || docsBusy) return;
    if (!modelId) { toast("Add a model first.", "bad"); goModels(); return; }
    if (asAgent && text.trim().length < 3) { toast("Describe the task in a few words.", "bad"); return; }
    try {
      const id = await ensureConv();
      if (!id) return;
      stick.current = true;
      if (asAgent) {
        await api.startTask(text.trim(), id, modelId);
        setInput("");
        await load(id);
        refreshConvs();
        return;
      }
      setInput("");
      const a = atts;
      setAtts([]);
      setDocs([]);
      live.clearNotices(id);
      live.pending(id, model?.label ?? "");
      await api.sendMessage(id, text, a, modelId, think, readyDocs);
      await load(id);
      refreshConvs();
    } catch (e) {
      if (convRef.current) live.clearReply(convRef.current);
      toast(errText(e), "bad");
    }
  };

  const uploadDocs = async (files: File[], mode: "embed" | "direct") => {
    const id = await ensureConv();
    if (!id) return;
    const items: PendingDoc[] = files.map((f, i) => ({ key: `${Date.now()}-${i}-${f.name}`, name: f.name, status: "uploading", mode }));
    setDocs((d) => [...d, ...items]);
    await Promise.all(files.map(async (f, i) => {
      const key = items[i].key;
      try {
        const { data } = await fileToAttachment(f);
        const ref = await api.docsAdd(id, f.name, mode, { data });
        setDocs((d) => d.map((x) => (x.key === key ? { ...x, status: "ready", ref } : x)));
      } catch (e) {
        setDocs((d) => d.map((x) => (x.key === key ? { ...x, status: "error", error: errText(e) } : x)));
      }
    }));
  };

  const attachFiles = async (files: FileList | File[]) => {
    const list = [...files];
    const imgs = list.filter((f) => f.type.startsWith("image/"));
    const docFiles = list.filter((f) => !f.type.startsWith("image/") && DOC_EXT.test(f.name));
    const other = list.length - imgs.length - docFiles.length;
    const tooBig = imgs.filter((f) => f.size > MAX_IMAGE_MB * 1024 * 1024);
    const ok = imgs.filter((f) => f.size <= MAX_IMAGE_MB * 1024 * 1024).slice(0, Math.max(0, MAX_IMAGES - atts.length));
    if (tooBig.length) toast(`${tooBig.length} image(s) are larger than ${MAX_IMAGE_MB} MB and were skipped.`, "bad");
    if (imgs.length - tooBig.length > ok.length) toast(`You can attach up to ${MAX_IMAGES} images per message.`, "bad");
    if (other > 0) toast(`${other} file(s) were skipped: use images, PDF, Word, PowerPoint, Excel, text, CSV, JSON or code files.`, "info");
    if (ok.length) {
      const out = await Promise.all(ok.map(fileToAttachment));
      setAtts((a) => [...a, ...out]);
    }
    const bigDocs = docFiles.filter((f) => f.size > MAX_DOC_MB * 1024 * 1024);
    if (bigDocs.length) toast(`${bigDocs.length} document(s) are larger than ${MAX_DOC_MB} MB and were skipped.`, "bad");
    const goodDocs = docFiles.filter((f) => f.size <= MAX_DOC_MB * 1024 * 1024);
    if (!goodDocs.length) return;
    const mode = settings.doc_mode;
    if (mode === "ask" || (mode === "embed" && !settings.embed_model)) setDocAsk(goodDocs);
    else uploadDocs(goodDocs, mode);
  };

  const removeDoc = (d: PendingDoc) => {
    setDocs((x) => x.filter((y) => y.key !== d.key));
    if (d.ref) api.docsRemove(d.ref.id).catch(() => {});
  };

  const screenshot = async () => {
    if (atts.length >= MAX_IMAGES) return toast(`You can attach up to ${MAX_IMAGES} images per message.`, "bad");
    try {
      const b64 = await api.captureScreenshot();
      setAtts((a) => [...a, { media_type: "image/jpeg", data: b64, name: "screenshot.jpg" }]);
    } catch (e) {
      toast(errText(e), "bad");
    }
  };

  const doExport = async (fmt: "md" | "json" | "html" | "pdf") => {
    if (!conv) return;
    try {
      if (fmt === "pdf") return printPdf(conv, settings.assistant_name);
      const content = fmt === "md" ? toMarkdown(conv, settings.assistant_name) : fmt === "json" ? toJSON(conv) : toHTML(conv, settings.assistant_name);
      const path = await saveExport(conv.title, fmt, content);
      if (path) toast(`Saved to ${path}`, "ok");
    } catch (e) {
      toast(errText(e), "bad");
    }
  };

  const fork = async (messageId?: string) => {
    if (!conv) return;
    try {
      const id = messageId ? await api.branchConversation(conv.id, messageId) : await api.forkConversation(conv.id);
      await refreshConvs();
      setConvId(id);
      toast(messageId ? "Branched into a new chat from that message" : "Forked into a new chat", "ok");
    } catch (e) {
      toast(errText(e), "bad");
    }
  };

  const speak = (m: Message) => {
    const synth = window.speechSynthesis;
    if (!synth) return toast("Read aloud is not available on this system.", "bad");
    synth.cancel();
    if (speaking === m.id) return setSpeaking(null);
    const plain = m.content.replace(/```[\s\S]*?```/g, " code block ").replace(/[#*_`>|]/g, "");
    const u = new SpeechSynthesisUtterance(plain);
    u.onend = () => setSpeaking((s) => (s === m.id ? null : s));
    setSpeaking(m.id);
    synth.speak(u);
  };

  const busy = !!reply;
  const lastAssistant = useMemo(() => [...(conv?.messages ?? [])].reverse().find((m) => m.role === "assistant"), [conv]);
  const lastUser = useMemo(() => [...(conv?.messages ?? [])].reverse().find((m) => m.role === "user"), [conv]);
  const agentCapable = !!model && (model.effective.vision || !!settings.roles.eyes || navigator.userAgent.includes("Windows"));
  const slashOpen = input.startsWith("/") && !input.includes(" ") ? SLASH.filter((s) => s.cmd.startsWith(input)) : [];

  // Rough context use: about 4 characters per token.
  const ctxUsed = useMemo(() => {
    const chars = (conv?.messages ?? []).reduce((n, m) => n + m.content.length + (m.images?.length ?? 0) * 3000, 0) + input.length + 1500;
    return Math.round(chars / 4);
  }, [conv, input]);
  const ctxMax = model?.effective.context || 0;
  const ctxPct = ctxMax ? Math.min(100, Math.round((ctxUsed / ctxMax) * 100)) : 0;

  // Stable handlers keep saved messages from re-rendering on every streamed token.
  const handlers = useRef<(kind: string, m: Message, arg?: unknown) => void>(() => {});
  handlers.current = async (kind, m, arg) => {
    if (!conv || !modelId) return;
    if (kind === "edit") setEditing(arg === null ? null : { id: m.id, text: arg as string });
    if (kind === "save" && editing) {
      if (!editing.text.trim()) return toast("The message cannot be empty.", "bad");
      stick.current = true;
      live.pending(conv.id, model?.label ?? "");
      await api.editAndResend(conv.id, m.id, editing.text, modelId, think).catch((e) => { live.clearReply(conv.id); toast(errText(e), "bad"); });
      setEditing(null);
      load(conv.id);
    }
    if (kind === "regen") {
      stick.current = true;
      live.pending(conv.id, model?.label ?? "");
      await api.regenerate(conv.id, m.id, modelId, think).catch((e) => { live.clearReply(conv.id); toast(errText(e), "bad"); });
      load(conv.id);
    }
    if (kind === "branch") fork(m.id);
    if (kind === "rate") { await api.rateMessage(conv.id, m.id, arg as number); load(conv.id); }
    if (kind === "speak") speak(m);
  };
  const act = useCallback((kind: string, m: Message, arg?: unknown) => handlers.current(kind, m, arg), []);

  const fullAccess = settings.autonomy === "full";
  const liveTps = reply?.firstAt && reply.pieces > 4 ? reply.pieces / Math.max(0.5, (Date.now() - reply.firstAt) / 1000) : 0;
  const elapsed = reply ? Math.max(0, Math.round((Date.now() - reply.started) / 1000)) : 0;
  const engineLoading = engine?.state === "loading" && model?.provider_kind === "llamacpp";

  return (
    <div className="chat">
      <div className="chat-head">
        <div className="title ellipsis">
          {conv && editTitle ? (
            <input className="input" autoFocus defaultValue={conv.title} maxLength={120}
              onBlur={async (e) => { const t = e.target.value.trim(); if (t && t !== conv.title) { await api.updateConversation(conv.id, { title: t }); load(conv.id); refreshConvs(); } setEditTitle(false); }}
              onKeyDown={(e) => { if (e.key === "Enter") (e.target as HTMLInputElement).blur(); if (e.key === "Escape") setEditTitle(false); }} />
          ) : (
            <span onDoubleClick={() => conv && setEditTitle(true)} title={conv ? "Double-click to rename" : undefined}>{conv?.title ?? "New chat"}</span>
          )}
        </div>
        <ModelPicker models={chatModels} value={modelId} onChange={switchModel} onManage={goModels} />
        {conv && (
          <Menu label="Chat actions" trigger={(o) => <button className={"icon-btn" + (o ? " active" : "")} title="More" aria-label="Chat actions"><MoreHorizontal size={17} /></button>}
            items={[
              { label: "Rename", icon: Pencil, onClick: () => setEditTitle(true) },
              { label: conv.pinned ? "Unpin" : "Pin to top", icon: conv.pinned ? PinOff : Pin, onClick: async () => { await api.updateConversation(conv.id, { pinned: !conv.pinned }); load(conv.id); refreshConvs(); } },
              { label: "Move to folder", icon: Folder, onClick: () => setFolderDlg(true) },
              { label: "Chat settings", icon: SlidersHorizontal, onClick: () => setChatSettings(true) },
              { label: "Fork chat", icon: GitFork, onClick: () => fork() },
              "sep",
              { label: "Copy as Markdown", icon: ClipboardCopy, onClick: () => { navigator.clipboard.writeText(toMarkdown(conv, settings.assistant_name)); toast("Chat copied as Markdown", "ok"); } },
              { label: "Export as Markdown", icon: FileText, onClick: () => doExport("md") },
              { label: "Export as PDF", icon: Printer, onClick: () => doExport("pdf") },
              { label: "Export as web page", icon: FileCode, onClick: () => doExport("html") },
              { label: "Export as JSON", icon: FileJson, onClick: () => doExport("json") },
              "sep",
              { label: "Delete chat", icon: Trash2, danger: true, onClick: () => setConfirmDelete(true) },
            ]} />
        )}
      </div>

      <div className="messages" ref={scroller} onScroll={onScroll}>
        {!conv?.messages.length && !reply && !lv.agent && (
          <div className="welcome">
            <h1>What should we do today?</h1>
            <p className="muted" style={{ marginTop: 0 }}>Chat as usual, attach documents, or switch on <b>Agent</b> to let {settings.assistant_name} operate your computer. You approve the plan, risky steps ask first, and <span className="kbd">{settings.stop_shortcut}</span> stops everything.</p>
            {chatModels.length === 0 ? (
              <Callout kind="warn" action={<button className="btn sm primary" onClick={goModels}>Add a model</button>}>Connect a model to get started: the built-in engine, a local runtime such as Ollama or LM Studio, or a cloud provider with your key.</Callout>
            ) : settings.show_suggestions && (
              <>
                <div className="grid2" style={{ marginTop: 18 }}>
                  {STARTERS.map((s) => (
                    <button key={s.t} className="choice" onClick={() => { setAgentMode(s.agent); setInput(s.t); taRef.current?.focus(); }}>
                      <span className={"badge " + (s.agent ? "accent" : "")}>{s.agent ? <><MousePointerClick size={12} /> Agent</> : <><Sparkles size={12} /> Chat</>}</span>
                      <span>{s.t}</span>
                    </button>
                  ))}
                </div>
                {playbooks.length > 0 && (
                  <>
                    <div className="side-section" style={{ padding: "20px 0 8px" }}>Your playbooks</div>
                    <div className="row wrap">
                      {playbooks.slice(0, 8).map((p) => (
                        <button key={p.id} className="chip" title={p.goal} onClick={() => { setAgentMode(true); setInput(p.goal); taRef.current?.focus(); }}><Play size={13} /> {p.name}</button>
                      ))}
                    </div>
                  </>
                )}
              </>
            )}
          </div>
        )}
        {conv?.messages.map((m) => (
          <MessageItem key={m.id} m={m} assistantName={settings.assistant_name} isLastAssistant={m.id === lastAssistant?.id}
            editing={editing?.id === m.id ? editing.text : null} act={act} speaking={speaking === m.id} showReasoning={settings.show_reasoning} busy={busy} />
        ))}
        {reply && (
          <div className="msg live-msg">
            <div className="who"><span className="avatar ai"><Bot size={13} /></span> {settings.assistant_name}<span className="muted" style={{ fontWeight: 500 }}>· {reply.model}</span></div>
            {reply.reasoning && settings.show_reasoning && (
              <Thinking text={reply.reasoning} live={!reply.text && !reply.tools.some((t) => t.status === "done")} seconds={Math.max(1, Math.round(((reply.thinkEnd ?? Date.now()) - (reply.firstAt ?? reply.started)) / 1000))} />
            )}
            {reply.reasoning && !settings.show_reasoning && !reply.text && <div className="wait-line"><Spinner size={13} /> <span className="shimmer-text">Thinking</span></div>}
            {reply.tools.map((t, i) => <ToolDrawer key={i} t={t} />)}
            {waiting && !reply.tools.length ? (
              <div className="wait-line" role="status">
                <Spinner size={13} />
                <span className="shimmer-text">{engineLoading ? `Loading ${reply.model} into memory` : `Waiting for ${reply.model || "the model"}`}</span>
                <span className="muted">{elapsed}s</span>
                {elapsed >= 4 && !engineLoading && <span className="muted">· the model is reading the conversation{ctxUsed > 3000 ? ` (~${fmtK(ctxUsed)} tokens)` : ""}</span>}
              </div>
            ) : reply.text ? (
              <div className="md streaming"><MD text={splitThink(reply.text).body} /></div>
            ) : null}
            {reply.pieces > 0 && (
              <div className="msg-actions show">
                <span className="msg-meta"><Gauge size={12} /> {liveTps ? `${liveTps.toFixed(1)} tok/s` : "…"} · {reply.pieces.toLocaleString()} tokens{reply.firstAt ? ` · first token ${((reply.firstAt - reply.started) / 1000).toFixed(1)}s` : ""}</span>
              </div>
            )}
          </div>
        )}
        {lv.agent && <AgentLive a={lv.agent} />}
        {[...lv.notices, ...notices].map((n, i) => <div className="notice" key={i}><Callout kind="info">{n}</Callout></div>)}
        {lv.error && (
          <div className="notice">
            <Callout kind="bad" action={lv.error.fallback && conv ? (
              <button className="btn sm" onClick={async () => {
                const fb = lv.error!.fallback!;
                live.clearError(conv.id);
                setModelId(fb);
                if (lastUser) { live.pending(conv.id, models.find((x) => x.id === fb)?.label ?? ""); await api.editAndResend(conv.id, lastUser.id, lastUser.content, fb); load(conv.id); }
              }}>Retry with fallback model</button>
            ) : lastUser && conv ? (
              <button className="btn sm" onClick={async () => {
                if (!modelId) return;
                live.clearError(conv.id);
                live.pending(conv.id, model?.label ?? "");
                await api.editAndResend(conv.id, lastUser.id, lastUser.content, modelId, think).catch((e) => { live.clearReply(conv.id); toast(errText(e), "bad"); });
                load(conv.id);
              }}><RefreshCw size={13} /> Retry</button>
            ) : undefined}>{lv.error.text}</Callout>
          </div>
        )}
      </div>

      {!atBottom && ((conv?.messages.length ?? 0) > 0 || reply) && (
        <div style={{ position: "relative", height: 0 }}>
          <button className="btn sm scroll-down" onClick={toBottom} aria-label="Scroll to the latest message"><ArrowDown size={14} /> {reply ? "Follow reply" : "Latest"}</button>
        </div>
      )}
      <div className="composer" onDragOver={(e) => { e.preventDefault(); e.currentTarget.classList.add("drag"); }} onDragLeave={(e) => e.currentTarget.classList.remove("drag")}
        onDrop={(e) => { e.preventDefault(); e.currentTarget.classList.remove("drag"); if (!agentMode) attachFiles(e.dataTransfer.files); }}>
        {fullAccess && (
          <div className="full-banner"><ShieldAlert size={14} /> Full access is on: the assistant and agent act without asking. <button className="link" onClick={() => setSettings({ ...settings, autonomy: "balanced" })}>Turn off</button></div>
        )}
        <div className={"composer-box " + (agentMode ? "agent" : "")} style={{ position: "relative" }}>
          {slashOpen.length > 0 && (
            <div className="popover up" style={{ minWidth: 340 }}>
              {slashOpen.map((s) => (
                <button key={s.cmd} className="menu-item" onClick={() => { setInput(s.cmd + " "); taRef.current?.focus(); }}><b className="mono">{s.cmd}</b><span className="muted small">{s.desc}</span></button>
              ))}
            </div>
          )}
          {(atts.length > 0 || docs.length > 0) && (
            <div className="thumbs">
              {atts.map((a, i) => (
                <div className="th" key={i}>
                  <img src={`data:${a.media_type};base64,${a.data}`} alt={a.name ?? "attachment"} />
                  <button className="x" onClick={() => setAtts(atts.filter((_, j) => j !== i))} aria-label="Remove attachment"><X size={11} /></button>
                </div>
              ))}
              {docs.map((d) => (
                <div key={d.key} className={"doc-chip " + d.status} title={d.error ?? (d.ref ? `${d.ref.chars.toLocaleString()} characters${d.ref.chunks ? ` · ${d.ref.chunks} chunks` : ""}` : "Reading…")}>
                  {d.status === "uploading" ? <Spinner size={13} /> : d.status === "error" ? <AlertTriangle size={13} /> : d.mode === "embed" ? <Database size={13} /> : <FileText size={13} />}
                  <span className="ellipsis">{d.name}</span>
                  <span className="muted small">{d.status === "uploading" ? (d.mode === "embed" ? "embedding…" : "reading…") : d.status === "error" ? "failed" : d.mode === "embed" ? "search" : `~${fmtK(Math.round((d.ref?.chars ?? 0) / 4))} tok`}</span>
                  <button className="x" onClick={() => removeDoc(d)} aria-label={`Remove ${d.name}`}><X size={11} /></button>
                </div>
              ))}
            </div>
          )}
          {docs.some((d) => d.status === "error") && <div className="field-error" style={{ marginBottom: 4 }}>{docs.find((d) => d.status === "error")?.error}</div>}
          {atts.length > 0 && model && !model.effective.vision && <div className="field-error" style={{ marginBottom: 4 }}>{model.label} cannot read images; they will be left out.</div>}
          <textarea
            ref={taRef}
            value={input}
            onChange={(e) => setInput(e.target.value)}
            placeholder={agentMode ? "Describe a task, e.g. “Rename the PDFs in Downloads by date”" : `Message ${settings.assistant_name}  ·  type / for commands`}
            onKeyDown={(e) => {
              const sendKey = settings.send_on_enter ? e.key === "Enter" && !e.shiftKey : e.key === "Enter" && (e.ctrlKey || e.metaKey);
              if (sendKey) {
                e.preventDefault();
                if (!busy) send();
              }
              if (e.key === "Escape" && busy && conv) { e.preventDefault(); api.stopChat(conv.id); }
              if (e.key === "ArrowUp" && !input && lastUser && !busy) { e.preventDefault(); setEditing({ id: lastUser.id, text: lastUser.content }); }
            }}
            onPaste={(e) => {
              const files = [...e.clipboardData.files];
              if (files.length) { e.preventDefault(); attachFiles(files); }
            }}
            rows={Math.min(8, Math.max(2, input.split("\n").length))}
            aria-label="Message"
            aria-invalid={!!inputError}
          />
          {inputError && <div className="field-error">{inputError}</div>}
          <div className="composer-tools">
            <input ref={fileRef} type="file" multiple hidden accept="image/*,.pdf,.docx,.pptx,.xlsx,.txt,.md,.csv,.tsv,.json,.jsonl,.yaml,.yml,.toml,.xml,.html,.htm,.log,.ini,.rs,.py,.js,.ts,.tsx,.jsx,.java,.kt,.c,.h,.cpp,.cs,.go,.rb,.php,.swift,.sh,.ps1,.sql,.css,.tex"
              onChange={(e) => { if (e.target.files) attachFiles(e.target.files); e.target.value = ""; }} />
            <button className="icon-btn" title="Attach images or documents" aria-label="Attach files" onClick={() => fileRef.current?.click()} disabled={agentMode}><Paperclip size={16} /></button>
            <button className="icon-btn" title="Attach a screenshot of your screen" aria-label="Attach screenshot" onClick={screenshot} disabled={agentMode}><Camera size={16} /></button>
            {canThink && !agentMode && (
              <button className={"mode-toggle " + (thinkOn ? "on" : "")} onClick={toggleThink} aria-pressed={thinkOn}
                title={thinkOn ? "Thinking is on: the model reasons before answering. Click for faster answers without thinking." : "Thinking is off. Click to let the model reason before answering."}>
                <Brain size={14} /> Think
              </button>
            )}
            <button className={"mode-toggle " + (agentMode ? "on" : "")} onClick={() => setAgentMode(!agentMode)} disabled={!agentCapable} aria-pressed={agentMode}
              title={agentCapable ? "Let the agent operate your computer" : "The selected model cannot see the screen; add a vision model or an Eyes model"}>
              <MousePointerClick size={14} /> Agent
            </button>
            {agentMode && (
              <Seg value={settings.autonomy} label="Autonomy" onChange={(v) => (v === "full" && settings.autonomy !== "full" ? setConfirmFull(true) : setSettings({ ...settings, autonomy: v }))}
                options={[{ value: "supervised", label: "Supervised" }, { value: "balanced", label: "Balanced" }, { value: "autonomous", label: "Autonomous" }, { value: "full", label: "Full access" }]} />
            )}
            <span className="grow" />
            {ctxMax > 0 && !agentMode && (
              <span className={"ctx-meter" + (ctxPct > 85 ? " hot" : "")} title={`About ${ctxUsed.toLocaleString()} of ${ctxMax.toLocaleString()} tokens of ${model?.label}'s context are used. Older messages are left out when it is full.`}>
                <span className="ctx-bar"><i style={{ width: `${Math.max(3, ctxPct)}%` }} /></span>{fmtK(ctxUsed)} / {fmtK(ctxMax)}
              </span>
            )}
            {agentMode && agentRunning && <span className="small muted">A task is running; new ones queue</span>}
            {busy ? (
              <button className="send-btn" onClick={() => conv && api.stopChat(conv.id)} aria-label="Stop reply" title="Stop (Esc)"><Square size={13} fill="currentColor" /></button>
            ) : (
              <button className="send-btn" onClick={() => send()} disabled={(!input.trim() && !atts.length && !docs.some((d) => d.status === "ready")) || !modelId || !!inputError || docsBusy}
                aria-label={agentMode ? "Start task" : "Send"} title={docsBusy ? "Waiting for documents" : agentMode ? "Start task" : "Send"}>
                {agentMode ? <Play size={15} /> : <ArrowUp size={17} />}
              </button>
            )}
          </div>
        </div>
        <div className="composer-hint">{settings.send_on_enter ? "Enter to send · Shift+Enter for a new line" : "Ctrl+Enter to send"} · Esc stops · ↑ edits your last message · drop files to attach</div>
      </div>

      {docAsk && (
        <DocModeDialog files={docAsk} hasEmbed={!!settings.embed_model} context={ctxMax}
          onSetup={() => { setDocAsk(null); goModels(); }}
          onClose={() => setDocAsk(null)}
          onChoose={(mode, remember) => {
            const files = docAsk;
            setDocAsk(null);
            if (remember) setSettings({ ...settings, doc_mode: mode });
            uploadDocs(files, mode);
          }} />
      )}
      {chatSettings && conv && <ChatSettings conv={conv} onClose={() => setChatSettings(false)} onSaved={() => { load(conv.id); toast("Chat settings saved", "ok"); }} />}
      {confirmFull && (
        <ConfirmDialog title="Give the assistant full access?" confirmLabel="Turn on full access" onClose={() => setConfirmFull(false)} onConfirm={async () => { await setSettings({ ...settings, autonomy: "full" }); }}>
          <p style={{ marginTop: 0 }}>With full access, the agent and chat tools act on their own: no plan approval, no confirmation before clicking send, deleting, installing, running shell commands or stopping programs.</p>
          <p>Only use it with a model you trust, for tasks you can watch. Passwords, sign-in screens, CAPTCHAs and off-limits apps still hand control back to you, and <span className="kbd">{settings.stop_shortcut}</span> stops everything at once.</p>
        </ConfirmDialog>
      )}
      {confirmDelete && conv && (
        <ConfirmDialog title="Delete chat?" onClose={() => setConfirmDelete(false)} onConfirm={async () => { await api.deleteConversation(conv.id); setConvId(null); await refreshConvs(); toast("Chat deleted", "ok"); }}>
          “{conv.title}” and all its messages and documents will be removed from this computer.
        </ConfirmDialog>
      )}
      {folderDlg && conv && (
        <PromptDialog title="Move to folder" label="Folder name" hint="Leave empty to remove it from a folder" initial={conv.folder}
          validate={(v) => (v.length > 40 ? "Up to 40 characters" : /[\\/]/.test(v) ? "No slashes" : undefined)}
          onSubmit={async (v) => { await api.updateConversation(conv.id, { folder: v }); await refreshConvs(); await load(conv.id); }}
          onClose={() => setFolderDlg(false)} confirmLabel="Move" />
      )}
    </div>
  );
}

const STEP_ICON: Record<string, typeof Bot> = { action: MousePointerClick, thought: Brain, plan: ListChecks, blocked: ShieldAlert, denied: X, stuck: AlertTriangle };

function AgentLive({ a }: { a: LiveAgent }) {
  const steps = a.steps.slice(-8);
  const paused = a.status === "paused";
  const thinking = !a.text && !!a.reasoning;
  return (
    <div className="msg live-msg">
      <div className="who"><span className="avatar ai"><MousePointerClick size={13} /></span> Agent{a.model && <span className="muted" style={{ fontWeight: 500 }}>· {a.model}</span>}<span className="badge accent">step {a.step}</span></div>
      <div className="agent-live">
        <div className="agent-live-head">
          {paused ? <Pause size={14} /> : <Spinner size={14} />}
          <span className="grow ellipsis">{a.message || "Working…"}</span>
          <button className="btn sm" onClick={() => (paused ? api.agentResume() : api.agentPause())}>{paused ? <><Play size={13} /> Resume</> : <><Pause size={13} /> Pause</>}</button>
          <button className="btn sm danger" onClick={() => api.agentStop()}><Square size={12} /> Stop</button>
        </div>
        {steps.length > 0 && (
          <ol className="agent-steps">
            {steps.map((s) => {
              const Icon = STEP_ICON[s.kind] ?? Wrench;
              return (
                <li key={s.id} className={s.kind}>
                  <Icon size={13} /><span className="ellipsis">{s.title}</span>
                  {s.result && s.kind === "action" && <span className="muted small ellipsis">{s.result.slice(0, 80)}</span>}
                </li>
              );
            })}
          </ol>
        )}
        {a.reasoning && <Thinking text={a.reasoning} live={thinking} />}
        {a.text && <div className="agent-thought md"><MD text={a.text.replace(/```[\s\S]*?(```|$)/g, "").trim() || "Deciding the next action…"} /></div>}
      </div>
    </div>
  );
}

function DocModeDialog({ files, hasEmbed, context, onChoose, onClose, onSetup }: {
  files: File[]; hasEmbed: boolean; context: number; onChoose: (mode: "embed" | "direct", remember: boolean) => void; onClose: () => void; onSetup: () => void;
}) {
  const [remember, setRemember] = useState(false);
  const bytes = files.reduce((n, f) => n + f.size, 0);
  // Text files are about 4 bytes per token; PDFs and Office files carry much more than their text.
  const estTokens = Math.round(files.reduce((n, f) => n + f.size / (/\.(pdf|docx|pptx|xlsx)$/i.test(f.name) ? 12 : 4), 0));
  const tooBig = context > 0 && estTokens > context * 0.7;
  return (
    <Modal title={files.length === 1 ? `How should the model use “${files[0].name}”?` : `How should the model use these ${files.length} documents?`} icon={FileText} onClose={onClose}
      subtitle={`${bytes >= 1024 * 1024 ? `${(bytes / 1024 / 1024).toFixed(1)} MB` : `${Math.max(1, Math.round(bytes / 1024))} KB`} · roughly ${fmtK(estTokens)} tokens of text`}>
      <div className="doc-modes">
        <button className="choice doc-mode" onClick={() => onChoose("embed", remember)} disabled={!hasEmbed}>
          <span className="row"><Database size={16} /> <b>Search it (embedding)</b>{tooBig && hasEmbed && <span className="badge ok">Recommended</span>}</span>
          <span className="muted small">Split into passages and indexed with your embedding model. Each question pulls in only the relevant parts, so long documents and many files fit, even in small models.</span>
          {!hasEmbed && <span className="small" style={{ color: "var(--warn)" }}>Needs an embedding model. <span className="link" role="link" tabIndex={0} onClick={(e) => { e.stopPropagation(); onSetup(); }}>Set one up in Models</span></span>}
        </button>
        <button className="choice doc-mode" onClick={() => onChoose("direct", remember)}>
          <span className="row"><FileText size={16} /> <b>Send the full text</b>{!tooBig && <span className="badge ok">Best for short files</span>}</span>
          <span className="muted small">The whole text goes to the model with your message. Most accurate for short files, but uses context with every reply.</span>
          {tooBig && <span className="small" style={{ color: "var(--warn)" }}>This is likely larger than the model's context ({fmtK(context)} tokens); older parts would be cut.</span>}
        </button>
      </div>
      <div style={{ marginTop: 12 }}><Checkbox checked={remember} onChange={setRemember}>Remember my choice (change it in Settings → Documents and sub-agents)</Checkbox></div>
    </Modal>
  );
}

function ChatSettings({ conv, onClose, onSaved }: { conv: Conversation; onClose: () => void; onSaved: () => void }) {
  const [prompt, setPrompt] = useState(conv.system_prompt);
  const [useTemp, setUseTemp] = useState(conv.params?.temperature != null);
  const [temp, setTemp] = useState(conv.params?.temperature ?? 0.7);
  const [useMax, setUseMax] = useState(conv.params?.max_tokens != null);
  const [maxTok, setMaxTok] = useState(conv.params?.max_tokens ?? 4096);
  const [err, setErr] = useState<string | null>(null);
  const save = async () => {
    try {
      await api.updateConversation(conv.id, { system_prompt: prompt, params: { temperature: useTemp ? temp : null, max_tokens: useMax ? maxTok : null } });
      onSaved();
      onClose();
    } catch (e) {
      setErr(errText(e));
    }
  };
  return (
    <Modal title="Chat settings" subtitle="These apply to this chat only, on top of your global settings." icon={SlidersHorizontal} onClose={onClose}
      footer={<><button className="btn" onClick={onClose}>Cancel</button><button className="btn primary" disabled={prompt.length > 8000} onClick={save}>Save</button></>}>
      <label className="label" htmlFor="chat-instr">Instructions</label>
      <textarea id="chat-instr" className={"input" + (prompt.length > 8000 ? " invalid" : "")} rows={6} value={prompt} onChange={(e) => setPrompt(e.target.value)} placeholder="e.g. You are a concise technical editor. Answer in British English." />
      <div className={prompt.length > 8000 ? "field-error" : "field-hint"} style={{ marginTop: 6 }}>{prompt.length.toLocaleString()} / 8,000 characters</div>
      <div className="chat-params">
        <div>
          <Checkbox checked={useTemp} onChange={setUseTemp}>Custom temperature</Checkbox>
          {useTemp ? <Slider label="Temperature" value={temp} min={0} max={2} step={0.05} onChange={setTemp} format={(v) => v.toFixed(2)} /> : <div className="field-hint">The model's default. Lower is more focused, higher more creative.</div>}
        </div>
        <div>
          <Checkbox checked={useMax} onChange={setUseMax}>Limit reply length</Checkbox>
          {useMax ? <NumberField label="Max tokens per reply" value={maxTok} min={16} max={200000} step={256} onCommit={setMaxTok} suffix="tokens" /> : <div className="field-hint">Up to 8,192 tokens per reply.</div>}
        </div>
      </div>
      {err && <Callout kind="bad">{err}</Callout>}
    </Modal>
  );
}

type ItemProps = { m: Message; assistantName: string; isLastAssistant: boolean; editing: string | null; act: (kind: string, m: Message, arg?: unknown) => void; speaking: boolean; showReasoning: boolean; busy: boolean };

const MessageItem = memo(function MessageItem({ m, assistantName, isLastAssistant, editing, act, speaking, showReasoning, busy }: ItemProps) {
  const { think, body } = splitThink(m.content);
  const reasoning = m.reasoning || think;
  const isTask = !!m.meta?.task_id;
  const [copied, copy] = useCopy();
  const rating = m.meta?.rating ?? 0;
  const time = new Date(m.created).toLocaleString([], { dateStyle: "medium", timeStyle: "short" });
  const tokens = (m.meta?.input_tokens ?? 0) + (m.meta?.output_tokens ?? 0);
  return (
    <div className={"msg " + m.role}>
      {m.role === "assistant" && (
        <div className="who">
          <span className="avatar ai">{isTask ? <MousePointerClick size={13} /> : <Bot size={13} />}</span>
          {isTask ? "Agent" : assistantName}{m.meta?.model ? <span className="muted" style={{ fontWeight: 500 }}>· {m.meta.model}</span> : null}
          {m.meta?.cost ? <span className="badge">{money(m.meta.cost)}</span> : null}
          {m.meta?.stopped && <span className="badge warn">stopped</span>}
        </div>
      )}
      {m.role === "user" ? (
        editing !== null ? (
          <div style={{ width: "100%" }}>
            <textarea className="input" rows={4} value={editing} onChange={(e) => act("edit", m, e.target.value)} autoFocus
              onKeyDown={(e) => { if (e.key === "Escape") act("edit", m, null); if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) act("save", m); }} />
            <div className="row end" style={{ marginTop: 8 }}>
              <button className="btn sm" onClick={() => act("edit", m, null)}>Cancel</button>
              <button className="btn sm primary" onClick={() => act("save", m)} disabled={!editing.trim()}>Save and resend</button>
            </div>
          </div>
        ) : (
          <div className="bubble">
            {m.images?.map((im, i) => <img key={i} className="att" src={`data:${im.media_type};base64,${im.data}`} alt={im.name ?? "image"} />)}
            {m.meta?.docs?.length ? (
              <div className="bubble-docs">
                {m.meta.docs.map((d) => (
                  <span key={d.id} className="doc-chip ready" title={d.mode === "embed" ? "Searched for each question" : "Sent in full"}>
                    {d.mode === "embed" ? <Database size={13} /> : <FileText size={13} />}<span className="ellipsis">{d.name}</span>
                  </span>
                ))}
              </div>
            ) : null}
            {m.content}
          </div>
        )
      ) : (
        <div className={isTask ? "task-card" : undefined}>
          {reasoning && showReasoning && <Thinking text={reasoning} live={false} />}
          {m.meta?.tools?.map((t, i) => <ToolDrawer key={i} t={{ ...t, status: "done" }} />)}
          <div className="md"><MD text={body} /></div>
        </div>
      )}
      {editing === null && (
        <div className="msg-actions">
          <button className={"icon-btn" + (copied ? " copied" : "")} title="Copy" aria-label="Copy" onClick={() => copy("m", body)}>{copied ? <Check size={14} /> : <Copy size={14} />}</button>
          {m.role === "user" && !busy && <button className="icon-btn" title="Edit and resend" aria-label="Edit" onClick={() => act("edit", m, m.content)}><Pencil size={14} /></button>}
          {m.role === "assistant" && (
            <>
              <button className={"icon-btn" + (speaking ? " feedback-on" : "")} title={speaking ? "Stop reading" : "Read aloud"} aria-label="Read aloud" onClick={() => act("speak", m)}>{speaking ? <VolumeX size={14} /> : <Volume2 size={14} />}</button>
              {!isTask && <button className={"icon-btn" + (rating === 1 ? " feedback-on" : "")} title="Good reply" aria-label="Good reply" aria-pressed={rating === 1} onClick={() => act("rate", m, rating === 1 ? 0 : 1)}><ThumbsUp size={14} /></button>}
              {!isTask && <button className={"icon-btn" + (rating === -1 ? " feedback-on" : "")} title="Bad reply" aria-label="Bad reply" aria-pressed={rating === -1} onClick={() => act("rate", m, rating === -1 ? 0 : -1)}><ThumbsDown size={14} /></button>}
            </>
          )}
          {m.role === "assistant" && isLastAssistant && !busy && !isTask && <button className="icon-btn" title="Regenerate" aria-label="Regenerate" onClick={() => act("regen", m)}><RefreshCw size={14} /></button>}
          <button className="icon-btn" title="Branch into a new chat from here" aria-label="Branch" onClick={() => act("branch", m)}><GitBranch size={14} /></button>
          <span className="msg-meta">
            {time}
            {tokens ? ` · ${tokens.toLocaleString()} tokens` : ""}
            {m.meta?.tps ? <> · <Gauge size={11} /> {m.meta.tps.toFixed(1)} tok/s</> : null}
            {m.meta?.ttft_ms ? ` · first token ${(m.meta.ttft_ms / 1000).toFixed(1)}s` : ""}
          </span>
        </div>
      )}
    </div>
  );
});
