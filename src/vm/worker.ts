/// <reference lib="webworker" />
// Runs the v86 emulator off the UI thread. Commands are written to a file on the shared 9p
// filesystem and started over the serial console; results come back through files, so
// quoting and terminal echo never corrupt output.
import { V86 } from "v86";

type Files = [string, string][];
type Exec = { id: number; cmd: string; timeout_ms: number; files: Files };

let emu: V86 | null = null;
let serial = "";
const waiters: { re: RegExp; resolve: (m: RegExpMatchArray) => void }[] = [];
const enc = new TextEncoder();
const dec = new TextDecoder();
const post = (m: unknown) => (self as unknown as Worker).postMessage(m);

function onByte(b: number) {
  serial += String.fromCharCode(b);
  if (serial.length > 20000) serial = serial.slice(-10000);
  check();
}

function check() {
  for (let i = 0; i < waiters.length; i++) {
    const m = serial.match(waiters[i].re);
    if (m) {
      const w = waiters.splice(i, 1)[0];
      serial = serial.slice((m.index ?? 0) + m[0].length);
      w.resolve(m);
      i--;
    }
  }
}

function waitFor(re: RegExp, ms: number): Promise<RegExpMatchArray | null> {
  return new Promise((resolve) => {
    const w = { re, resolve: (m: RegExpMatchArray) => { clearTimeout(t); resolve(m); } };
    const t = setTimeout(() => { const i = waiters.indexOf(w); if (i >= 0) waiters.splice(i, 1); resolve(null); }, ms);
    waiters.push(w);
    check(); // the answer may already be buffered
  });
}

const send = (s: string) => emu!.serial0_send(s);
// Markers are split in the typed command so an echoed command line never matches them.
const mark = (name: string) => `echo "__CA_${name.slice(0, 2)}""${name.slice(2)}__"`;

function b64(bytes: Uint8Array): string {
  let s = "";
  for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(s);
}
function unb64(s: string): Uint8Array {
  const bin = atob(s);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}
async function readText(path: string, limit = 1_000_000): Promise<string> {
  try {
    const b = await emu!.read_file(path);
    const t = dec.decode(b.length > limit ? b.subarray(0, limit) : b);
    return b.length > limit ? `${t}\n…(output truncated at ${limit} bytes)` : t;
  } catch {
    return "";
  }
}

