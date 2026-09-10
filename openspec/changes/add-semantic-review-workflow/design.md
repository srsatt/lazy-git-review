## Context

See [proposal.md](proposal.md) for motivation and release scope. The repository has only `README.md` and OpenSpec scaffolding; there is no application architecture, test suite, or existing capability contract to preserve.

Observed inputs, inspected on 2026-09-08:

- The local Inker acceptance checkout has 327 tracked paths. Its root scripts coordinate a Bun/NestJS TypeScript backend and a React/Vite TypeScript frontend. The frontend uses bundler module resolution and React JSX; the backend uses CommonJS, decorators, and `@/*` aliases. Both application TypeScript configs exclude test files. Backend tests use `bun:test`; frontend tests use Vitest. These are separate language-server project configurations, not one default TypeScript project.
- Inker has unrelated working-tree edits. Acceptance must use isolated, pinned sources. The renderer introduction `154cb1ccb75d32e3387ccc914ebf0034503bd4cd`, based on `475e161075c508ebe23fe64b69e263247cd869ba`, changes 37 files with 1,456 insertions and 116 deletions, including backend logic, TSX, and tests. The later `22247acbedbfe6d4aa5d947d443aaccb1f9ae91d` is another available renderer fixture. Neither is proof of all HTML/CSS/frontend paths.
- The user's `dotfiles/nvim/lua/plugins/code-review.lua` loads `choplin/code-review.nvim` with default options. `lazy-lock.json` pins `ed91462e20bd08c3be71efb11a4a7d00459f0b47`. The default `<leader>rc` adds a line/range comment; default storage is memory. Its optional file backend writes per-thread Markdown. The model stores file, line range, body, and context, but no revision or diff side. The parser drops unknown frontmatter fields, and built-in list navigation uses ordinary `:edit`.
- Diffview and Octo are installed in the user's Neovim configuration. Diffview supports revision comparisons and path filtering; it displays non-local sources in synthetic buffers. Octo has GitHub review functionality, but no observed public import interface for the user's comment plugin. Integration cannot assume these plugins already share anchors or a queue.

External evidence:

