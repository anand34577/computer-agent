// Live state of replies and agent runs, per conversation. It lives outside React so a reply
// keeps streaming into view when you switch chats or pages and come back.
import { useSyncExternalStore } from "react";
import { on, type AgentState, type FastChecks, type ToolEvent } from "./api";

/** permission: "auto" ran without asking, "asked" after your OK, "declined" by you, "off" was blocked. */
export type LiveTool = { name: string; args: unknown; result?: string; status: string; permission?: string; call_id?: string; ms?: number; started?: number; title?: string };
export type LiveReply = {
  message_id: string;
  text: string;
  reasoning: string;
  tools: LiveTool[];
  model: string;
  started: number;
  firstAt?: number;
  thinkEnd?: number;
  /** Streamed pieces; servers send about one token per piece. */
  pieces: number;
};
export type LiveStep = { id: string; idx: number; kind: string; title: string; detail: string; result: string; risk: string; screenshot?: string };
export type LiveAgent = {
  task_id: string;
  status: string;
  message: string;
  step: number;
  steps: LiveStep[];
  text: string;
  reasoning: string;
  model: string;
  started: number;
  plan: string[];
  planStep: number;
  thought: string;
  doing: string;
  /** Tool calls of this task, newest last; running ones can be cancelled. */
  tools: LiveTool[];
  fast?: FastChecks;
};
export type ConvLive = {
  reply?: LiveReply;
  agent?: LiveAgent;
  notices: string[];
  error?: { text: string; fallback: string | null };
  /** Bumped when a reply or task finishes, so the view reloads the saved messages. */
  done: number;
};

const EMPTY: ConvLive = { notices: [], done: 0 };
const state = new Map<string, ConvLive>();
const listeners = new Set<() => void>();
let frame = 0;
let version = 0;

function notify() {
  // Batch bursts of streamed pieces into one render per frame.
  if (frame) return;
  frame = requestAnimationFrame(() => {
    frame = 0;
    listeners.forEach((l) => l());
  });
}

function patch(id: string, fn: (c: ConvLive) => ConvLive) {
  const before = state.get(id)?.reply;
  state.set(id, fn(state.get(id) ?? EMPTY));
  if (!before !== !state.get(id)?.reply) version++;
  notify();
}

export const live = {
  get: (id: string | null) => (id ? state.get(id) ?? EMPTY : EMPTY),
  clearReply: (id: string) => patch(id, (c) => ({ ...c, reply: undefined })),
  clearError: (id: string) => patch(id, (c) => ({ ...c, error: undefined })),
  clearNotices: (id: string) => patch(id, (c) => ({ ...c, notices: [] })),
  notice: (id: string, text: string) => patch(id, (c) => ({ ...c, notices: [...c.notices, text] })),
  /** Shows the waiting state straight away, before the backend's first event. */
  pending: (id: string, model: string) => patch(id, (c) => (c.reply ? c : { ...c, error: undefined, reply: { message_id: "", text: "", reasoning: "", tools: [], model, started: Date.now(), pieces: 0 } })),
  busy: (id: string | null) => !!id && !!state.get(id)?.reply,
};

export function useLive(id: string | null): ConvLive {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => live.get(id),
  );
}

/** Changes when any chat starts or stops replying (for the sidebar's busy marks). */
export function useLiveBusy(): number {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => version,
  );
}

let started = false;
const taskConv = new Map<string, string>();

