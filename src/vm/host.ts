// Entry point of the hidden "vm" window: loads the verified images from the engine,
// boots Linux in a worker and relays commands between the engine and the worker.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import wasmUrl from "v86/build/v86.wasm?url";

export async function startVm(memory: number) {
  const worker = new Worker(new URL("./worker.ts", import.meta.url), { type: "module" });
  const fail = (error: string) => invoke("vm_failed", { error }).catch(() => {});
  worker.onerror = (e) => fail(e.message || "The VM worker crashed");
  worker.onmessage = (e) => {
    const m = e.data;
    if (m.type === "ready") invoke("vm_ready", { bootMs: m.boot_ms });
    else if (m.type === "failed") fail(m.error);
    else if (m.type === "result") invoke("vm_result", { result: m });
    else if (m.type === "restart") invoke("vm_stop");
  };
  await listen<{ id: number; cmd: string; timeout_ms: number; files: [string, string][] }>("vm-exec", (e) => {
    worker.postMessage({ type: "exec", ...e.payload });
  });
  try {
    const [wasm, bios, vga, linux] = await Promise.all([
      fetch(wasmUrl).then((r) => r.arrayBuffer()),
      invoke<ArrayBuffer>("vm_image", { name: "seabios.bin" }),
      invoke<ArrayBuffer>("vm_image", { name: "vgabios.bin" }),
      invoke<ArrayBuffer>("vm_image", { name: "linux.bin" }),
    ]);
    worker.postMessage({ type: "boot", wasm, bios, vga, linux, memory }, [wasm, bios, vga, linux]);
  } catch (e) {
    fail(String(e));
  }
}
