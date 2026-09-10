## 1. Rust application and session contracts

- [x] 1.1 Create the single-package Rust library/binary structure and pin the toolchain/dependencies described in design decision 1; verify `cargo check --all-targets` and `lgr --help` succeed.
- [x] 1.2 Define versioned JSON envelopes, typed session/graph/context IDs, structured errors, stdout/stderr behavior, and exit codes; verify CLI contract tests parse success and failure output without log contamination.
- [x] 1.3 Implement per-user session storage and transactional SQLite schema/migrations with backups; verify reopen, repository isolation, and migration rollback tests preserve drafts and progress.
- [x] 1.4 Implement revision-checked atomic writes and bounded contention handling; verify competing CLI updates reject the stale writer without partial changes.
- [x] 1.5 Add dependency diagnostics for Git, LSP executables/runtimes, Neovim/plugins, and the GitHub adapter; verify missing dependencies disable only the relevant capabilities and produce specific setup guidance.
- [x] 1.6 Add versioned `~/.lgr/settings.json` with default shared data/scripts directories, generic named agent argv, persistent selection, and CLI overrides; verify initialization preserves existing configuration, configuration never launches an agent, and review consumers work without `--data-dir`.
- [x] 1.7 Separate ad-hoc agents from named agent profiles, migrate version-one harness settings, and launch ranking with embedded skill injection; verify `-a opencode`, `-p work`, scripts-directory resolution, dry-run, and no shell interpolation.
- [x] 1.8 Keep shipped defaults, documentation, fixtures, and metadata free of developer identities, machine paths, and account-specific wrappers; verify fresh settings are neutral while existing user-owned settings remain compatible.
- [x] 1.9 Add repository-scoped recent session discovery with lifecycle and review counts; verify ready sessions are identifiable without knowing an opaque session ID.

## 2. Immutable Git snapshots

- [x] 2.1 Implement local merge-base branch reviews and explicit direct revision comparisons; verify a divergent-branches fixture excludes unrelated target-branch changes and records exact IDs.
- [x] 2.2 Implement HEAD-to-index and index-to-working-tree capture with explicit untracked inclusion; verify one file with mixed staged/unstaged edits appears correctly in separate snapshots.
- [x] 2.3 Implement capture consistency checks and private source/object storage without checkout mutation; verify concurrent edit failure/retry and dirty-source preservation with before/after content and index fingerprints.
- [x] 2.4 Parse complete change inventories and hunks with old/new paths/ranges; verify additions, deletions, renames, mode-only changes, binary files, submodules, unusual filenames, and unsupported conflict handling.
- [x] 2.5 Materialize isolated before/after workspaces and private visualization commits for non-commit inputs; verify each materialized blob matches capture and the source Git refs/hooks remain untouched.
- [x] 2.6 Implement session resume and explicit refresh with unchanged-content matching; verify changed hunks reset review status and ambiguous old anchors remain visible without automatic reassignment.
- [x] 2.7 Accept Diffview-style `A..B` direct and `A...B` merge-base review inputs alongside explicit flags; verify resolved endpoint/comparison identities and malformed-range errors.
- [x] 2.8 Add one all-uncommitted input comparing HEAD to staged, unstaged, and untracked working-tree content; verify complete inventory and unchanged source/index state.
- [x] 2.9 Add opt-in reusable review creation keyed by exact selected Git content and resolved revisions; verify unchanged input reuses one session while a second edit with unchanged Git status invalidates it.

## 3. Language-server clients and graph assembly

