# Ranking CLI

`lgr rank SESSION` does not use these commands: LGR embeds bounded evidence in one prompt, consumes one JSON model response, and applies/finalizes locally. The commands below remain available for manual/API ranking and for bounded title/explanation modes.

All commands emit one schema-versioned JSON envelope on stdout. `graph evidence` returns readable patch text; decode `content_base64` from source and hunk responses.

Agent-launched title and explanation commands share a hard allowance of 16 successful response envelopes and 196608 serialized bytes. Exceeding either returns `agent_response_budget_exceeded` instead of the requested payload.

```sh
lgr graph evidence SESSION --max-bytes 131072
lgr graph overview SESSION
lgr graph units SESSION [--parent HUNK_ID]
lgr graph nodes SESSION ID... --fields id,kind,name,locations,hunk_ids
lgr graph walk SESSION --seeds ID,ID --edges runtime_test,test_reference,calls --depth 2 --limit 100
lgr graph walk SESSION --seeds ID,ID --edges runtime_test,test_reference,calls --depth 2 --limit 100 --continuation TOKEN
lgr graph hunks SESSION HUNK_OR_UNIT_ID... --max-bytes 65536
lgr graph source SESSION NODE_ID... --max-bytes 65536
lgr graph score SESSION --graph-revision grf_... --compact --require-complete --updates '[{"node_id":"h_...","title":"Reject expired authentication tokens","score":90,"tags":["security"],"rationale":"Authentication boundary changed","confidence":0.9,"evidence_ids":["h_..."],"authority":"model"}]'
lgr graph label SESSION --graph-revision grf_... --updates '[{"node_id":"h_...","title":"Reject expired authentication tokens","evidence_ids":["h_..."],"authority":"model"}]'
lgr graph queue SESSION
lgr graph finalize SESSION --compact --require-complete
lgr context show SESSION
lgr tests show SESSION
lgr explain SESSION --show
lgr explain SESSION --graph-revision grf_... --expected-revision 0 --updates '[{"item_id":"u_...","text":"Routes authentication failures through the shared guard.","annotations":[{"side":"right","path":"src/auth.ts","start_line":42,"end_line":44,"label":"new rejection path"}],"evidence_ids":["u_...","e_..."]}]'
```

Built-in tags: `security`, `non-trivial-logic`, `behavior`, `api`, `tests`, `configuration`, and `mechanical`. Short user-defined tags are allowed.

Titles are optional for backward-compatible score clients and limited to one safe line of 120 Unicode characters. `graph label` changes only leaf labels; it does not assess, score, unfinalize, or finalize anything. Explanation text/manual notes are limited to 4 KiB; each item accepts at most 8 annotations with 512-byte labels. Hard request limits: traversal depth 16, traversal nodes 10,000, source/hunk page 1 MiB, 16 tags per assessment, 48 bytes per tag, and 500 rationale characters. Unknown IDs in node/source/hunk batches return per-item errors. Score, label, and explanation batches are atomic.

`graph evidence` bounds the complete serialized response and distributes the remaining patch allowance across active changes. It also returns context, graph coverage, and explicit completeness. Compact score and finalize responses contain counts and revisions without echoing the queue.

Expansion is explicit and may run language servers:

```sh
lgr graph expand SESSION --time-budget 60 --max-files 500 --max-symbols 5000
```