async function boot(d: { wasm: ArrayBuffer; bios: ArrayBuffer; vga: ArrayBuffer; linux: ArrayBuffer; memory: number }) {
  const t0 = performance.now();
  emu = new V86({
    wasm_fn: async (imports: WebAssembly.Imports) => (await WebAssembly.instantiate(d.wasm, imports)).instance.exports,
    memory_size: d.memory * 1024 * 1024,
    vga_memory_size: 2 * 1024 * 1024,
    bios: { buffer: d.bios },
    vga_bios: { buffer: d.vga },
    bzimage: { buffer: d.linux },
    cmdline: "tsc=reliable mitigations=off random.trust_cpu=on",
    filesystem: {},
    autostart: true,
    disable_keyboard: true,
    disable_mouse: true,
    disable_speaker: true,
  } as never);
  emu.add_listener("serial0-output-byte", onByte);
  post({ type: "log", text: "booting" });
  // Some images print the prompt before the console is attached; a nudge brings it back.
  let prompt = null;
  for (let i = 0; i < 8 && !prompt; i++) {
    prompt = await waitFor(/[#%$] $/, 15_000);
    if (!prompt) send("\n");
  }
  if (!prompt) throw new Error("Linux did not reach a shell prompt");
  send(`stty -echo 2>/dev/null; grep -q ' /mnt ' /proc/mounts || mount -t 9p host9p /mnt; mkdir -p /mnt/.ca /mnt/work; cd /mnt/work; export HOME=/root PS1='$ '; ${mark("READY")}\n`);
  if (!(await waitFor(/__CA_READY__/, 20_000))) throw new Error("The VM shell did not answer");
  await emu.create_file("/.ca/probe", enc.encode("ok"));
  send(`test -f /mnt/.ca/probe && ${mark("9POK")} || ${mark("9PNO")}\n`);
  const m = await waitFor(/__CA_9P(OK|NO)__/, 10_000);
  if (!m || m[1] !== "OK") throw new Error("The shared folder could not be mounted in the VM");
  post({ type: "ready", boot_ms: Math.round(performance.now() - t0) });
}

async function exec(x: Exec) {
  const dir = `/mnt/.ca`;
  const sent = new Map<string, Uint8Array>();
  // 1. Reset the working folder to mirror the host's shared folder.
  const dirs = new Set<string>();
  for (const [rel] of x.files) {
    const parts = rel.split("/");
    for (let i = 1; i < parts.length; i++) dirs.add(parts.slice(0, i).join("/"));
  }
  await emu!.create_file(`/.ca/${x.id}.dirs`, enc.encode([...dirs].join("\n") + "\n"));
  send(`cd /; rm -rf /mnt/work; mkdir -p /mnt/work; while IFS= read -r d; do [ -n "$d" ] && mkdir -p "/mnt/work/$d"; done < ${dir}/${x.id}.dirs; cd /mnt/work; ${mark(`MK_${x.id}`)}\n`);
  if (!(await waitFor(new RegExp(`__CA_MK_${x.id}__`), 15_000))) throw new Error("The VM did not answer");
  for (const [rel, data] of x.files) {
    const bytes = unb64(data);
    sent.set(rel, bytes);
    await emu!.create_file(`/work/${rel}`, bytes);
  }
  // 2. Run the command from a file.
  await emu!.create_file(`/.ca/${x.id}.sh`, enc.encode(x.cmd + "\n"));
  // The serial console has no job control, so Ctrl+C cannot interrupt; the guest's own
  // `timeout` enforces the limit and the VM keeps running.
  const secs = Math.max(1, Math.ceil(x.timeout_ms / 1000));
  const io = `> ${dir}/${x.id}.out 2> ${dir}/${x.id}.err < /dev/null`;
  const started = performance.now();
  // Plain-shell watchdog (BusyBox `timeout` flags differ between versions): kill the command
  // and its children when the limit passes, then report its exit status.
  const kill = `for c in $(pgrep -P $p 2>/dev/null); do kill -9 $c 2>/dev/null; done; kill -9 $p 2>/dev/null`;
  send(`cd /mnt/work; sh ${dir}/${x.id}.sh ${io} & p=$!; (sleep ${secs}; ${kill}) >/dev/null 2>&1 & w=$!; wait $p; rc=$?; kill $w 2>/dev/null; echo "__CA_RC_${x.id}_$rc""__"\n`);
  const rc = await waitFor(new RegExp(`__CA_RC_${x.id}_(\\d+)__`), x.timeout_ms + 8000);
  if (!rc) {
    post({ type: "result", id: x.id, code: -1, stdout: await readText(`/.ca/${x.id}.out`), stderr: "The command could not be stopped at its time limit; the VM will restart.", timed_out: true, files: [] });
    post({ type: "restart" });
    return;
  }
  const timedOut = Number(rc[1]) === 137 && performance.now() - started >= x.timeout_ms - 1500;
  // 3. Collect output and changed files.
  send(`cd /mnt/work; find . -type f > ${dir}/${x.id}.lst 2>/dev/null; ${mark(`LS_${x.id}`)}\n`);
  await waitFor(new RegExp(`__CA_LS_${x.id}__`), 15_000);
  const listed = (await readText(`/.ca/${x.id}.lst`)).split("\n").map((l) => l.replace(/^\.\//, "")).filter(Boolean);
  const files: Files = [];
  let total = 0;
  for (const rel of listed) {
    try {
      const b = await emu!.read_file(`/work/${rel}`);
      total += b.length;
      if (total > 32 * 1024 * 1024) break;
      const before = sent.get(rel);
      const same = before && before.length === b.length && before.every((v, i) => v === b[i]);
      if (!same) files.push([rel, b64(b)]);
    } catch { /* unreadable special file */ }
  }
  const deleted = [...sent.keys()].filter((k) => !listed.includes(k));
  const stdout = await readText(`/.ca/${x.id}.out`);
  let stderr = await readText(`/.ca/${x.id}.err`, 200_000);
  if (timedOut) stderr += `${stderr ? "\n" : ""}[Stopped at the ${secs}-second time limit]`;
  send(`rm -f ${dir}/${x.id}.*\n`);
  post({ type: "result", id: x.id, code: timedOut ? -1 : Number(rc[1]), stdout, stderr, timed_out: timedOut, files, deleted });
}

let queue = Promise.resolve();
self.onmessage = (e: MessageEvent) => {
  const m = e.data;
  if (m.type === "boot") {
    boot(m).catch((err) => post({ type: "failed", error: String(err?.message ?? err) }));
  } else if (m.type === "exec") {
    queue = queue.then(() => exec(m).catch((err) => post({ type: "result", id: m.id, error: String(err?.message ?? err) })));
  }
};
