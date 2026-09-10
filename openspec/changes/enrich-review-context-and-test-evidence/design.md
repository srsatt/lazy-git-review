## Context

See proposal.md for motivation. Inspection of the current implementation found:

- `src/tui/preview.rs` parses immutable patch lines into text plus old/new line numbers, with a 32 MiB cache and a bounded asynchronous source reader. `src/tui/render.rs` colors whole added/deleted lines; it has no syntax spans.
- `src/graph.rs` exposes Hunk, Symbol, Reference, and FileChange nodes. `src/tui/graph.rs` projects semantic edges onto changed hunks and relegates unchanged targets to secondary evidence. Tests that did not change therefore cannot appear as review items, even when they are relevant.
- `CoverageEntry` describes LSP indexing availability, not executed test coverage. New runtime evidence must use a separate schema and terminology.
- `src/ranking.rs` separates titles from assessments, with revision-checked writes. `src/progress.rs` tracks node IDs; comments use snapshot/side/path/range anchors. Chunking must coordinate all of these consumers.
- `src/context.rs` stores origin, timestamp, digest, Markdown, and truncation; `src/github.rs` already reads PR data and issue/review comments. The TUI receives neither a structured discussion index nor hunk-specific explanations.
- Main specs are empty; foundation and redesign contracts remain in unarchived changes. This proposal adds distinct capability specs rather than mutating those artifacts. During eventual synchronization, qualify the foundation's “one hunk remains one independently reviewable change” scenario as legacy/unpartitioned mode: the raw hunk stays addressable while chunk mode presents its children. Preserve all redesigned navigation and latency requirements.

## Goals / Non-Goals

**Goals:** Reuse parser/highlighting assets; provide deterministic review units; expose explainable, attributed evidence next to captured code; support a usable runner-independent coverage path; retain fast keyboard interaction and stable comment coordinates.

**Non-Goals:** A new model provider, mandatory LLM generation, automatic defect findings, automatic publication, executing tests during ordinary browsing, universal runner instrumentation, or automatically crawling linked ticket/report services.

## Decisions

### 1. Share captured-source parsing between highlighting and chunk boundaries

Use embedded Tree-sitter grammars and `tree-sitter-highlight` for TS/TSX, JS/JSX, HTML, and CSS. Pin compatible grammar/query assets and retain their licenses. LSP remains responsible for cross-file references; cached symbol ranges are preferred chunk boundaries, with syntax boundaries as fallback. Avoid a separate editor process and independent parser frameworks.

Parse each captured side separately. Map token byte ranges onto patch old/new line coordinates before converting tabs, controls, and Unicode to display spans. Highlighting patch fragments alone would lose multiline string/comment context. Syntax colors occupy text foreground; diff markers/gutters and restrained background roles preserve addition/deletion meaning. LLM emphasis uses an annotation marker/underline rather than overwriting syntax or diff state.

Add optional syntax roles to existing palettes with defaults, preserving all old custom JSON palettes. `tui.syntax_highlighting` defaults true; unknown languages, parser failures, binary files, and files above a 2 MiB parse budget keep the plain captured preview. Background work has a bounded queue, cancellation/generation IDs, and a 32 MiB token cache; keys and first plain preview remain available while parsing. Theme changes restyle cached token classes without reparsing.

