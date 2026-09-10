## 1. Semantic titles and compatibility

- [x] 1.1 Add bounded optional titles and canonical hunk-label storage in `src/ranking.rs`, preserving existing queue fields; verify old serialized rankings load, untitled score clients still work, and labeling alone does not assess an unranked hunk.
- [x] 1.2 Add title/fallback projection and provenance to queue output; verify distinct hunks in one symbol retain separate labels, deterministic fallback ranges, unchanged ordering, and complete accounting.
- [x] 1.3 Implement revision-checked atomic `graph label` batches and title support in score batches; verify invalid/control-character titles reject the batch and concurrent label/score writes preserve manual overrides and completed work.
- [x] 1.4 Add explicit `rank --titles-only` with default missing-title selection and force refresh, plus bundled prompt/reference/example updates; verify a fake external agent and the executable skill example enrich titles without changing scores, tags, rationales, finalized state, graph caches, or drafts.

## 2. Related-hunk projection

- [x] 2.1 Implement a focused side/path/range index and bounded hunk-to-hunk projection over semantic edges, including reverse test-to-implementation mapping; verify direct range matches, smallest-enclosing-symbol context, additions, deletions, renames, and no shared-file-only links.
- [x] 2.2 Aggregate categories/evidence by target hunk, exclude self-links, and sort deterministically by relation then importance; verify multiple references and before/after routes produce one row without losing supporting evidence.
- [x] 2.3 Retain secondary unchanged-source context and raw evidence inspection; verify targets without changed hunks have usable symbol/side/line labels and never affect completion, while partial coverage does not claim absent tests.

## 3. Preview data and navigation state

- [x] 3.1 Supply snapshot patches through the CLI/TUI adapter and implement cached preview parsing in a dedicated module; verify old/new coordinates and exact captured contents for modified, added, deleted, renamed, non-text, and edited-after-capture inputs.
- [x] 3.2 Add bounded asynchronous unchanged-source loading and preview chunk/cache limits from the design; verify large/long-line patches, safe control-byte display, stale-result suppression, explicit continuation, and recoverable source failure.
- [x] 3.3 Replace raw-neighbor default navigation with queue/related view frames, `l` follow, `h` Back, and compatible `g`; verify recursive traversal restores selection, list/preview offsets, filters, and review counts without opening an editor.
- [x] 3.4 Add list/preview focus, paging, preview scroll keys, and stateful viewport tracking; verify 100-item navigation, narrow resize, first-change positioning, and no automatic reviewed status.
- [x] 3.5 Persist only meaningful durable state changes, with clean exit/signal handling and contextual help; verify navigation-only actions cause no progress writes, saved status survives Ctrl+C, terminal mode is restored, and existing comment/export/manual-priority paths still work.

## 4. Readable presentation and filters

- [x] 4.1 Build title-first queue and grouped related-hunk rows in focused renderer modules with one compact location and Unicode display-width clipping; verify rendered output distinguishes repeated-path hunks and duplicate basenames at 80x24, 100x30, and 120x40.
- [x] 4.2 Replace metadata-only details with a focused diff preview and compact rationale/location context; verify at least ten code lines at 100x30 and six at 80x24, visible selection and scrolling, and a recoverable minimum-size layout.
- [x] 4.3 Implement searchable tag multi-select with counts, OR semantics, status AND semantics, apply/cancel/All tags, and empty-filter recovery; verify modal Enter/Space cannot open or review a hunk and popup resume preserves applied filters.
- [x] 4.4 Add centralized semantic palettes, bundled Night Owl/Tokyo Night/Catppuccin/terminal presets, a `T` selector, custom `tui.themes` JSON declarations, and no-color/indexed-terminal behavior; verify configuration compatibility, live selection, `NO_COLOR`, `--no-color`, truecolor/indexed/monochrome renderings, and distinguishable focus/additions/deletions without color.
- [x] 4.5 Gate TUI startup for non-TTY and dumb terminals with actionable machine-readable errors; verify redirected stdin/stdout returns promptly, no raw-mode escape output enters JSON pipelines, and existing help/version/error-envelope contracts remain intact.

## 5. Neovim consumer path

- [x] 5.1 Add configurable Diffview `gq` integration for view and file-panel help entries; verify isolated Neovim fixtures cover both sides, help discoverability, custom-map preservation, and unchanged mappings outside Diffview.
- [x] 5.2 Update popup footer/focus handling for the new key model and preserve existing terminal/session state; verify repeated gq returns to the same job with filters, selected hunk, preview offset, and immediate keyboard input.
- [x] 5.3 Update `docs/neovim.md`, README, and the user's installed Diffview setup where needed using normal scoped approval; verify actual loaded configuration resolves to the new bridge and gq is shown in Diffview help without requiring a leader key.

## 6. Complete-flow acceptance and CLI audit

- [x] 6.1 Run `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test --all-targets --all-features`, plus affected Neovim acceptance fixtures; require zero exit statuses before proceeding to installation.
- [x] 6.2 Exercise a representative captured review in a real terminal: open queue, read/scroll a hunk, pick tags, follow a changed test and usage/definition, Back, mark reviewed, open exact Diffview side, return with gq, hide/resume, export/comment, and quit/reopen; record observable results and rendered evidence rather than treating unit tests as consumer proof.
- [x] 6.3 Measure startup, first preview, and 200 cached select/follow/back actions using a prebuilt fixture with at least 10,000 nodes, 10,000 edges, and 100 hunks; record host, median/p95, cache state, and IO/process counts, and meet the design's 1s/100ms/50ms limits.
- [x] 6.4 After implementation and flow testing, apply `nodejs-cli-best-practices` to all 41 practices and deliver `acceptance/semantic-review-ux-cli-audit.md` with status counts, per-section evidence, justified Rust equivalents/exceptions, and prioritized recommendations with concrete code fixes for failures; resolve in-scope high-impact issues and explicitly record unrelated follow-ups.
- [x] 6.5 Rebuild/install via `./scripts/install.sh` with normal scoped approval, then repeat the installed Neovim queue → related hunk → preview → Diffview → gq path and verify old finalized sessions open without automatic reranking; record binary/bridge provenance and any restart needed.
- [x] 6.6 Run strict OpenSpec validation and add the UX acceptance evidence to project documentation; verify both delta specs map to completed tasks while leaving the foundation's live-GitHub and full-release gates pending.
