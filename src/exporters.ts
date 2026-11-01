// Chat exports: Markdown, JSON, standalone HTML and PDF (through the system print dialog,
// which offers "Save as PDF" / "Microsoft Print to PDF").
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { api, type Conversation, type Message } from "./api";

const who = (m: Message, assistant: string) => (m.role === "user" ? "You" : m.meta?.task_id ? "Agent" : assistant);
const stamp = (ms: number) => new Date(ms).toLocaleString();
export const safeName = (t: string) => t.replace(/[\\/:*?"<>|]+/g, " ").replace(/\s+/g, " ").trim().slice(0, 80) || "chat";

export function toMarkdown(c: Conversation, assistant: string): string {
  let md = `# ${c.title}\n\n_Exported ${new Date().toLocaleString()}_\n\n`;
  for (const m of c.messages) {
    md += `### ${who(m, assistant)}${m.meta?.model ? ` · ${m.meta.model}` : ""} — ${stamp(m.created)}\n\n`;
    if (m.reasoning) md += `<details><summary>Reasoning</summary>\n\n${m.reasoning}\n\n</details>\n\n`;
    for (const t of m.meta?.tools ?? []) md += `> Tool **${t.name}** \`${JSON.stringify(t.args)}\`\n\n`;
    if (m.images?.length) md += `_${m.images.length} image(s) attached_\n\n`;
    md += `${m.content}\n\n`;
  }
  return md;
}

export function toJSON(c: Conversation): string {
  return JSON.stringify({ ...c, exported: new Date().toISOString(), messages: c.messages.map((m) => ({ ...m, images: m.images?.map((i) => ({ media_type: i.media_type, name: i.name })) })) }, null, 2);
}

const CSS = `
body{font:15px/1.6 "Segoe UI",system-ui,-apple-system,sans-serif;color:#16181d;max-width:780px;margin:32px auto;padding:0 24px}
h1{font-size:24px;margin:0 0 4px}.meta{color:#80858f;font-size:12px;margin-bottom:28px}
.m{margin:0 0 22px;page-break-inside:avoid}.who{font-size:12px;font-weight:600;color:#80858f;margin-bottom:6px}
.u .b{background:#f1f2f4;border-radius:12px;padding:10px 14px;white-space:pre-wrap}
pre{background:#f6f7f9;border:1px solid #e4e5e9;border-radius:8px;padding:10px 12px;overflow:auto;white-space:pre-wrap}
code{font-family:Consolas,ui-monospace,monospace;font-size:.9em}table{border-collapse:collapse}td,th{border:1px solid #e4e5e9;padding:4px 8px}
details{border:1px solid #e4e5e9;border-radius:8px;padding:6px 10px;margin-bottom:8px;color:#4b505b;font-size:13px;white-space:pre-wrap}
img{max-width:320px;border-radius:8px;display:block;margin-bottom:8px}
@page{margin:18mm}`;

export function toHTML(c: Conversation, assistant: string): string {
  const esc = (s: string) => s.replace(/[&<>"]/g, (ch) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[ch]!);
  const body = c.messages
    .map((m) => {
      const head = `<div class="who">${esc(who(m, assistant))}${m.meta?.model ? ` · ${esc(m.meta.model)}` : ""} · ${esc(stamp(m.created))}</div>`;
      const imgs = (m.images ?? []).map((i) => `<img src="data:${i.media_type};base64,${i.data}" alt="">`).join("");
      if (m.role === "user") return `<div class="m u">${head}<div class="b">${imgs}${esc(m.content)}</div></div>`;
      const reasoning = m.reasoning ? `<details><summary>Reasoning</summary>${esc(m.reasoning)}</details>` : "";
      const md = renderToStaticMarkup(createElement(ReactMarkdown, { remarkPlugins: [remarkGfm] }, m.content));
      return `<div class="m">${head}${reasoning}${md}</div>`;
    })
    .join("\n");
  return `<!doctype html><html><head><meta charset="utf-8"><title>${esc(c.title)}</title><style>${CSS}</style></head><body><h1>${esc(c.title)}</h1><div class="meta">${c.messages.length} messages · exported ${esc(new Date().toLocaleString())}</div>${body}</body></html>`;
}

/** Asks where to save, then writes the file. Returns the path, or null if cancelled. */
export async function saveExport(defaultName: string, ext: "md" | "json" | "html" | "csv", content: string): Promise<string | null> {
  const names = { md: "Markdown", json: "JSON", html: "Web page", csv: "CSV" };
  const path = await api.pickPath("save", { defaultName: `${safeName(defaultName)}.${ext}`, filterName: names[ext], extensions: [ext] });
  if (!path) return null;
  await api.saveTextFile(path, content);
  return path;
}

/** Prints a clean copy of the chat; the print dialog's "Save as PDF" produces the PDF. */
export function printPdf(c: Conversation, assistant: string) {
  const f = document.createElement("iframe");
  f.style.cssText = "position:fixed;right:0;bottom:0;width:0;height:0;border:0";
  f.srcdoc = toHTML(c, assistant);
  f.onload = () => {
    f.contentWindow?.focus();
    f.contentWindow?.print();
    setTimeout(() => f.remove(), 60_000);
  };
  document.body.appendChild(f);
}