- [x] 3.1 Integrate the LSP client framework and process lifecycle, capability negotiation, configuration callbacks, progress, cancellation, limits, and rejection of workspace edits; verify a deterministic server fixture covers timeout/crash/unsupported-method behavior.
- [x] 3.2 Add TypeScript/JavaScript/TSX/JSX, HTML, and CSS server profiles with multi-root configuration and version recording; verify actual configured servers initialize and expose expected document structure on qualified fixtures.
- [x] 3.3 Discover Inker-style separate frontend/backend projects and preserve relevant compiler settings in isolated before/after roots; verify aliases, JSX, decorators, and project identity against project-local symbol/reference assertions.
- [x] 3.4 Normalize document symbols and hunk overlap into snapshot-bound graph nodes; verify nested/multiple symbols, unmatched hunks, removed symbols, and before/after counterparts without conflating identities.
- [x] 3.5 Extract bounded references, definitions, and supported incoming/outgoing calls with typed evidence; verify side/direction/provenance against fixtures and record unsupported HTML/CSS relations explicitly.
- [x] 3.6 Implement position-encoding conversion using captured source; verify UTF-16, emoji, multibyte text, CRLF, empty ranges, and deletion coordinates match Git and editor locations.
- [x] 3.7 Add explicit test discovery and `test_reference` classification, including isolated test-aware project configuration where needed; verify resolved test imports for both Bun/Vitest patterns excluded by application configs and distinguish naming-only associations.
- [x] 3.8 Implement graph coverage, unfinished frontiers, cancellation, bounded continuation, and fingerprinted caching; verify raw-change inventory survives partial indexing and source/config/server/dependency changes invalidate affected cached results.
- [x] 3.9 Expose graph cache hits and force bypass through the CLI; verify repeated unchanged builds retain one revision without relaunching language servers.

## 4. Context and pre-review hooks

- [x] 4.1 Implement Markdown files/inline notes, provenance, digesting, limits, and context retrieval; verify local review works offline and changing a document marks prior ranking stale.
- [x] 4.2 Implement the explicitly configured executable/argv hook with JSON stdin, Markdown stdout, timeout/output limits, and recorded failures; verify successful output, failure, retry, and explicit continue-without-context behavior.
- [x] 4.3 Enforce context trust boundaries and server-plugin configuration policy; verify PR text cannot enable hooks, Markdown/link ingestion runs no executable, and unconfigured project-provided LSP plugins are not activated.

## 5. Agent graph API and ranking

- [x] 5.1 Implement overview/inventory, field projection, and batch node lookup; verify every captured change is discoverable and missing IDs return per-item errors within response budgets.
- [x] 5.2 Implement multi-seed, typed/directional graph traversal with bounded recursion and revision-bound continuation; verify cycles/shared nodes, global batch limits, cancellation, and exact pagination coverage.
- [x] 5.3 Implement selected hunk/source retrieval and batch read operations with independent source pagination; verify byte budgets, oversized hunks, old-side content, and absence of silent truncation.
- [x] 5.4 Implement explicit graph expansion publishing a new revision; verify ordinary read queries launch no LSP work and expansion makes older finalized rankings visibly stale.
- [x] 5.5 Implement atomic score/tag/evidence batches and manual overrides; verify invalid records reject the whole batch and unknown evidence, invalid scores, and stale revisions return actionable errors.
- [x] 5.6 Implement deterministic hunk queue projection, symbol inheritance, tags/filters, unranked items, and non-text change items; verify mixed-risk hunks in one file sort independently and every change counts once.
- [x] 5.7 Implement ranking-run metrics, resumption, finalization, and reviewer handoff; verify graph/context revisions are bound, inherited versus directly assessed coverage is reported, and incomplete runs cannot claim full assessment.
- [x] 5.8 Create the distributable ranking skill with overview-first batching, bounded neighborhoods, selective source reads, explicit budgets, and ranking-only authority; verify an executable scripted example completes against the CLI and the skill documents independent-agent/model handoff.
- [x] 5.9 Reuse finalized rankings bound to the current graph and context while retaining an explicit force rerun; verify cache hits start no agent and context changes invalidate them.

## 6. Canonical drafts and Markdown exchange

- [x] 6.1 Implement persistent comment IDs, snapshot/path/side/range/content anchors, body revisions, and plugin/remote ID mappings; verify LEFT/RIGHT, renamed/deleted files, restart, and orphaned-anchor behavior.
- [x] 6.2 Implement readable `review.md` export and the adjacent anchor metadata file using the documented format; verify exports contain ordered feedback/source context and can be read without the companion file by a reviewer.
- [x] 6.3 Implement body edits, new location sections, and unanchored general-feedback import; verify stable identity, exact-anchor matching, missing/damaged metadata errors, and idempotent reimport.
- [x] 6.4 Implement import conflict handling and explicit draft deletion; verify concurrent body edits preserve both versions and omitted Markdown sections do not silently delete drafts or remote comments.

