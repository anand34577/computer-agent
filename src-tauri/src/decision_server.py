"""Local server for a structured decision model (Intern-Decision), used by Computer Agent.

The app writes this file into its data folder and starts it with its own Python environment.
It answers multiple-choice questions about a screenshot and some text in one forward pass.

    POST /predict   {"state": ..., "questions": {...}, "images_b64": ["<jpeg base64>", ...]}
    GET  /health    model, device and dtype once loaded

It listens on 127.0.0.1 unless --host says otherwise. With DECISION_TOKEN set, every request
must carry "Authorization: Bearer <token>".
"""
from __future__ import annotations

import argparse
import base64
import importlib.util
import json
import os
import sys
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

MAX_BODY = 40 * 1024 * 1024
MAX_IMAGES = 8


def log(msg: str) -> None:
    print(msg, flush=True)


def load_engine(model_dir: Path, device: str, dtype: str):
    # The model ships its own inference code; load it from the pinned download.
    spec = importlib.util.spec_from_file_location("intern_decision_inference", model_dir / "inference.py")
    if spec is None or spec.loader is None:
        raise SystemExit(f"inference.py not found in {model_dir}")
    mod = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = mod  # dataclasses look the module up while it loads
    spec.loader.exec_module(mod)
    import torch

    if device == "auto":
        device = "cuda" if torch.cuda.is_available() else "cpu"
    if dtype == "auto":
        if device == "cuda":
            dtype = "bfloat16" if torch.cuda.is_bf16_supported() else "float16"
        else:
            dtype = "float32"
    if device == "cpu":
        torch.set_num_threads(max(1, (os.cpu_count() or 2) - 1))
    log(f"Loading {model_dir.name} on {device} ({dtype})…")
    engine = mod.DecisionEngine(str(model_dir), device=device, dtype=dtype)
    return engine, device, dtype, getattr(mod, "MODEL_NAME", model_dir.name)


class State:
    engine = None
    info: dict = {}
    lock = threading.Lock()
    token = os.environ.get("DECISION_TOKEN") or ""


class Handler(BaseHTTPRequestHandler):
    server_version = "DecisionServer/1"

    def log_message(self, fmt, *args):  # keep the console for our own lines
        pass

    def _send(self, code: int, body: dict) -> None:
        data = json.dumps(body).encode("utf-8")
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def _authorised(self) -> bool:
        if not State.token:
            return True
        return self.headers.get("Authorization", "") == f"Bearer {State.token}"

    def do_GET(self):
        if not self._authorised():
            return self._send(401, {"error": "Missing or wrong token"})
        if self.path.rstrip("/") == "/health":
            return self._send(200, {"ok": State.engine is not None, **State.info})
        self._send(404, {"error": "Not found"})

    def do_POST(self):
        if not self._authorised():
            return self._send(401, {"error": "Missing or wrong token"})
        if self.path.rstrip("/") != "/predict":
            return self._send(404, {"error": "Not found"})
        length = int(self.headers.get("Content-Length") or 0)
        if length <= 0 or length > MAX_BODY:
            return self._send(413, {"error": "Request body is empty or too large"})
        try:
            req = json.loads(self.rfile.read(length))
        except ValueError:
            return self._send(400, {"error": "Body is not valid JSON"})
        if not isinstance(req, dict):
            return self._send(400, {"error": "Body must be a JSON object"})
        images = req.pop("images_b64", None) or []
        if not isinstance(images, list) or len(images) > MAX_IMAGES:
            return self._send(400, {"error": f"images_b64 must be a list of up to {MAX_IMAGES} images"})
        req.pop("images", None)  # only uploaded images, never paths on this machine
        paths: list[str] = []
        try:
            for b64 in images:
                fd, path = tempfile.mkstemp(suffix=".jpg", prefix="decision-")
                with os.fdopen(fd, "wb") as f:
                    f.write(base64.b64decode(b64, validate=True))
                paths.append(path)
            if paths:
                req["images"] = paths
            start = time.perf_counter()
            with State.lock:
                result = State.engine.predict(req)
            result.setdefault("timing", {})["total_ms"] = round((time.perf_counter() - start) * 1000, 2)
            self._send(200, result)
        except (ValueError, ArithmeticError) as e:
            self._send(400, {"error": str(e)})
        except Exception as e:  # noqa: BLE001 - report, keep serving
            log(f"predict failed: {e!r}")
            self._send(500, {"error": f"{type(e).__name__}: {e}"})
        finally:
            for p in paths:
                try:
                    os.remove(p)
                except OSError:
                    pass


class Server(ThreadingHTTPServer):
    # Never share the port: on Windows, address reuse lets two servers listen on one port.
    allow_reuse_address = False
    daemon_threads = True

    def server_bind(self):
        import socket
        if hasattr(socket, "SO_EXCLUSIVEADDRUSE"):
            self.socket.setsockopt(socket.SOL_SOCKET, socket.SO_EXCLUSIVEADDRUSE, 1)
        super().server_bind()


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", required=True, help="Folder with the downloaded model")
    ap.add_argument("--host", default="127.0.0.1")
    ap.add_argument("--port", type=int, default=47615)
    ap.add_argument("--device", default="auto", choices=["auto", "cuda", "cpu"])
    ap.add_argument("--dtype", default="auto", choices=["auto", "bfloat16", "float16", "float32"])
    args = ap.parse_args()

    model_dir = Path(args.model).resolve()
    engine, device, dtype, name = load_engine(model_dir, args.device, args.dtype)
    State.engine = engine
    State.info = {"model": name, "device": device, "dtype": dtype}
    # Warm up so the first real decision is fast.
    try:
        engine.predict({"state": "warm-up", "questions": {"ok": {"type": "noul", "instructions": "Is this a warm-up?"}}})
    except Exception as e:  # noqa: BLE001
        log(f"warm-up failed: {e!r}")
    srv = Server((args.host, args.port), Handler)
    log(f"READY http://{args.host}:{args.port} {name} {device} {dtype}")
    try:
        srv.serve_forever()
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    sys.exit(main())
