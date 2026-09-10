#!/usr/bin/env python3
import json
import pathlib
import sys

state = pathlib.Path(sys.argv[1])
mode = sys.argv[2]
base = sys.argv[3]
head = sys.argv[4]
args = sys.argv[5:]

if "user" in args and "--jq" in args:
    print("intruder" if mode == "wrong_identity" else "reviewer")
    raise SystemExit(0)

endpoint = next((arg for arg in args if arg.startswith("repos/")), "")
if "--method" in args and "POST" in args:
    count = int(state.read_text()) if state.exists() else 0
    state.write_text(str(count + 1))
    payload = json.load(sys.stdin)
    if mode == "validate_payload":
        single = next(comment for comment in payload["comments"] if comment["body"] == "Left-side feedback")
        if "start_line" in single or "start_side" in single:
            print("single-line comments must omit multiline fields", file=sys.stderr)
            raise SystemExit(2)
        multiline = next(comment for comment in payload["comments"] if comment["body"] == "Please verify this behavior")
        if multiline.get("start_line") != 1 or multiline.get("start_side") != "RIGHT":
            print("multiline comment coordinates are incomplete", file=sys.stderr)
            raise SystemExit(2)
    if mode == "lost_response":
        print("connection lost", file=sys.stderr)
        raise SystemExit(1)
    print(json.dumps({"id": 9001, "state": "COMMENTED", "comments": [{"id": 9100, "path": "a.ts", "line": 2, "body": "Please verify this behavior"}]}))
elif endpoint.endswith("/comments") and "/issues/" in endpoint:
    print(json.dumps([[{"id": 1, "body": "first", "user": {"login": "a"}}], [{"id": 2, "body": "second", "user": {"login": "b"}}]]))
elif endpoint.endswith("/comments"):
    print(json.dumps([[{"id": 3, "body": "inline", "position": None, "original_position": 2}]]))
elif endpoint.endswith("/reviews"):
    if mode == "reconcile":
        print(json.dumps([[{"id": 9001, "body": "Summary", "commit_id": head, "user": {"login": "reviewer"}}]]))
    else:
        print(json.dumps([[{"id": 4, "body": "review"}]]))
else:
    print(json.dumps({
        "number": 7,
        "title": "Test PR",
        "body": "Intent",
        "base": {"sha": base, "repo": {"full_name": "example/repository"}},
        "head": {"sha": head, "ref": "feature/JT-95384-settings", "repo": {"full_name": "fork/example"}},
    }))
