#!/usr/bin/env python3
import json
import os
from pathlib import Path
import sys

prompt = sys.argv[-1]
evidence = json.loads(prompt.split("<lgr_evidence>\n", 1)[1].split("\n</lgr_evidence>", 1)[0])
data = Path(os.environ["LGR_DATA_DIR"])
(data / "agent-session").write_text(os.environ["LGR_SESSION_ID"])
(data / "agent-prompt").write_text(prompt)
(data / "agent-budget").write_text(str("LGR_AGENT_BUDGET_PATH" in os.environ).lower())
print(json.dumps({"assessments": [{
    "node_id": item["node_id"],
    "title": "Review captured change",
    "score": 50,
    "tags": ["behavior"],
    "rationale": "Captured behavior changes",
    "confidence": 0.5,
    "evidence_ids": [item["node_id"]],
    "authority": "model",
} for item in evidence["items"]]}))
