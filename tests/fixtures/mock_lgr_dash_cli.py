#!/usr/bin/env python3
import json
import os
import pathlib
import sys


args = sys.argv[1:]
record = {
    "args": args,
    "has_youtrack_secret": any(
        name in os.environ for name in ("YOUTRACK_TOKEN", "YOUTRACK_API_KEY")
    ),
}
if "--file" in args:
    context_path = pathlib.Path(args[args.index("--file") + 1])
    record["context"] = context_path.read_text()
with pathlib.Path(os.environ["MOCK_LGR_LOG"]).open("a") as output:
    output.write(json.dumps(record) + "\n")


def success(result):
    print(json.dumps({"ok": True, "result": result, "errors": []}))


if args[0:1] == ["--server"]:
    print("1")
elif args[0:2] == ["github", "--repository"] and "create" in args:
    success(
        {
            "session": {"id": "ses_test"},
            "pull": {
                "head_ref": "mc/jt-95384-hub-settings",
                "title": "JT-95384 Rework settings",
                "body": "Also preserves context from JT-95384.",
            },
        }
    )
elif args[0:2] == ["context", "add"]:
    success({"entries": []})
elif args[0:2] == ["graph", "build"]:
    success({})
elif args[0:2] == ["rank", "ses_test"]:
    print("RANKER_INTERNAL_WALL " * 200, file=sys.stderr)
    success({})
elif args[0:2] == ["tui", "ses_test"]:
    success({})
elif args[0:2] == ["comment", "list"]:
    drafts = {}
    if os.environ.get("MOCK_LGR_DRAFTS") == "1":
        drafts["cmt_test"] = {
            "remote_id": None,
            "anchor": {"path": "a.ts"},
            "stale": False,
            "orphaned_reason": None,
        }
    success({"drafts": drafts, "conflicts": []})
elif args[0:2] == ["github", "--repository"] and "preview" in args:
    success({"comments": [{"draft_id": "cmt_test"}]})
elif args[0:2] == ["github", "--repository"] and "submit" in args:
    success({"id": 9001})
else:
    print(json.dumps({"ok": False, "errors": [{"message": "unexpected command"}]}))
    raise SystemExit(2)
