#!/usr/bin/env python3
import json
import sys
import time

mode = sys.argv[1] if len(sys.argv) > 1 else "normal"


def read_message():
    length = None
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        if line in (b"\r\n", b"\n"):
            break
        key, value = line.decode("ascii").split(":", 1)
        if key.lower() == "content-length":
            length = int(value.strip())
    if length is None:
        return None
    return json.loads(sys.stdin.buffer.read(length))


def send(payload):
    body = json.dumps(payload, separators=(",", ":")).encode()
    sys.stdout.buffer.write(f"Content-Length: {len(body)}\r\n\r\n".encode() + body)
    sys.stdout.buffer.flush()


while True:
    message = read_message()
    if message is None:
        break
    method = message.get("method")
    request_id = message.get("id")
    if mode == "crash" and method == "initialize":
        sys.exit(23)
    if mode == "timeout" and method == "initialize":
        time.sleep(5)
    if request_id is None:
        if method == "exit":
            break
        continue
    if method == "initialize":
        capabilities = {
            "positionEncoding": "utf-16",
            "documentSymbolProvider": True,
            "referencesProvider": mode != "unsupported",
            "definitionProvider": mode != "unsupported",
            "callHierarchyProvider": mode != "unsupported",
        }
        send({"jsonrpc": "2.0", "id": request_id, "result": {
            "capabilities": capabilities,
            "serverInfo": {"name": "lgr-mock", "version": "1.0.0"},
        }})
    elif method == "textDocument/documentSymbol":
        send({"jsonrpc": "2.0", "id": request_id, "result": [{
            "name": "changed",
            "kind": 12,
            "range": {"start": {"line": 0, "character": 0}, "end": {"line": 2, "character": 1}},
            "selectionRange": {"start": {"line": 0, "character": 16}, "end": {"line": 0, "character": 23}},
        }]})
    elif method == "textDocument/references":
        uri = message["params"]["textDocument"]["uri"]
        send({"jsonrpc": "2.0", "id": request_id, "result": [{
            "uri": uri,
            "range": {"start": {"line": 1, "character": 2}, "end": {"line": 1, "character": 9}},
        }]})
    elif method == "textDocument/definition":
        uri = message["params"]["textDocument"]["uri"]
        send({"jsonrpc": "2.0", "id": request_id, "result": {
            "uri": uri,
            "range": {"start": {"line": 0, "character": 16}, "end": {"line": 0, "character": 23}},
        }})
    elif method == "textDocument/prepareCallHierarchy":
        uri = message["params"]["textDocument"]["uri"]
        send({"jsonrpc": "2.0", "id": request_id, "result": [{
            "name": "changed", "kind": 12, "uri": uri,
            "range": {"start": {"line": 0, "character": 0}, "end": {"line": 2, "character": 1}},
            "selectionRange": {"start": {"line": 0, "character": 16}, "end": {"line": 0, "character": 23}},
        }]})
    elif method in ("callHierarchy/incomingCalls", "callHierarchy/outgoingCalls"):
        send({"jsonrpc": "2.0", "id": request_id, "result": []})
    elif method == "shutdown":
        send({"jsonrpc": "2.0", "id": request_id, "result": None})
    else:
        send({"jsonrpc": "2.0", "id": request_id, "error": {"code": -32601, "message": "unsupported"}})
