#!/bin/sh
set -eu

if [ "$#" -lt 3 ] || [ "$#" -gt 4 ]; then
  echo "usage: example-ranking.sh SESSION GRAPH_REVISION NODE_ID [titles-only]" >&2
  exit 2
fi

session=$1
revision=$2
node=$3
lgr=${LGR_BIN:-lgr}

run_lgr() {
  if [ -n "${LGR_DATA_DIR:-}" ]; then
    "$lgr" --data-dir "$LGR_DATA_DIR" "$@"
  else
    "$lgr" "$@"
  fi
}

if [ "${4:-}" = "titles-only" ]; then
  run_lgr graph hunks "$session" "$node" --max-bytes 65536
  run_lgr graph label "$session" --graph-revision "$revision" --updates "[{\"node_id\":\"$node\",\"title\":\"Explain the selected semantic change\",\"evidence_ids\":[\"$node\"],\"authority\":\"model\"}]"
else
  run_lgr graph evidence "$session" --max-bytes 65536
  run_lgr graph score "$session" --graph-revision "$revision" --compact --require-complete --updates "[{\"node_id\":\"$node\",\"title\":\"Explain the selected semantic change\",\"score\":50,\"tags\":[\"behavior\"],\"rationale\":\"Example ranking based on the selected hunk\",\"confidence\":0.5,\"evidence_ids\":[\"$node\"],\"authority\":\"model\"}]"
  run_lgr graph finalize "$session" --compact --require-complete
fi