- [Diffscope](https://github.com/evalops/diffscope) documents optional LSP symbol indexing, graph-context limits, Markdown output, and PR posting. Its [manifest](https://raw.githubusercontent.com/evalops/diffscope/main/Cargo.toml) is Apache-2.0 and includes model/server/database dependencies. These are useful design references; a small reusable graph library API was not established by this inspection.
- [LSP document symbols](https://raw.githubusercontent.com/microsoft/language-server-protocol/gh-pages/_specifications/lsp/3.17/language/documentSymbol.md) and [call hierarchy](https://raw.githubusercontent.com/microsoft/language-server-protocol/gh-pages/_specifications/lsp/3.17/language/callHierarchy.md) provide structure and optional call information, not a ready-made Git change graph or universal runtime dependency graph.
- [typescript-language-server](https://github.com/typescript-language-server/typescript-language-server) supplies a stdio LSP interface to TypeScript. [vscode-langservers-extracted](https://github.com/hrsh7th/vscode-langservers-extracted/blob/master/README.md) supplies HTML and CSS servers. Their actual advertised capabilities must be recorded during qualification.
- [async-lsp](https://docs.rs/async-lsp/latest/async_lsp/) supports language clients as well as servers. [Ratatui](https://github.com/ratatui/ratatui), [Diffview](https://github.com/sindrets/diffview.nvim), and [GitHub's review API](https://docs.github.com/en/rest/pulls/reviews) cover the remaining external building blocks.

## Goals / Non-Goals

**Goals:** Keep the custom code concentrated in snapshot identity, graph assembly, ranking, and integration adapters. Make every reported relationship traceable to captured source. Make ranking efficient for a smaller external model, with durable output another model or human can consume. Finish all seven capability contracts in one release.

**Non-Goals:** A model-provider SDK, autonomous bug-finding agent, code modification, hosted service, custom diff renderer, general compiler/data-flow framework, complete cross-language dependency analysis, or first-release GitHub reply/edit/resolve synchronization. The installed plugin's local thread features do not imply equivalent GitHub operations.

Planning defaults selected from the user's answers: executable name `lgr`; initial binaries for macOS Apple Silicon and Linux x86_64; editable `review.md` plus stable anchor metadata; existing CLI-capable agent performs ranking; user-configured Markdown hooks cover ticket retrieval. These choices complete the unanswered packaging/comment-format details without adding another service or model integration.

## Decisions

### 1. One Rust package with small modules and reusable adapters

Use a single Cargo package initially, with a library core and `lgr` binary. Split `src/` by `snapshot`, `lsp`, `graph`, `context`, `ranking`, `session`, `comments`, `github`, `editor`, `tui`, and CLI command responsibility. Keep Lua integration under `integrations/nvim/`, the distributable skill under `skills/semantic-review-rank/`, and fixtures under `tests/fixtures/`. Do not build a crate framework before there are multiple consumers that require one.

Reuse Git subprocesses for revision/diff behavior; Clap for command parsing; Serde for JSON; Tokio and async-lsp for bounded LSP processes; rusqlite with bundled SQLite for session persistence; Ratatui/Crossterm for the queue; a Markdown parser for exchange; Neovim's RPC and existing plugins for editor UI. Pin compatible dependencies and a tested Rust toolchain during implementation. Language-server executables and their runtimes remain external, discoverable dependencies.

Publish the first public version as `0.1.0` under EUPL-1.2. Use SPDX identifier `EUPL-1.2` in Cargo metadata and place the canonical English EUPL-1.2 text in `LICENSE` with `Copyright 2026 Pavel Reutov. Licensed under the EUPL-1.2.` Release source and archives include `LICENSE` and `THIRD_PARTY_LICENSES.md`; third-party components retain their own licence notices. Cargo version, binary version, Git tag, archive name, and release notes must identify the same `0.1.0` release.

Documentation uses a short README as entrypoint plus focused guides: installation and platform prerequisites; configuration and agent profiles; Neovim/Diffview/code-review.nvim and gh-dash integration; troubleshooting, data backup, upgrade, and uninstall. Archive and source instructions converge on the same installed binary, helper commands, shared assets, settings layout, and `lgr doctor` verification. Examples use portable placeholders and clean isolated acceptance rather than relying on the developer's machine state.

Alternative: embed Diffscope wholesale. Rejected because its automated review/model/server surface is broader than the desired ranking tool. Prefer its documented approaches and a small upstream extraction only if implementation discovers a genuinely reusable, licensed interface; the release must not depend on an uncommitted upstream refactor. A new TUI diff renderer is also unnecessary.

### 2. Immutable source snapshots, separate mutable session state

The persistent model separates:

| Record | Identity and role |
| --- | --- |
| Snapshot | Repository identity, input mode, original base/head IDs, merge base, before/after tree and content digests |
| Graph revision | Snapshot ID, indexing configuration/server versions, nodes, edges, coverage and unfinished frontier |
| Context bundle | Snapshot ID, captured metadata/documents/hook outputs and digest |
| Ranking run | Graph revision + context digest, scores/tags/evidence, budget accounting, completeness |
| Review state | Session revision, selected item, reviewed flags and manual overrides |
| Comment | Stable ID, snapshot anchor, body revision, optional plugin and GitHub IDs |
| Submission | Target repository/PR/head, selected draft versions, payload digest, attempt state and remote IDs |

SQLite transactions and revision preconditions coordinate short-lived CLI processes, TUI, and Lua calls without a service daemon. Use WAL and bounded busy timeouts. Store immutable source blobs and editor materializations in per-user application storage, namespaced by repository and session; do not put cache data or `.code-review` files in the user's source checkout by default. Markdown is an exchange surface, not a competing authoritative database.

Layer launch caching by identity rather than elapsed time. Opt-in review creation reuses the newest session only when canonical repository, comparison kind, resolved commits, selected index tree, tracked diff bytes, and selected untracked path/content bytes match; it confirms the fingerprint before returning. Graph building independently validates its analysis fingerprint. Ranking reuse requires a finalized result bound to the current graph revision and context digest. Each response reports its own cache hit, and `graph build --force` or `rank --force` bypasses the relevant layer. This preserves progress and comments while preventing Git status strings alone from hiding changed bytes.

For PR/branch input, resolve commits once and compute the requested merge-base comparison. Also accept Diffview-style revision expressions: `A..B` compares the two resolved endpoints directly, while `A...B` compares the merge base of the endpoints to `B`. Explicit `--base/--head` flags remain available. For staged/unstaged input, capture the index and selected working-tree blobs into private storage, then verify content/index fingerprints before publishing the snapshot. Untracked inclusion is explicit; ignored paths remain excluded unless individually supplied. A file-level change item represents binary, mode-only, or submodule changes that have no text hunk.

Use Git's explicit, stable diff settings: disable external diff/textconv/hooks for capture, request full object identities and rename information, and define hunk context consistently. Handle filenames via NUL-delimited Git metadata and proper Git patch-path decoding. Never derive old-side lines from current working-tree content. Preserve merge conflicts as an unsupported comparison with a clear recovery message rather than selecting an index stage silently.

Materialize captured sources into a tool-owned Git workspace for LSP and editor use. Reuse original commit objects for committed reviews; represent non-commit index/worktree snapshots with private synthetic before/after commits so Diffview can use its normal revision-pair interface. Original PR SHAs remain separate from private visualization commits and are the only eligible GitHub submission identities. Private object creation must never create a commit, ref, or hook side effect in the source repository.

Alternative: analyze and display the live checkout. Rejected because subsequent edits invalidate evidence and comments, while stashing or checking out PR branches would disrupt the user's work.

### 3. LSP supplies evidence; Git defines what changed

Start with a complete raw Git inventory. Identify relevant project roots from language configuration and explicit overrides, preserving multiple TS configurations. Index both before and after sides in isolated roots so deletions, renames, and removed callers are represented. Limit concurrent servers and cache results by source/configuration digest, project root, server executable/version, and dependency fingerprint.

Use `initialize`, negotiated position encodings/capabilities, document opening, document symbols, references, definition/type-definition/implementation when useful and supported, and prepared incoming/outgoing call hierarchy. Handle server configuration/workspace requests, progress, cancellation, partial results, timeouts, shutdown, and crashes through the LSP framework. Reject server requests to apply workspace edits; indexing performs no code actions. Source content remains the captured version.

For each hunk, associate all overlapping before/after symbols, retain unmatched hunk content, and expand a bounded neighborhood of affected symbols and references. Node keys include snapshot, side, path, kind, and range; rename matching is a separate evidence-bearing relation, not identity guessed from a symbol name. API storage uses normalized line/column ranges with explicit encoding; Git/editor conversion operates on the captured bytes.

TypeScript/JavaScript, TSX/JSX, HTML, and CSS are first-release qualification profiles. Capability coverage differs by server. CSS selectors and HTML document structure are useful even without call edges; missing cross-language bindings remain unknown. Unsupported files keep raw hunks. Do not add a second parser or a regex-based semantic graph as an invisible fallback.

Project dependencies are optional for raw review but influence semantic resolution. Reuse existing dependency installations only when explicitly configured and fingerprinted; report a mismatch against a historical snapshot's lockfiles. Never run installation or build scripts automatically. Initial qualified fixtures must resolve their expected project-local references without relying on unrecorded current-checkout state. Disable project-provided executable LSP plugins by default unless the user has configured them as trusted.

Alternative: a head-only symbol index. Rejected because it cannot reliably describe deleted functions, removed callers, or LEFT-side evidence.

### 4. Tests are typed relationships with honest strength

The core relations are `contains`, `overlaps`, `references`, `calls`, `test_reference`, and `counterpart`. Each has source side, location, producer, confidence, and optional evidence details. Traversal can invert direction without storing duplicate inverse edges.

Recognize test files through configurable globs and framework conventions. A resolved reference from a test is `test_reference` with an underlying reference kind. Naming-only links use the same queryable test category with `evidence_kind=heuristic`, never `resolved`. A missing edge means unknown coverage, not a missing test or passing test.

Because Inker excludes tests from application configurations, enumerate adjacent and configured test roots explicitly and open their documents. If the server still excludes them from project reference search, run a test-aware indexing profile in a separate disposable overlay which preserves the compiler options/aliases and adds the test files to project membership. Record that derived configuration and its diagnostics; do not rewrite Inker's configs. Validate actual resolved helper imports in fixtures, rather than accepting filename association as proof of semantic linking.

Alternative: flatten tests into ordinary references. Rejected because the ranking agent needs a cheap way to inspect evidence around changed logic without exhausting production-reference neighborhoods.

### 5. Bounded JSON CLI with explicit external agent launch

All commands below are proposed release interfaces, not currently implemented commands:

```text
lgr review create --pr <url-or-number> --context intent.md --json
lgr review create --base main --head feature --json
lgr review create feature-A..origin/develop --json
lgr review create origin/develop...feature-A --json
lgr review create --staged --json
lgr review create --unstaged --json
lgr graph build --session S --time-budget-seconds 180 --json
lgr graph overview --session S --json
lgr graph nodes --session S --ids H1,S2,S3 --fields summary --json
lgr graph walk --session S --seeds S2,S3 --edges references,test_reference --direction both --depth 2 --json
lgr graph expand --session S --seeds S2 --depth 1 --json
lgr graph source --session S --ids H1,H2 --format patch --json
lgr graph batch --session S --input requests.json --json
lgr rank apply --session S --input scores.json --json
lgr rank finalize --session S --json
lgr rank S --profile work
lgr rank S --agent opencode
lgr queue --session S --json
lgr tui --session S
lgr comments export --session S --output review.md
lgr comments import --session S --input review.md
lgr github preview --session S --event COMMENT --json
lgr github submit --session S --preview P --json
```

Use schema-versioned envelopes with request ID, session ID, graph/context revisions, results, errors, coverage, byte usage, and continuation cursors. Stdin is accepted instead of a file for batch requests. Read batches return per-operation errors; write batches validate all records then commit atomically. No arbitrary SQL, shell evaluation, or agent-selected executables are exposed.

Defaults: summary fields, depth 2, at most 200 returned nodes and 400 edges, 32 KiB per response, and a configurable wall-clock deadline. The bounds cover the whole batch. Source chunks are independently paginated; no silent truncation inside a hunk. Resume tokens bind the query and graph revision, deduplicate visited nodes, and preserve the frontier. An oversized result returns a resumable item or explicit size error. `graph expand` performs bounded LSP work and publishes a new graph revision; ordinary reads do not invoke indexing.

The supplied agent skill proceeds inventory/context → batch triage → targeted reference/test neighborhoods → selected source → score batch → coverage/finalization. It tracks returned bytes and query count, avoids fetching unchanged source repeatedly, and uses a default run budget of 60 CLI queries and 512 KiB returned content, overridable for larger reviews. It can finalize partial assessment only as explicitly partial; it cannot call such a run complete. No model provider, price, or unsupported accuracy claim is embedded in the binary.

Use `~/.lgr/settings.json` as the single versioned user configuration. Its default `data_dir` is `~/.lgr/data`, so normal consumers share sessions without repeated flags; CLI flags remain explicit one-command overrides. Reserve `~/.lgr/scripts` for user-owned hooks and agent scripts. Store named agent profiles as structured argv. Public defaults use generic `codex` and `opencode` commands; machine-specific profiles remain only in user settings. `--profile/-p` chooses one profile for a run; `--agent/-a` instead chooses an ad-hoc executable, resolving bare user script names from the scripts directory before `PATH`.

`lgr rank` launches the selected external process in the reviewed repository, passes session/data identities in environment variables, and appends a prompt containing the bundled ranking skill and CLI reference. Known CLI normalization makes `--agent opencode` invoke `opencode run`; generic scripts receive the prompt as their last argv element. Arguments are never shell-evaluated. `--dry-run` exposes the resolved argv without execution. This is process orchestration only: `lgr` still contains no model provider client or credentials, and the injected skill retains ranking-only authority. Relative configured directories resolve from the settings file, invalid or unknown fields fail closed, and installation never overwrites an existing valid settings file. Version-one harness fields load as their equivalent version-two agent-profile fields.

Alternative: dump the entire diff/graph into one prompt, or force a one-node-at-a-time tool API. Both defeat the user's request for efficient graph access. MCP can later wrap the stable CLI contract; it is not another first-release transport to maintain.

### 6. Rank hunks and symbols, then project onto one hunk queue

Each assessment contains a score from 0 to 100, a bounded confidence value, tags, short rationale, evidence IDs, and producer/run metadata. Scores express review importance, not the probability of a bug. Seed tags include `security`, `non-trivial-logic`, `behavior`, `api`, `tests`, `configuration`, and `mechanical`; custom labels are supported. Tags are independent of symbol kinds and test-reference edge types.

Effective hunk score: explicit manual hunk override; otherwise explicit agent hunk score; otherwise the highest assessed overlapping symbol score; otherwise unranked. A manual symbol override takes precedence over that symbol's agent score during inheritance. Explicit manual queue placement is retained separately from the calculated score. Unranked changes stay in a clearly labeled lane and count against complete assessment. Stable ties use normalized path, old/new start, and hunk ID. File-level non-text changes use the same assessment/status contract.

Symbols expose all their hunks and aggregate context, but each hunk contributes once to review completion. Cross-file relationships help navigation and scoring; they do not force multi-file semantic groups. A symbol score can provisionally cover its hunks, but the agent's final coverage report distinguishes direct hunk inspection from inherited assessment.

Finalization binds graph revision and context digest and verifies that every change has an assessment or explicit unresolved disposition. Full ranking requires every change assessed; unresolved dispositions yield a partial result. A new graph/context revision marks the result stale. Refresh can carry source-identical drafts/progress with a recorded match, but requires revalidating ranking for changed context or relationships.

Alternative: file scores or a fixed risk formula based on reference count. Rejected because files can contain unrelated changes and incomplete LSP references must not become false confidence.

### 7. TUI controls the queue; Neovim remains the review surface

Ratatui shows a compact ranked list, current symbol/path, score/tags, rationale on demand, and reviewed/remaining counts. Queue rows left-elide long paths so filenames and hunk/symbol names remain distinguishable. It does not render a full diff or a force-directed graph canvas. A graph drill-down builds one adjacency index from the already-loaded persisted graph, then shows typed directed neighbors for the selected hunk or symbol. Test references, usages/references, definitions/imports, and calls sort ahead of structural edges. Reviewers can traverse to a neighbor, backtrack, open its source, or return to the unchanged queue without an LSP, agent, network, or storage round-trip. Keyboard actions also move, filter, override priority, mark reviewed, open the editor, and export comments. Status is persisted transactionally; merely opening an item does not mark it reviewed.

Offer two transports: attach to an explicitly supplied Neovim socket, or suspend the TUI terminal while launching Neovim and resume on exit. Use a typed message with session, snapshot, file, side, and range. Pass arguments as structured process/RPC values and use `vim.system` argument arrays from Lua. Do not interpolate filenames into Lua or shell expressions.

Common editor launch actions compose the existing CLI stages asynchronously: snapshot creation, graph build, configured external ranking, session attach, then first-item navigation. The all-uncommitted input compares `HEAD` to the working tree and includes staged, unstaged, and untracked content; staged-only and unstaged-only inputs remain distinct. Branch launch uses a configurable development ref or a prompted ref with merge-base semantics and never fetches or changes refs. Reject concurrent launches instead of interleaving mutable session state.

For all input modes, open the private snapshot workspace with Diffview's supported revision-pair/path-filter interface. Select the correct diff-side buffer and jump to the captured range after its buffer-open event. Resolve buffer identity through a small, pinned Diffview adapter and attach canonical metadata to the buffer. Use the actual revision identity, never a guessed URI regex alone. Original PR/source paths remain canonical outside the private workspace. Override queue next/previous only for active review buffers; let existing bindings continue elsewhere.

Alternative: make Diffview's native file list represent the semantic order. Rejected because ranking is per hunk/symbol and the TUI already owns ordering. The adapter only needs to open a selected item accurately, not replace Diffview's model.

### 8. Reuse code-review.nvim UI; keep canonical anchors outside it

Use the existing plugin for prompt editing and indicators. The installed implementation permits only memory/file storage selection, not a custom public backend. Its public `add_comment` gets context from the current buffer; internal state methods handle add/update/delete. Therefore ship a small, version-checked bridge around those state operations for active review buffers, plus a dedicated file-storage projection. Pin and contract-test the supported plugin commit. Do not pretend this is a stable public extension API.

The bridge registers each review buffer's normalized plugin name with canonical snapshot/path/side identity. When the existing add-comment action saves, resolve its captured file/range through that registry, persist a canonical draft using the CLI, and record the plugin ID mapping. This avoids inferring the source from the currently focused floating comment editor. Store plugin Markdown under a session directory, activate its persistence explicitly at setup, and scope one active attached session per Neovim instance. Preserve unrelated existing plugin state and restore normal behavior when detached. Unsupported versions fail with a diagnostic rather than silently dropping anchors.

The pinned plugin already writes two useful formats: per-thread Markdown with known frontmatter (`file`, `line_start`, `line_end`, `time`, `author`, `thread_id`) and compact `path:Lstart-end: body` exports. Reuse these as plugin projections/display examples, but keep full anchors in SQLite: its parser discards unknown fields and some reply metadata does not round-trip. The bridge must preserve raw incompatible edits and flag a conflict. GitHub publication uses canonical root drafts, not a reverse parse of the plugin's compact export.

The portable human document uses a familiar location heading and freeform body. Proposed examples:

```markdown
# Review

<!-- lgr-review: {"schema":1,"session":"S","snapshot":"SN","export":"E"} -->

## backend/src/common/http-server-timeouts.ts:12-18 (RIGHT)
<!-- lgr-comment: {"id":"C1","base_revision":3,"anchor":"A1"} -->
Please explain why the headers timeout is larger than the keep-alive timeout.

## frontend/src/config.ts:8 (LEFT)
<!-- lgr-comment: {"id":"C2","base_revision":1,"anchor":"A2"} -->
Is this removed fallback still needed for a relative API URL?

## General feedback

Please review the timeout changes before the rendering cleanup.
```

Export an adjacent `review.anchors.json` containing referenced anchor IDs, full source coordinates/digests, and base body revisions; JSON comments in Markdown contain only stable exchange identifiers. A reviewer or LLM can read the Markdown alone. Round-trip import uses both files for precise updates; missing sidecars preserve text as unresolved rather than guessing inline positions. New location sections can obtain an anchor by exact match against the exported snapshot, with ambiguity reported. Deleting a section is not a delete command. Compare the current body revision with the export revision to detect concurrent changes.

Alternative: add custom fields to the plugin's Markdown and treat it as authoritative. Rejected because rewrites discard those fields. Alternative: replace the comment plugin. Rejected because preserving the user's `<leader>rc` workflow and minimizing UI code is a core preference.

### 9. Publish via GitHub CLI authentication with explicit review intent

Use an adapter invoking the configured GitHub CLI executable with structured arguments and JSON request files. A versioned user-owned profile file maps `host/owner/repository` globs to an account-bound CLI command and expected login. Resolution is automatic and fails closed on no match or multiple matches; an explicit named-profile override resolves intentional overlap. Commands are argv arrays, profiles store no token, public defaults contain no user identity, and the tool never switches global GitHub authentication or rewrites global Git configuration. An optional canonical API host supports repository patterns that use local SSH host aliases. Keep repository host/base owner/PR identity independent of head-fork identity. Import metadata/comments with pagination and remote IDs; mark outdated imported locations without forcing them onto current hunks.

Preview records selected draft IDs/body revisions, original PR head SHA, event, summary, and converted inline coordinates. Map LEFT to before-side diff lines and RIGHT to after-side lines, including valid multiline ranges. For renames, translate the retained old/new paths to the GitHub PR diff entry's API path rather than blindly submitting the old filename. Re-fetch the PR immediately before submission and reject a changed head or changed drafts. Submit against the recorded commit ID. A PR update racing the request can still make a correctly submitted review outdated; retain its actual reviewed commit and surface that state rather than claiming the current head was reviewed.

Persist submission intent and payload digest before sending one new-review request. GitHub does not provide a general exactly-once guarantee here: a timeout enters `uncertain`, followed by read-only reconciliation against author, commit, event, remote body/comment IDs/content, and attempt time. If matches are absent or ambiguous, do not automatically retry. Acknowledged remote IDs prevent ordinary repeated submission. Errors retain local drafts; invalid anchors never silently fall back to top-level comments.

Existing Octo remains available to the user for full GitHub thread management. Using its private review buffers as a publisher would couple two independent queue models and more plugin internals, so the first release uses the existing CLI authentication and GitHub review API directly.

For `gh-dash.nvim`, expose a generic `github-profile exec` adapter. It derives host/repository identity from an explicitly selected local Git remote, resolves exactly one account-bound profile, verifies the expected login, and replaces itself with the structured GitHub command plus extension arguments. A small Lua toggle remembers the Git root and closes a hidden dashboard before opening from another repository, preventing a process authenticated for one repository class from being reused in another. Neither layer invokes `gh auth switch` or falls back to an unmatched default account.

### 10. Context hooks are bounded user-configured inputs

Support Markdown files, inline notes, and GitHub metadata first. An optional pre-review hook is an executable/argv array from user-owned configuration; it receives session/repository/PR JSON on stdin and returns Markdown on stdout. Default timeout is 30 seconds and output limit is 256 KiB. Persist output, provenance, digest, and failure status. Hook failure requires retry or an explicit continue-without-context option; refresh does not erase earlier context.

Do not discover executable hooks from a reviewed PR. Do not execute Markdown or follow URLs automatically. The skill treats all fetched text as evidence, not instructions. Tickets requiring a particular vendor API can be supplied by the user's hook without first-release connector implementations.

### 11. One release gate with Inker and focused fixtures

Implementation adds meaningful contract tests for all seven specs, using temporary Git repositories, deterministic LSP fixtures, and mocked GitHub responses. Qualify the actual TypeScript, HTML, and CSS servers and pinned Neovim plugins separately; mock-server tests alone do not prove editor/language compatibility.

Use Inker's pinned 37-file renderer comparison as the real-repository cold-index case and record repository commit IDs, hardware, dependency/config fingerprints, server versions, node/edge counts, coverage, elapsed time, and peak memory. On the user's Apple Silicon host, target at most 180 seconds from available source snapshot to initial graph, at most 30 seconds for the same cached graph, and under 500 ms for bounded cached metadata/queue queries. Dependency downloads, model execution, and remote PR retrieval are reported separately. A fast result missing the required fixture symbols/test references does not pass.

Add isolated frontend/backend test fixtures based on Inker's configuration patterns to cover TSX, HTML, CSS, excluded tests, config aliases, renames, deletions, Unicode, and a hunk spanning multiple symbols. Do not claim the historical backend-heavy comparison proves those independent paths. Keep Inker's tracked files, index, and existing edits unchanged; do not check its data or credentials into fixtures.

Replay a deterministic assessment to verify exact score inheritance, tags, overrides, pagination, and total hunk accounting. Separately run an actual external ranking agent with the shipped skill on Inker, recording model/runner, elapsed time, query/byte totals, completeness, and its explanations. Human acceptance confirms a consequential renderer/timeout logic change can precede mechanical changes and that the important test references are reachable. No provider-price or broad ranking-accuracy claim follows from one run.

Prove TUI → selected Diffview hunk → `<leader>rc` → persistent draft → Markdown body edit/import → GitHub preview. A real submission against a designated test PR is a release gate only when explicitly authorized during implementation; API fixtures and a local preview must not be called live GitHub proof. All components ship together after the full gate passes.

## Risks / Trade-offs

- LSP is incomplete or project-dependent → expose coverage and provenance, qualify real servers, retain all raw changes, and test excluded-test discovery explicitly.
- Before/after indexing and private sources cost disk/time → content-address source, reuse validated graph results, bound expansion, and measure the actual Inker fixture.
- Dependency/configuration mismatches weaken historical semantics → fingerprint and report them; never claim reference completeness from the current checkout's dependencies.
- Small ranking models misjudge importance → keep evidence, confidence, tags, manual overrides, and unassessed items visible; ranking cannot hide changes or publish findings.
- Existing Neovim plugins lack stable anchor hooks → isolate and pin the small adapters with real-buffer compatibility tests; fail clearly on unsupported revisions.
- Three editing surfaces can diverge → one canonical store, explicit exchange IDs/revisions, idempotent import, and conflict preservation.
- A PR can change or an API response can be lost → source-bound previews, last-moment head checks, submission journaling, and no blind mutation retries.
- A compiled application still depends on LSP/Neovim runtimes → package the binary/bridge/skill together and provide precise dependency diagnostics instead of claiming a fully self-contained language toolchain.

## Migration Plan

This is a new application; no existing repository data migration is required. During implementation, create versioned session storage with transactional migrations and backups before upgrades. Distribute binaries, skill, and Lua bridge together with tested dependency/plugin versions. Neovim integration is an explicit configuration addition; never rewrite the user's dotfiles during install.

Rollout publishes `0.1.0` source and supported-platform archives only after licence/package inspection, clean-install documentation acceptance, full local and CI gates, disposable fixture sessions, read-only Inker acceptance, and an explicitly authorized GitHub test PR. Each archive carries checksums, EUPL-1.2 text, third-party notices, setup guides, helper commands, skill, and Neovim bridge. Rollback removes/disables the bridge and returns to existing Neovim behavior; exported Markdown and versioned session backups remain readable. Cache cleanup must target only identified tool-owned snapshot directories and must not delete draft stores by default.
