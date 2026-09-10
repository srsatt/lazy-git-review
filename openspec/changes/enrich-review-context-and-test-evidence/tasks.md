## 1. Review-unit contracts and compatibility

- [x] 1.1 Add versioned review-unit metadata and additive settings defaults; verify old session/settings fixtures load unchanged and raw hunk APIs retain their IDs and output contracts.
- [x] 1.2 Implement partitioning using captured symbol/syntax boundaries and bounded patch-row fallback; verify a 300-line addition, oversized function, replacement, deletion, rename, metadata-only change, CRLF, and Unicode fixtures own every changed row exactly once with stable IDs.
- [x] 1.3 Add partition preview/apply and graph unit inspection with projection revision checks and backups; verify explicit legacy migration, deterministic reuse, stale-writer rejection, and rollback without lost drafts.
- [x] 1.4 Extend ranking, external-agent unit reads/writes, and progress to active leaf units; verify inherited versus explicit assessments, per-unit titles, parent all-children completion, filtered denominators, and ambiguous repartition handling.
- [x] 1.5 Resolve chunk ranges through comments, Markdown export, source retrieval, and Neovim editor targets; verify preserved old comments and exact renamed/deleted-side anchors in adapter fixtures.

## 2. Captured syntax highlighting

- [x] 2.1 Add pinned compatible Tree-sitter/highlight grammars and license attribution for TS/TSX, JS/JSX, HTML, and CSS; verify each grammar and its queries load against the supported Rust build.
- [x] 2.2 Implement bounded background captured-side tokenization and byte-to-display span projection; verify multiline context, tabs, control bytes, emoji, CRLF, stale-result cancellation, and no working-tree reads.
- [x] 2.3 Add optional syntax theme roles and highlighting settings; verify all existing custom palettes load, live theme switching preserves token data, and no-color/disabled/unsupported-language fallbacks render usable previews.
- [x] 2.4 Compose syntax, diff state, selection, and annotation emphasis in the preview; verify readable 80x24 and 100x30 terminal fixtures with added/deleted annotated code and bounded large-file behavior.

## 3. Runtime test evidence

- [x] 3.1 Define test-profile settings and versioned normalized run/report manifests separate from LSP CoverageEntry; verify case/file/suite attribution, statuses, fingerprints, and old-settings compatibility with serialization fixtures.
- [x] 3.2 Implement bounded Istanbul JSON and LCOV imports plus attributed manifest ingestion; verify source-map/original-hash checks, malformed/oversized inputs, missing provenance, and aggregate reports that create no invented test links.
- [x] 3.3 Implement explicit test dry-run/run/show commands using disposable captured content, configured preparation, bounded output, and process-group cancellation; verify source/index/snapshot preservation, dependency errors, timeout, failure, and cancellation with child-process fixtures.
- [x] 3.4 Add completed-run caching and force behavior; verify changes to side, source, runner, test selection, configuration, or dependency fingerprint invalidate reuse while ordinary browsing never executes tests.
- [x] 3.5 Deliver a real Jest test-file isolation recipe emitting separate coverage reports and manifests; verify two test files exercise distinct production ranges, accurately retain file-level attribution and failed outcomes, and do not assert individual-case coverage.
- [x] 3.6 Add runtime relation projection and unchanged test navigation with static/runtime evidence aggregation; verify production-to-unchanged-test and reverse traversal, duplicate elimination, left/right handling, and unchanged completion totals.

## 4. Context and explanation data

- [x] 4.1 Extend context storage with stable source keys, authors, thread/anchor metadata, and captured revisions; verify idempotent imports, stale intent behavior, partial refresh retention, and old ContextBundle compatibility.
- [x] 4.2 Normalize captured PR title/body/review summaries/comments and add missing thread metadata retrieval through the existing profile adapter; verify pagination, resolved/outdated state, exact anchor mapping, and unmatched session-level context using recorded API fixtures.
- [x] 4.3 Support local Markdown and explicitly supplied Folio exports through bounded ingestion and optional configured hooks; verify offline operation, citations to captured excerpts, truncation, and inert Markdown containing command-like text.
- [x] 4.4 Add separately versioned explanations and range annotations with atomic batch CLI updates; verify text/evidence/range limits, stale revision rejection, manual-note preservation, and unchanged ranking/finalization/drafts.
- [x] 4.5 Add explicit external-agent explain mode and update the embedded ranking instructions for optional explanations/unit evidence; verify fake-agent batches, dry-run argv/profile routing, bounded reads, force/cache behavior, and citation validation without automatic launches.

## 5. Keyboard and editor experience

- [x] 5.1 Implement i Code/Context and I session overview with optional populated sections and source excerpts; verify independent scroll state, focus, empty-context recovery, and no progress writes during toggling.
- [x] 5.2 Integrate explanations, manual comments, annotation markers, source citations, and test outcomes for raw hunks and chunks; verify stale/source/model distinctions and recoverable unavailable data in rendered fixtures.
- [x] 5.3 Preserve pane modes, related targets, filters, and offsets through follow/Back, full-parent preview, and popup hide/resume; verify headless Neovim gq and comment shortcuts plus immediate terminal keyboard focus.
- [x] 5.4 Update CLI help, TUI contextual help, README, Neovim docs, and settings/coverage examples; verify documented commands against help and executable fixture examples without personal repository/account data.

## 6. Integrated acceptance and delivery

- [x] 6.1 Run `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --all-targets --all-features`, and all affected Neovim acceptance fixtures; record zero exit statuses and meaningful regression results.
- [x] 6.2 Exercise a complete captured review with a 300-line addition, changed production code, unchanged tests, a PR thread, local draft, and Folio export; record code highlighting, i toggle, coverage-linked tests, chunk review/comment, exact Diffview opening, gq return, and quit/reopen evidence.
- [x] 6.3 Extend the release benchmark to 10k nodes/10k edges/100 leaf units and 200 mixed actions; record host, cold/warm cache, memory, first-frame/plain/highlight timings, cached p95, and IO/process counts against design budgets.
- [x] 6.4 Reapply the CLI UX audit to new commands and asynchronous states; deliver findings with in-scope fixes verified and unrelated recommendations separated in the acceptance document.
- [x] 6.5 Install through the existing script with normal scoped approval and repeat the installed Neovim review flow; verify installed binary/bridge provenance, old finalized-session behavior, and migration backup/recovery.
- [x] 6.6 Reconcile the foundation hunk-unit and redesign relation contracts during spec synchronization, preserve their other requirements, and validate this change strictly; record requirement-to-test evidence and leave unrelated live-GitHub/release gates unchanged.
