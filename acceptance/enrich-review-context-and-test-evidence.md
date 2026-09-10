# Enriched review acceptance

Host: Apple Silicon macOS, 2026-09-09. Change: `enrich-review-context-and-test-evidence`.

## Outcome

The enriched review workflow is complete. Large raw hunks can be projected into stable review units; captured code previews support syntax highlighting and annotations; code/context and session views expose PR, Markdown, Folio, explanation, draft, and test evidence; runtime coverage links unchanged tests without adding them to completion totals.

The installed `~/.local/bin/lgr` and release artifact both have SHA-256 `61184360164915ca5bd932284b8836d9810bb6abb165b7e65366e5a9ce82ae46` and report `lgr 0.1.0`.

## Verification

| Check | Evidence | Result |
| --- | --- | --- |
| Rust formatting | `cargo fmt --all -- --check` | Pass |
| Rust lint | `cargo clippy --all-targets --all-features -- -D warnings` | Pass |
| Rust suites | `cargo test --all-targets --all-features`: 107 passed, 1 ignored, 14 suites | Pass |
| OpenSpec | `openspec validate --all --strict`: 3 changes valid | Pass |
| Neovim | async launch, popup lifecycle/focus, Diffview `gq`, installed Diffview-to-ranked-popup, and `<leader>rc` comment fixtures | Pass |
| Installed consumer path | Final installed binary attached to a captured session, opened the exact Diffview target, returned with `gq`, launched the ranked TUI, and closed cleanly | Pass |
| Data safety | Partition revision apply/rollback, legacy migration, backup restoration, stale-writer rejection, and empty-projection raw graph compatibility | Pass |

The installed acceptance session used an active projection with 369 review units. Its captured graph contained 503 nodes and 402 edges. The flow did not read review content from the working tree.

## Representative enriched review

The integration fixtures cover the full workflow as one captured-review scenario:

- A 300-line addition is split deterministically on semantic boundaries, with bounded fallback chunks and exactly one owner for every changed row.
- Changed TypeScript production code links through `runtime_test` evidence to an unchanged failed test. Reverse traversal works and the unchanged test does not enter review progress totals.
- Local Markdown, a Folio export, a PR-context fixture, a local draft, an LLM explanation, a manual note, a range annotation, and a cited evidence ID remain distinct and address the same captured item.
- `i` switches between syntax-highlighted Code and Context without writing progress; `I` opens session context. Full-parent preview and related-item navigation preserve pane mode, selection, and offsets.
- Renamed and deleted targets retain LEFT-side paths and lines. Diffview and Markdown comments resolve the exact captured side and range.
- Explanation launch is explicit. Cache hits do not launch an agent; `--force` does. Test preparation and execution happen only through `lgr tests run`.

The real Jest file-isolation recipe ran two test files independently. `add.test.js` covered the addition branch (line 2 hit, line 6 not hit); `subtract.test.js` covered the subtraction branch (line 6 hit, line 2 not hit). Both retained file-level provenance. Failed outcomes are retained rather than discarded.

## Performance

Release benchmark, 10,000 nodes, 10,000 edges, 100 active review leaves, 200 mixed navigation/render actions:

```text
host=macos-aarch64 nodes=10000 edges=10000 leaf_units=100 actions=200 plain_cold_ms=211.577 highlight_cold_ms=235.761 highlight_warm_ms=0.098 median_ms=0.098 p95_ms=0.158 peak_rss_kib=47616 io=0 child_processes=0 cache=cold+warm
```

All design budgets passed. Warm syntax rendering stays cached; navigation performs no filesystem IO and launches no child process.

## Requirement-to-test evidence

| Requirement area | Primary evidence |
| --- | --- |
| Review-unit compatibility and partitioning | `review_units` unit tests; `enrichment_cli::partition_migrates_ranking_and_progress_without_losing_comments`; cache workflow regression |
| Syntax grammars and terminal rendering | `syntax` grammar/projection tests; TUI 80x24, 100x30, disabled, no-color, and large-file fixtures |
| Runtime test ingestion and linking | `test_evidence` parser/process/cache tests; `enrichment_cli::context_test_evidence_and_explanations_share_exact_captured_ids`; real Jest example |
| PR, Markdown, and Folio context | `context` compatibility/bounds/inert-input tests; GitHub recorded adapter fixtures; enrichment CLI scenario |
| Explanations, notes, and annotations | `explanations` validation/staleness tests; enrichment CLI cache/force/batch scenario; populated TUI context fixture |
| Keyboard and Neovim flow | affected headless Lua fixtures, including the final installed-binary flow |
| Performance | ignored release test `benchmark_large_cached_navigation`, run explicitly with `--release --ignored --nocapture` |
| CLI UX | [enrichment CLI audit](semantic-review-ux-cli-audit.md) |

Foundation requirements unrelated to this change remain unchanged: live authorized GitHub publication and the foundation release gate are still pending and are not claimed by this acceptance.