## 7. Neovim and existing plugin bridge

- [x] 7.1 Implement the small Lua bridge and structured RPC/CLI transport for one attached session; verify paths containing spaces/quotes reach the CLI safely and disconnects preserve session state.
- [x] 7.2 Integrate Diffview revision-pair/path selection and buffer identity/range targeting; verify committed, staged, unstaged, LEFT-side deletions, and renamed files display captured content even after source checkout edits.
- [x] 7.3 Add version-checked `code-review.nvim` capture/edit/delete adapters and session-scoped persistence; verify the pinned plugin's normal/visual `<leader>rc` action produces canonical anchors rather than Diffview URIs.
- [x] 7.4 Preserve comment identity across plugin persistence/reload and external Markdown edits; verify unknown/lossy thread metadata is retained as a conflict and unsupported plugin versions fail without losing drafts.
- [x] 7.5 Add ranked next/previous and reviewed/unreviewed editor actions plus attach/detach handling; verify they share the CLI queue state, retain existing unrelated bindings/comments, and never mark viewed content reviewed automatically.
- [x] 7.6 Document minimal opt-in Neovim configuration and tested plugin commits; verify the recipe in an isolated Neovim configuration rather than modifying the user's dotfiles.
- [x] 7.7 Add a Git Dash launcher that restarts on Git-root changes and executes `gh dash` through directory-matched, identity-verified GitHub profiles; verify remote parsing, dry-run resolution, structured execution, and no global account switch.
- [x] 7.8 Add an asynchronous Neovim launch group for uncommitted, development-base, prompted-base, staged, and unstaged reviews; verify structured argv, serialized launches, session attach, and first-item navigation.
- [x] 7.9 Make Neovim launch actions opt into layered session, graph, and ranking reuse; verify repeated unchanged launches still attach and navigate without repeated expensive work.
- [x] 7.10 Add compact animated launch feedback for snapshot, graph, ranking, and opening stages with a statusline API; verify animation, transitions, overlap rejection, and cleanup after failure.
- [x] 7.11 Present the ranked queue after launch and open the full captured Diffview comparison before focusing a selected hunk; verify multi-file visibility, ranked labels, and path/line focus.
- [x] 7.12 Host the ranked TUI in a resumable Neovim terminal popup connected to the current RPC server and expose it in Diffview help; verify popup lifecycle, safe argv, same-editor navigation, and user mappings.

## 8. Lightweight terminal review queue

- [x] 8.1 Implement the Ratatui queue with current symbol/path, scores/tags, rationale access, coverage, and progress; verify rendering and keyboard tests preserve total counts under tag/status filtering.
- [x] 8.2 Implement manual priority edits, selection, review status, and resume; verify changes survive restart and appear in Neovim/CLI consumers with stale-write conflicts handled.
- [x] 8.3 Implement both attached-Neovim navigation and suspend/open/resume terminal mode; verify terminal state is restored on normal exit and editor failure, with unchanged selection/drafts after disconnection.
- [x] 8.4 Expose draft/export actions and recoverable missing-editor behavior from the queue; verify Markdown/source operations remain available when Diffview cannot open.
- [x] 8.5 Allow `lgr tui` to select the newest indexed session for the current repository while retaining explicit session selection; verify repository isolation and actionable empty-state failure.
- [x] 8.6 Add a queue-first in-memory graph explorer with typed/directed neighbors, recursive follow/back navigation, source opening, and left-elided queue paths; verify test/reference priority, selection preservation, no per-step external work, and readable narrow rendering.

## 9. GitHub input and review publication