/** Subscribes once, for the whole app lifetime. */
export function initLive() {
  if (started) return;
  started = true;
  on<{ conversation_id: string; message_id: string; model: string }>("chat-start", (p) => {
    patch(p.conversation_id, (c) => ({ ...c, error: undefined, reply: { message_id: p.message_id, text: "", reasoning: "", tools: [], model: p.model, started: c.reply?.started ?? Date.now(), pieces: 0 } }));
  });
  on<{ conversation_id: string; kind: string; text: string }>("chat-delta", (p) => {
    patch(p.conversation_id, (c) => {
      const r = c.reply;
      if (!r) return c;
      const now = Date.now();
      const base = { ...r, pieces: r.pieces + 1, firstAt: r.firstAt ?? now };
      return { ...c, reply: p.kind === "reasoning" ? { ...base, reasoning: r.reasoning + p.text } : { ...base, text: r.text + p.text, thinkEnd: r.thinkEnd ?? (r.reasoning ? now : undefined) } };
    });
  });
  on<{ conversation_id: string; call_id?: string; name: string; args: unknown; status: string; result?: string; permission?: string; ms?: number }>("chat-tool", (p) => {
    patch(p.conversation_id, (c) => {
      const r = c.reply;
      if (!r) return c;
      const tools = [...r.tools];
      const i = tools.findIndex((t) => t.status === "running" && (p.call_id ? t.call_id === p.call_id : t.name === p.name));
      if (p.status === "done" && i >= 0) tools[i] = { ...tools[i], status: "done", result: p.result, permission: p.permission, ms: p.ms };
      else tools.push({ name: p.name, args: p.args, status: p.status, result: p.result, permission: p.permission, call_id: p.call_id, ms: p.ms, started: Date.now() });
      return { ...c, reply: { ...r, tools } };
    });
  });
  on<{ conversation_id: string }>("chat-done", (p) => patch(p.conversation_id, (c) => ({ ...c, done: c.done + 1 })));
  on<{ conversation_id: string; error: string; fallback: string | null }>("chat-error", (p) =>
    patch(p.conversation_id, (c) => ({ ...c, done: c.done + 1, error: { text: p.error, fallback: p.fallback } })),
  );
  on<{ conversation_id: string; text: string }>("chat-notice", (p) => patch(p.conversation_id, (c) => ({ ...c, notices: [...c.notices, p.text] })));

  // Agent runs started from a chat show their progress inside it.
  const agentPatch = (task: string, conv: string | null | undefined, fn: (a: LiveAgent) => LiveAgent) => {
    const id = conv ?? taskConv.get(task);
    if (!id) return;
    taskConv.set(task, id);
    patch(id, (c) => {
      const a: LiveAgent = c.agent && c.agent.task_id === task ? c.agent : { task_id: task, status: "starting", message: "", step: 0, steps: [], text: "", reasoning: "", model: "", started: Date.now(), plan: [], planStep: 0, thought: "", doing: "", tools: [] };
      return { ...c, agent: fn(a) };
    });
  };
  on<AgentState>("agent-state", (s) =>
    agentPatch(s.task_id, s.conversation_id, (a) => ({ ...a, status: s.status, message: s.message, step: s.step, model: a.model || s.doer, plan: s.plan ?? a.plan, planStep: s.plan_step ?? a.planStep, thought: s.thought ?? a.thought, doing: s.doing ?? a.doing, fast: s.fast ?? a.fast })),
  );
  on<ToolEvent>("agent-tool", (t) =>
    agentPatch(t.task_id ?? "", t.conversation_id, (a) => {
      const tools = [...a.tools];
      const i = tools.findIndex((x) => x.call_id === t.call_id);
      const row: LiveTool = { name: t.name, title: t.title, args: t.args, status: t.status, result: t.result, call_id: t.call_id, ms: t.ms, started: i >= 0 ? tools[i].started : Date.now() };
      if (i >= 0) tools[i] = row;
      else tools.push(row);
      return { ...a, tools: tools.slice(-40) };
    }),
  );
  on<LiveStep & { task_id: string; conversation_id?: string | null }>("agent-step", (s) =>
    agentPatch(s.task_id, s.conversation_id, (a) => ({ ...a, steps: [...a.steps, { id: s.id, idx: s.idx, kind: s.kind, title: s.title, detail: s.detail, result: s.result, risk: s.risk, screenshot: s.screenshot }].slice(-60) })),
  );
  on<{ task_id: string; conversation_id?: string | null; kind: string; text: string; model?: string }>("agent-delta", (d) =>
    agentPatch(d.task_id, d.conversation_id, (a) =>
      d.kind === "start" ? { ...a, text: "", reasoning: "", model: d.model ?? a.model } : d.kind === "reasoning" ? { ...a, reasoning: a.reasoning + d.text } : { ...a, text: a.text + d.text },
    ),
  );
  on<{ task_id: string; conversation_id: string | null }>("agent-finished", (f) => {
    const id = f.conversation_id ?? taskConv.get(f.task_id);
    taskConv.delete(f.task_id);
    if (id) patch(id, (c) => ({ ...c, agent: undefined, done: c.done + 1 }));
  });
}
