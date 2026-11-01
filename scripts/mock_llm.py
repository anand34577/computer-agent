"""Scripted OpenAI-compatible server for end-to-end testing without a real model.

Run:  python scripts/mock_llm.py   (listens on http://127.0.0.1:8765/v1)

It answers the capability self-test, streams chat replies, returns a plan, and drives a
short agent task that only uses file tools inside ~/ComputerAgentTest (no mouse or keyboard).
"""

import json
import os
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

TEST_DIR = os.path.join(os.path.expanduser("~"), "ComputerAgentTest")


def text_of(msg):
    c = msg.get("content")
    if isinstance(c, str):
        return c
    if isinstance(c, list):
        return " ".join(p.get("text", "") for p in c if p.get("type") == "text")
    return ""


def has_image(msgs):
    return any(isinstance(m.get("content"), list) and any(p.get("type") == "image_url" for p in m["content"]) for m in msgs)


def decide(body):
    msgs = body.get("messages", [])
    tools = [t["function"]["name"] for t in body.get("tools", [])]
    system = text_of(msgs[0]) if msgs and msgs[0].get("role") == "system" else ""
    last = text_of(msgs[-1]) if msgs else ""

    if "record_number" in tools:
        return {"tool": ("record_number", {"n": 42})}
    if has_image(msgs) and "colour" in last:
        return {"text": "red green blue"}
    if "You plan tasks" in system:
        return {"text": json.dumps({"steps": ["Create the test folder", "Write a note into it", "Finish"]})}
    if "done" in tools:
        done_calls = sum(1 for m in msgs if m.get("role") == "tool")
        if done_calls == 0:
            return {"text": "I will create the test folder first.", "tool": ("create_folder", {"path": TEST_DIR})}
        if done_calls == 1:
            return {"text": "Now I write the note.", "tool": ("write_file", {"path": os.path.join(TEST_DIR, "note.txt"), "content": "Hello from the agent test."})}
        return {"text": "All set.", "tool": ("done", {"summary": "Created ComputerAgentTest\\note.txt", "success": True})}
    if "list_files" in tools and "downloads" in last.lower():
        if not any(m.get("role") == "tool" for m in msgs):
            return {"tool": ("list_files", {"path": "downloads"})}
        return {"text": "Your Downloads folder listing came back; the tool call worked."}
    if "think" in last.lower():
        # DeepSeek-R1 style: reasoning inline in <think> tags, then the answer with a code block.
        return {"text": "<think>The user wants a quick check of reasoning. I will add 2 and 2, then show a code sample.\nStep 1: 2 + 2 = 4.\nStep 2: format the answer.</think>The answer is **4**.\n\n```python\nprint(2 + 2)\n```", "slow": True}
    if "reason" in last.lower():
        # OpenAI-compatible reasoning_content deltas (DeepSeek API, vLLM, Ollama).
        return {"reasoning": "Considering the request. Comparing two options. Picking the simpler one.", "text": "I picked the simpler option."}
    return {"text": "Hello from the **mock model**. Streaming works, and this reply arrived in several chunks."}


class H(BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def _json(self, code, obj):
        data = json.dumps(obj).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        if self.path.endswith("/models"):
            return self._json(200, {"data": [{"id": "mock-agent-1"}, {"id": "mock-text-only"}]})
        self._json(404, {"error": "not found"})

    def do_POST(self):
        n = int(self.headers.get("Content-Length", 0))
        body = json.loads(self.rfile.read(n) or b"{}")
        d = decide(body)
        text = d.get("text", "")
        tool = d.get("tool")
        usage = {"prompt_tokens": 1200, "completion_tokens": 40}
        if not body.get("stream"):
            msg = {"role": "assistant", "content": text}
            if tool:
                msg["tool_calls"] = [{"id": "call_1", "type": "function", "function": {"name": tool[0], "arguments": json.dumps(tool[1])}}]
            return self._json(200, {"choices": [{"message": msg, "finish_reason": "stop"}], "usage": usage})
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()

        def send(obj):
            self.wfile.write(f"data: {json.dumps(obj)}\n\n".encode())
            self.wfile.flush()

        for i in range(0, len(d.get("reasoning", "")), 8):
            send({"choices": [{"delta": {"reasoning_content": d["reasoning"][i : i + 8]}}]})
            time.sleep(0.12)
        for i in range(0, len(text), 12):
            send({"choices": [{"delta": {"content": text[i : i + 12]}}]})
            time.sleep(0.25 if d.get("slow") and i < text.find("</think>") else 0.05)
        if tool:
            send({"choices": [{"delta": {"tool_calls": [{"index": 0, "id": "call_1", "function": {"name": tool[0], "arguments": json.dumps(tool[1])}}]}}]})
        send({"choices": [], "usage": usage})
        self.wfile.write(b"data: [DONE]\n\n")


if __name__ == "__main__":
    print("mock LLM on http://127.0.0.1:8765/v1")
    ThreadingHTTPServer(("127.0.0.1", 8765), H).serve_forever()
