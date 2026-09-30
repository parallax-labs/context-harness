"""Local deterministic stdio MCP fixture; no network or third-party modules."""
import json
import os
import sys
import time

mode = sys.argv[1] if len(sys.argv) > 1 else "ok"
with open("mcp-pid", "w", encoding="utf-8") as file:
    file.write(str(os.getpid()))

def send(value):
    print(json.dumps(value), flush=True)

def tool(name):
    return {"name": name, "description": "Echo test input", "inputSchema": {
        "type": "object", "properties": {"text": {"type": "string"}},
        "required": ["text"], "additionalProperties": False},
        "annotations": {"readOnlyHint": True}}

for line in sys.stdin:
    message = json.loads(line)
    with open("mcp-messages", "a", encoding="utf-8") as file:
        file.write(json.dumps(message) + "\n")
    if "result" in message or "error" in message:
        continue
    if "id" not in message:
        continue
    method = message.get("method")
    if method == "initialize":
        if mode == "startup-timeout":
            time.sleep(60)
        result = {"protocolVersion": message["params"]["protocolVersion"],
                  "capabilities": {"tools": {}}, "serverInfo": {"name": "fixture", "version": "1"}}
    elif method == "tools/list":
        if mode == "oversized":
            sys.stdout.write("x" * (2 * 1024 * 1024) + "\n")
            sys.stdout.flush()
            continue
        if mode == "malformed":
            print("not json", flush=True)
            continue
        if mode == "duplicate":
            result = {"tools": [tool("echo"), tool("echo")]}
        elif mode == "many":
            result = {"tools": [tool("t" + str(i)) for i in range(129)]}
        elif mode == "cycle":
            result = {"tools": [], "nextCursor": "same"}
        elif mode == "paged" and "cursor" not in message.get("params", {}):
            result = {"tools": [tool("other")], "nextCursor": "page2"}
        else:
            result = {"tools": [tool("echo")]}
    elif method == "tools/call":
        if mode == "call-timeout":
            time.sleep(60)
        if mode == "disconnect":
            sys.exit(0)
        if mode == "call-error":
            result = {"content": [{"type": "text", "text": "fixture-private-error"}], "isError": True}
        else:
            result = {"content": [{"type": "text", "text": "external: " + message["params"]["arguments"]["text"]}]}
    else:
        send({"jsonrpc": "2.0", "id": message["id"], "error": {"code": -32601, "message": "unsupported"}})
        continue
    send({"jsonrpc": "2.0", "id": message["id"], "result": result})
    if method == "tools/list" and mode == "callback":
        send({"jsonrpc": "2.0", "id": "server-request", "method": "roots/list"})
