# Release acceptance

Host: Apple Silicon macOS, 2026-09-08. Historical Inker range: `475e161075c508ebe23fe64b69e263247cd869ba..154cb1ccb75d32e3387ccc914ebf0034503bd4cd`.

| Capability | Evidence | Result |
| --- | --- | --- |
| Review snapshots | Rust fixtures cover branch merge-base, direct commits, staged/unstaged/untracked, rename/delete/binary/mode/submodule/raw paths, private materialization, and refresh | Pass |
| Semantic graph | Mock lifecycle tests plus real TypeScript/TSX/HTML/CSS servers; split projects, alias, decorator, excluded tests, position encodings, cache invalidation | Pass |
| Review context | Offline Markdown, digests, UTF-8 limits, absolute argv hook, failure/continue, and non-executing untrusted context tests | Pass |
| Semantic ranking | Scripted skill example and pinned Inker ranking: 82/82 assessed; device API score 98 precedes lockfile score 5 | Pass |
| Review navigation | Ratatui filtering/progress/conflict tests, interactive PTY navigation, and actual `<leader>rc` capture through the pinned plugins | Pass |
| Review comments | Workflow fixture covers plugin identity, LEFT/RIGHT anchor model, readable Markdown, idempotent import, conflict retention, and preview | Pass |
| GitHub reviews | Fork/pagination/anchor/identity/staleness/journaling/lost-response/reconciliation mocks plus repository-matched multi-profile tests | Pass; live authorized PR proof pending |

Inker graph: 82 hunks, 9,285 nodes, 5,408 edges, no unfinished frontier. Cold graph 15.36s, cached graph 0.66s, overview 0.13s. Resolved test-reference evidence includes screen composer, renderer, and weather-widget tests. External ranker: Codex GPT-5 following the bundled `semantic-review-ranker` skill; four measured graph queries, 76,596 returned bytes, 18,718 requested source bytes, 517ms aggregate CLI query time; all scores, tags, rationales, and evidence persisted without a model provider inside `lgr`.

Release cannot be called complete until an explicitly authorized test PR is submitted and fetched back. Mocks intentionally do not satisfy the live-publication gate.

The follow-up enriched-review acceptance is recorded in [enrich-review-context-and-test-evidence.md](enrich-review-context-and-test-evidence.md). It covers review-unit partitioning, captured syntax highlighting, runtime test evidence, context/explanations, the installed Neovim flow, and the expanded performance benchmark without changing the pending live-publication gate above.