Reference: [Tree-sitter syntax highlighting](https://tree-sitter.github.io/tree-sitter/3-syntax-highlighting.html) documents reusable highlight queries and the Rust highlighter library.

### 2. Add a versioned review-unit projection, preserving raw Git hunks

Persist `review-units.json` with schema version, snapshot ID, graph revision, partition version/config digest, unit IDs, parent hunk ID, patch-row ownership, and old/new ranges. Do not rewrite `snapshot.json` patches or reuse raw hunk IDs for children. Unit IDs hash parent identity, algorithm version, and canonical owned ranges. Identical inputs reproduce identical units.

Default new-session splitting: split textual hunks above 120 changed lines, target 80 changed lines, hard ceiling 120 owned changed lines per unit. Prefer function/method/component/test boundaries, then statement/block boundaries. An oversized function or syntax-free text falls back to bounded contiguous patch-row windows. Keep a contiguous delete/add replacement together when it fits the ceiling; otherwise split its patch rows explicitly. Up to three context lines can overlap, but every added/deleted patch row belongs to exactly one unit. A single enormous source line remains one unit and uses existing bounded display behavior.

For a 300-line added module, create multiple function-oriented units (or deterministic bounded windows); raw parent remains retrievable. Unchanged/metadata changes retain current inventory handling. Expose each unit's parent and part number, and permit full-parent preview.

New queues rank/count leaf units only. Parent symbol/hunk assessments can supply inherited score/rationale/title provenance; chunk-specific assessments override them. A reviewed parent implies all children only during explicit migration. Once any child changes status, parent reviewed state is derived from all children; filters never alter denominators. Old comments remain at exact coordinates and are visible from intersecting children; new comments use explicit selected ranges and existing GitHub anchor validation. Never use a generated chunk header as the authority for GitHub positions.

Legacy sessions default to their saved hunk queue. Explicit `lgr review partition SESSION` previews the mapping; `--apply` records the chosen projection and migrates exact-match state atomically with backup. Repartitioning changed boundaries retains parent comments and marks ambiguous child progress unreviewed. A projection revision mismatch rejects writes. Raw graph IDs and existing CLI commands stay valid; add `graph units`/`--unit` projections and expose unit IDs in the external-agent protocol.

### 3. One-key Code/Context pane, source-backed explanations

`i` toggles Code/Context in queue and related views; `I` opens the session context overview. Tab still changes pane focus; existing `t`, `T`, `v`, `c`, `l`, `h`, and `gq` keep their meanings. Each navigation frame retains pane mode, code scroll, context scroll, and active item. Context sections show What/why, Comments, Tests, and Sources only when they contain content; missing context offers the relevant add/enrich action. Session-wide PR description and unassigned discussions remain reachable without forcing an invented hunk association.

Introduce a separate `explanations.json` sidecar, indexed by raw hunk or review unit. Each entry has a short explanation, optional bounded side/path/line annotations, evidence references, authority, and input digests (snapshot, units, context, ranking, test evidence). Store concise conclusions intended for reviewers, not hidden chain-of-thought. Maximum 4 KiB text and eight annotations per item; each annotation has at most 512 bytes and exact captured ranges. Validate evidence existence, range bounds, and control bytes in atomic revision-checked batches. Manual notes remain separate from model output and cannot be overwritten by enrichment.

Reuse external-agent profiles via explicit `lgr explain SESSION` (batch selection/force, dry-run, cancellation) and a corresponding bounded read/write CLI. No launch on `i`. The fast ranker can optionally attach short explanations while ranking; a dedicated explain pass can read deeper context without changing scores or finalization. Display old explanations as stale when cited inputs change and retain the source excerpts that explain their provenance. Model annotations are local review evidence, never automatically GitHub draft comments.

### 4. Normalize captured context and attach by evidence

Extend context entries additively with source kind, external identity/URL, author, source update time, thread/reply identity, capture status, and optional snapshot/side/path/range. Reuse stored GitHub payloads, add missing thread state where available, and preserve resolved/outdated comments visibly. PR discussion here means PR comments, review summaries, and review threads; arbitrary GitHub Discussions URLs remain explicit Markdown/hook attachments.

Local Markdown and Folio Markdown exports enter the same bounded importer; optional configured hooks can call a local Folio export command for a user-supplied report ID. No Folio runtime dependency for normal review. Stable source keys plus content digests make repeated imports idempotent. Refresh is explicit, paginated, and reports partial/failure status without discarding previous usable context.

Exact captured inline anchors attach automatically. Old-commit or unmatched anchors remain session context/outdated, never guessed by filename or shifted silently. User/model associations carry their own provenance and confidence. Citations resolve to captured excerpts first, with original links as secondary metadata. Render Markdown as inert text; repository documents cannot configure or trigger tests, hooks, agent launches, or publishing. Adding explanatory output or draft comments does not silently modify the ranking intent digest; importing changed intent retains the existing stale-ranking behavior.

### 5. Runtime test evidence is an optional, separately versioned overlay

Add explicit commands `lgr tests run SESSION --profile NAME`, `tests import`, and `tests show`. Settings hold executable plus argv, project root, timeout, output limits, report format and source mapping. Selection and limits are visible in `--dry-run`; no shell interpolation or inferred command execution. Start with normalized manifest v1 plus Istanbul JSON and LCOV import. Ship a real Jest test-file isolation recipe exercising `--runTestsByPath` and coverage reports as an acceptance adapter; other runners can emit the same contract. Native per-case collection is optional and must demonstrate its attribution isolation before claiming case-level links.

A manifest records snapshot/side, run and runner identity/version, command/config/dependency/source digests, test identity and granularity (case/file/suite), test path/location when known, status, instrumented source hashes, remapped ranges/counts, report digest, and completion/cancellation. Combined reports without attribution remain suite-level evidence. Sequentially running one test file per report yields file-level links, not individual `it` links. Shared module initialization is labeled execution evidence; line hits never prove an assertion or behavior correct. Failed runs can retain observed hits alongside failure status; missing/skipped/partial coverage is unknown rather than zero.

Run only on an explicitly selected side (right by default; left is a separate run) in a disposable copy of the captured tree, separate from immutable storage. Dependency preparation is a configured step; missing dependencies fail with a useful message. Do not automatically install packages, reuse a mutable source tree, or call the copy a security sandbox. Capture stdout/stderr under byte limits, expose progress, and terminate the child process group on cancellation. Use default budgets of ten minutes, 8 MiB output, and 64 MiB coverage input, configurable per profile.

Import validates original-source remapping and hashes against the selected snapshot; reports for unrelated revisions are inspectable but cannot create current links. Never map right-side hits to deleted left-side lines. Cache only completed runs with matching snapshot, test selection, source/config/dependency/runner fingerprints; explicit force bypasses reuse. Freshness of test evidence is independent from LSP index coverage and can stale dependent explanations without invalidating static graph IDs.

Project dynamic edges onto changed units using executed source range intersections, retaining test identities as context nodes even when their files are unchanged. `l` shows Tests/Usages/Definitions with changed units plus separately marked unchanged test targets. Selecting an unchanged test previews its captured source; it never enters completion totals. Merge duplicate static/runtime targets while preserving both evidence kinds; filters can distinguish executed, static, and heuristic links.

References: [Jest CLI](https://jestjs.io/docs/cli) supplies file selection and coverage options; [Vitest coverage](https://vitest.dev/guide/coverage.html) describes coverage providers. The proposed per-file manifest derives attribution from isolated runs rather than assuming ordinary combined reports contain test identities.

### 6. Verification and response budgets

Extend existing Rust and headless Neovim fixtures instead of replacing them. Cover a 300-line addition with several functions/tests, a giant single function, replacements/deletions/renames, binary changes, CRLF/emoji, stale reports, unchanged tests, parameterized identities, cancelled test children, invalid annotations, and old serialized sessions.

Measure release-mode startup and 200 mixed Code/Context/select/follow/back actions on at least 10k nodes/10k edges/100 leaf units. Preserve useful first frame under 1s, first plain preview under 100ms, cached p95 under 50ms; target highlighted selected files up to 256 KiB within 250ms after their source is available. Record hardware, cold/warm cache, bounded memory, and process/IO counts. Navigation must spawn no agent/LSP/test/network work. Prove actual installed Diffview opening and `gq` return with context mode/offsets retained.

## Risks / Trade-offs

- Per-test isolation can be slow → explicit selection, file-level first adapter, budgets, caching, clear partial results.
- Coverage includes initialization and misses assertions → label execution scope and outcomes separately; never equate hits with behavioral correctness.
- Chunking changes ranking denominators → opt-in legacy migration, leaf accounting, raw parent preservation, backup/revision checks.
- Source-map mismatches can fabricate links → require matching original source hashes and retain unresolved mappings as diagnostics.
- Syntax colors can obscure diffs or model emphasis → distinct role layers and monochrome tests across built-in/custom themes.
- Documents may conflict or become stale → preserve author/source/time and cited captured versions; distinguish model interpretation from source text.
- New overlays can inflate caches → bounded per-item text, LRU token/source caches, bounded evidence traversal and paginated imports.

## Migration Plan

Deliver schema/CLI compatibility and partitioning first, then highlighting, runtime evidence, context/explanations, and the complete keyboard flow. Additive settings get defaults. Existing raw sessions keep their queue; explicit partition migration writes backups and transactional projection state. Disabling new enrichment/highlighting leaves raw review available; rollback restores the prior queue/progress backup without deleting drafts or imported evidence. Before release, reconcile the overlapping unarchived spec contracts and record the exact installed consumer path. No implementation or existing-spec synchronization occurs during this proposal.
