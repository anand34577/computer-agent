"""Minimal MCP server over stdio for testing the app's MCP client.

Add it in Extensions -> MCP servers -> Custom local program:
  command: python   arguments: -I  <full path to this file>
"""

import json
import sys

TOOLS = [
    {"name": "echo", "description": "Echoes the given text back.", "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]}},
    {"name": "add", "description": "Adds two numbers.", "inputSchema": {"type": "object", "properties": {"a": {"type": "number"}, "b": {"type": "number"}}, "required": ["a", "b"]}},
]


def reply(id_, result=None, error=None):
    msg = {"jsonrpc": "2.0", "id": id_}
    if error:
        msg["error"] = {"code": -32601, "message": error}
    else:
        msg["result"] = result
    sys.stdout.write(json.dumps(msg) + "\n")
    sys.stdout.flush()


for line in sys.stdin:
    try:
        req = json.loads(line)
    except ValueError:
        continue
    method, id_ = req.get("method"), req.get("id")
    if id_ is None:
        continue  # notification
    if method == "initialize":
        reply(id_, {"protocolVersion": "2025-06-18", "capabilities": {"tools": {}}, "serverInfo": {"name": "mock-mcp", "version": "1.0"}})
    elif method == "tools/list":
        reply(id_, {"tools": TOOLS})
    elif method == "tools/call":
        p = req.get("params", {})
        a = p.get("arguments", {})
        if p.get("name") == "echo":
            reply(id_, {"content": [{"type": "text", "text": str(a.get("text", ""))}]})
        elif p.get("name") == "add":
            reply(id_, {"content": [{"type": "text", "text": str(float(a.get("a", 0)) + float(a.get("b", 0)))}]})
        else:
            reply(id_, {"content": [{"type": "text", "text": "unknown tool"}], "isError": True})
    else:
        reply(id_, error=f"Method not found: {method}")