- [x] 9.1 Implement the configured GitHub CLI adapter with explicit host/base-repository/fork identity, paginated PR/comment retrieval, and original base/head SHAs; verify mocked fork/multi-page PRs produce the correct snapshot/context without changing global authentication.
- [x] 9.2 Connect GitHub PR creation/refresh to snapshot and context capture; verify local comment IDs remain distinct from imported remote IDs and outdated comments retain original coordinates.
- [x] 9.3 Implement COMMENT/APPROVE/REQUEST_CHANGES preview with selected draft versions, summary, and GitHub diff-coordinate mapping; verify single/multiline, LEFT/RIGHT, rename API paths, and unsupported-anchor diagnostics.
- [x] 9.4 Implement explicit submission with expected-account verification, latest PR-head and draft-version checks, and recorded remote IDs; verify a configured account-bound command is used and stale previews or identity mismatches cause zero mutations.
- [x] 9.5 Implement publication intent journaling, acknowledged-submission deduplication, and uncertain-result reconciliation; verify a simulated accepted request with a lost response does not trigger a blind duplicate retry.
- [x] 9.6 Verify network, authentication, API validation, and permission failures retain all drafts and allow offline review/Markdown export; assert no implicit inline-to-summary fallback or automatic publication from ranking/editor actions.
- [x] 9.7 Add repository-matched, account-bound GitHub profiles with structured commands, optional canonical API hosts, and expected identities; verify multi-profile auto-resolution, explicit overrides, SSH host aliases, ambiguous/no-match failure, token-free persistence, and zero global authentication/config changes.

## 10. Complete workflow and performance acceptance

- [x] 10.1 Add a cross-component fixture proving snapshot → graph → ranked hunks → Neovim comment → Markdown edit/import → GitHub preview; verify original source/index fingerprints and full change accounting at the end.
- [x] 10.2 Run the real-server Inker benchmark at `475e161075c508ebe23fe64b69e263247cd869ba..154cb1ccb75d32e3387ccc914ebf0034503bd4cd`; verify required renderer/test relationships, cold graph within 180 seconds, cached graph within 30 seconds, and bounded cached metadata queries below 500 ms on the recorded Apple Silicon host.
- [x] 10.3 Qualify isolated mixed frontend/backend, TSX, HTML, CSS, and excluded-test fixtures separately from the historical Inker diff; verify expected symbols, honest unsupported relations, and typed resolved test references with real language servers.
- [x] 10.4 Run an external ranking agent using the shipped skill against the pinned Inker session; record runner/model, elapsed time, query/byte totals, coverage, scores/tags/rationales, and demonstrate a consequential logic change ahead of mechanical work without a provider integration in `lgr`.
- [x] 10.5 Perform interactive acceptance with the TUI controlling navigation and the existing `<leader>rc` comment experience; verify source side/range, shared progress, persistence after restart, and editable Markdown handoff.
- [x] 10.6 After explicit authorization for a designated test PR, submit a real COMMENT review through the complete flow and fetch it back; verify author, original commit, inline path/side/range, and absence of duplicates. Keep this gate pending if live publication is not authorized; mocks are not live proof.

## 11. Single-release packaging and checks

- [x] 11.1 Package macOS Apple Silicon and Linux x86_64 binaries with the skill/bridge, dependency versions, and setup instructions; verify clean-environment smoke runs on each supported platform.
- [x] 11.2 Document all input modes, context hooks, agent budgets, graph coverage limits, comment exchange, GitHub commands, session backup, and opt-in Neovim setup; verify documented examples against the delivered CLI.
- [x] 11.3 Run `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test --all-targets --all-features`; verify zero exits plus the real-server/editor acceptance evidence after final release changes, and resolve failures before marking the release ready.
- [ ] 11.4 Publish project-authored source and bundles as version `0.1.0` under EUPL-1.2: add canonical licence text and notice, update Cargo metadata and third-party notices, bundle both notice files, and verify version/licence identity inside each archive.
- [x] 11.5 Create an indexed, thorough setup path covering archive and source installation, prerequisites, agent configuration, Neovim/Diffview/code-review.nvim, gh-dash and GitHub profiles, `lgr doctor`, data backup, upgrades, uninstall, and troubleshooting; verify required and optional steps from a clean isolated configuration.
- [x] 11.6 Make CI and release packaging enforce formatting, clippy, all-feature Rust suites, real language-server qualification, headless Neovim acceptance, archive smoke tests, and checksums on both supported targets; verify release publication cannot bypass successful checks.
- [ ] 11.7 Produce the complete `0.1.0` release acceptance record mapping all seven capability specs plus licence, documentation, package, and CI requirements to passing checks and measured limitations; verify all milestones, including authorized live GitHub proof, are complete before calling the release delivered.
