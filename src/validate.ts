// Client-side checks that mirror the engine's rules (src-tauri/src/validate.rs).
// They give instant feedback; the engine re-checks everything before saving.

export type Errors = Record<string, string | undefined>;

export const required = (v: string, what = "This field") => (v.trim() ? undefined : `${what} is required`);

export const maxLen = (v: string, n: number) => (v.length > n ? `Keep it under ${n} characters` : undefined);

export function url(v: string): string | undefined {
  const s = v.trim();
  if (!s) return "Enter a base address";
  let u: URL;
  try {
    u = new URL(s);
  } catch {
    return "Not a valid address. Example: http://127.0.0.1:11434 or https://api.example.com/v1";
  }
  if (u.protocol !== "http:" && u.protocol !== "https:") return "Address must start with http:// or https://";
  if (!u.hostname) return "Address has no host name";
  return undefined;
}

/** Same rules as the engine: loopback and private-network hosts count as local. */
export function hostScope(host: string): "loopback" | "private" | "public" {
  const h = host.trim().replace(/^\[|\]$/g, "").toLowerCase();
  if (h === "localhost" || h.endsWith(".localhost") || h === "0.0.0.0" || h === "::1" || h === "::") return "loopback";
  const v4 = h.match(/^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/);
  if (v4) {
    const [a, b] = [Number(v4[1]), Number(v4[2])];
    if (a === 127 || a === 0) return "loopback";
    if (a === 10 || (a === 172 && b >= 16 && b < 32) || (a === 192 && b === 168) || (a === 169 && b === 254) || (a === 100 && b >= 64 && b < 128)) return "private";
    return "public";
  }
  if (h.includes(":")) return /^(fc|fd|fe8|fe9|fea|feb)/.test(h) ? "private" : "public";
  if (!h.includes(".") || [".local", ".lan", ".home", ".internal", ".home.arpa", ".intranet", ".corp", ".ts.net"].some((x) => h.endsWith(x))) return "private";
  return "public";
}

export function isLocalHost(h: string) {
  return hostScope(h) !== "public";
}

export function isLocalUrl(v: string) {
  try {
    return isLocalHost(new URL(v.trim()).hostname);
  } catch {
    return false;
  }
}

/** A warning (not an error) for plain http to a public host. */
export function insecureWarning(v: string): string | undefined {
  try {
    const u = new URL(v.trim());
    if (u.protocol === "http:" && hostScope(u.hostname) === "public") return "Plain http to an internet address: your key and data are not encrypted. Use https:// if the server supports it.";
  } catch {
    /* invalid addresses are reported by url() */
  }
  return undefined;
}

export function range(v: number, lo: number, hi: number): string | undefined {
  if (!Number.isFinite(v)) return "Enter a number";
  if (v < lo || v > hi) return `Must be between ${lo} and ${hi}`;
  return undefined;
}

export function integer(v: number) {
  return Number.isInteger(v) ? undefined : "Use a whole number";
}

const MODS = ["ctrl", "control", "alt", "shift", "super", "cmd", "command", "commandorcontrol", "cmdorctrl"];
export function shortcut(v: string, requiredField = false): string | undefined {
  const s = v.trim();
  if (!s) return requiredField ? "A shortcut is required" : undefined;
  const parts = s.toLowerCase().split("+").map((p) => p.trim());
  if (parts.some((p) => !p)) return "Use the form Ctrl+Shift+F12";
  if (!parts.some((p) => MODS.includes(p))) return "Include at least one modifier (Ctrl, Alt or Shift)";
  if (parts.every((p) => MODS.includes(p))) return "Add a key after the modifiers";
  const joined = parts.join("+");
  if (joined === "ctrl+shift+esc" || joined === "ctrl+shift+escape") return "Ctrl+Shift+Esc opens Task Manager on Windows";
  if (joined === "ctrl+alt+delete" || joined === "ctrl+alt+del") return "Reserved by the system";
  return undefined;
}

export function absolutePath(v: string): string | undefined {
  const s = v.trim();
  if (!s) return undefined;
  if (/^[a-zA-Z]:[\\/]/.test(s) || s.startsWith("\\\\") || s.startsWith("/")) return undefined;
  return `"${s}" is not a full path (for example C:\\Users\\you\\Documents)`;
}

export function hexColor(v: string) {
  return /^#[0-9a-fA-F]{6}$/.test(v) ? undefined : "Use a colour like #4f6bed";
}

export function apiKey(v: string, needed: boolean): string | undefined {
  const s = v.trim();
  if (!s) return needed ? "This provider needs an API key" : undefined;
  if (/\s/.test(s)) return "The key contains spaces; paste it again";
  if (s.length < 8) return "That looks too short for an API key";
  return undefined;
}

export const hasErrors = (e: Errors) => Object.values(e).some(Boolean);

/** The engine reports settings problems as JSON [{field, message}]; anything else is a plain message. */
export function parseEngineErrors(e: unknown): { fields: Errors; message?: string } {
  const text = typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
  try {
    const arr = JSON.parse(text) as { field: string; message: string }[];
    if (Array.isArray(arr)) return { fields: Object.fromEntries(arr.map((x) => [x.field, x.message])) };
  } catch {
    /* plain text */
  }
  // "Field: message" lines from provider/model checks.
  const fields: Errors = {};
  let matched = false;
  for (const line of text.split("\n")) {
    const m = line.match(/^([A-Za-z ]{2,24}): (.+)$/);
    if (m) {
      fields[m[1]] = m[2];
      matched = true;
    }
  }
  return matched ? { fields } : { fields: {}, message: text };
}
