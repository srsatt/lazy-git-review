## Context

See `proposal.md` for motivation and the dependency on the unarchived foundation change. Source inspection found:

- `src/ranking.rs`: `Assessment` has rationale but no semantic title; `QueueItem.name` is copied from graph nodes, so hunks display Git headers.
- `src/tui/graph.rs`: adjacency expands hunks through symbols to raw reference nodes. `src/graph.rs` names reference nodes with the path alone. Distinct source locations therefore render as identical rows.
- `src/tui.rs`: both views reserve five rows including borders for metadata; no patch is supplied. `t` cycles tags, lists use manual selection styling without stateful viewport tracking, and queue key handling persists progress even for presentation-only keys.
- `src/snapshot.rs`: captured hunks already contain patches and before/after coordinates. `src/cli.rs` owns session loading and is the natural adapter for supplying snapshot-backed preview data.
- `nvim/lua/lazy-git-review/init.lua`: a reusable terminal popup and exact Diffview targeting already exist. The popup has a 120-column/40-row cap; shortcut integration is currently documented through Diffview setup rather than a shipped `gq` mapping.
- `src/main.rs` emits versioned JSON envelopes, including failures. Preserve this existing automation contract while auditing diagnostics and terminal behavior.

## Goals / Non-Goals

**Goals:** Make the first useful screen readable; let a reviewer understand, compare, and mark related changes without opening an editor; make latency and screen-level acceptance measurable; preserve snapshot identity and existing review data.

**Non-Goals:** A force-directed graph canvas, automatic bug findings, model calls during navigation, a rewrite of the LSP indexer, broad shell/CLI feature additions, or completion of the foundation's live-publication gate.

## Decisions

### 1. Titles describe changes; rationales explain priority

Add an optional bounded `title` to assessments and expose `title` plus `title_source` in queue output. Keep existing `name`, score, and rationale fields unchanged. New ranker instructions request a concrete verb phrase per hunk, for example “Stop retrying after terminal sync failure”; titles must not assert unverified bugs. Hunk identity remains the graph ID, never the title.

Use direct hunk titles. A symbol title is not silently copied onto several materially different hunks. Display fallback order: direct title; a single-line shortened existing rationale; symbol/change kind plus basename and range. Preserve title provenance so a fallback is not represented as a fresh model assessment. Missing titles do not invalidate graph caches or previously finalized scores.

Provide explicit title-only enrichment with `lgr rank SESSION --titles-only`, using the already configured external agent and bounded batches. A revision-checked `graph label` operation updates labels without changing scores, tags, manual authority, completion, or drafts. Keep a default-empty hunk-label map in ranking state so labeling an unassessed hunk never creates a score or marks it assessed; titles supplied with score batches update the same canonical map. Label and score writes use the existing atomic concurrency boundary so concurrent edits cannot overwrite each other. Existing `graph score` clients may omit titles. Reject blank/overlong supplied titles and unsafe control characters atomically; normalize only ordinary surrounding whitespace. Limit titles to 120 Unicode characters. Enrichment failure leaves fallbacks usable.

Alternative: use rationale as the permanent title. It requires no model-contract change but confuses change meaning with risk and frequently yields long sentences. Keep it solely as migration fallback.

### 2. Project graph evidence onto changed hunks

Build a review relation index once per loaded graph revision. Store compact relation records: target hunk ID, relation categories, direction, supporting edge IDs, intermediate symbol IDs, and mapping confidence. Use indexes by snapshot side and raw path bytes for symbol and hunk ranges.

For each source hunk, follow its overlapping symbols across a typed semantic edge (`test_reference`, `references`, `definition`, `calls`). Resolve the target location to directly overlapping changed hunks; if needed, resolve the smallest enclosing symbol and its changed hunks, labeling that route as symbol context. Also support the reverse direction so test hunks can return to production hunks. Bound the path to one semantic edge plus structural/range mapping; do not walk an entire transitive file graph. `counterpart` may normalize before/after symbol identity but does not imply an additional dependency.

Deduplicate by canonical target hunk ID, aggregating evidence and categories. Exclude self-links. A shared file, filename convention, or broad ancestor symbol alone does not establish a test/usage relationship. Prefer precise mappings over enclosing-symbol context. Sort by category (tests, usages/imports, definitions, calls, symbol context), then importance, then stable identity. Assign one primary group to each row and keep secondary categories in details. Keep direction relative to the current center.

Relations with no changed hunk are kept under a separately opened source-context section, with symbol, basename, side, line, and a captured source excerpt. They never count as review hunks. Raw edges remain available in an evidence inspector and the existing graph CLI. No indexed test relation means “No indexed related changes” with coverage context when relevant, never a claim that tests do not exist.

Alternative: deduplicate by filename. This hides separate changes and cannot support the requested hunk-to-hunk review. A graph canvas still exposes too much structure for the primary workflow.

### 3. A consistent queue / related changes / preview layout

Use the same compact row model in queue and related views: review marker, score, semantic title, and trailing basename:line. Allocate width to the title first. On wider screens show a parent path suffix only where it helps distinguish duplicate basenames. Never render the path twice. Unicode display-cell width, not character count, controls clipping; tabs/control bytes are rendered safely.

Related view retains the center title above grouped target hunks. A short breadcrumb makes backtracking legible; raw IDs and producer/confidence fields belong in the evidence inspector. A stateful list viewport follows selection through long lists and resize. The preview updates for the selected row without changing the center until `l` is pressed.

At 100x30 and larger, reserve at least ten visible code lines for the preview, in addition to compact title/location metadata. At 80x24 retain both a navigable list and at least six code lines; smaller terminals use an explicit compact layout and expose a minimum-size hint only when controls cannot fit. Patch preview includes old/new line numbers, `+`/`-` markers, added/deleted/context styling, scroll position, and the concise rationale where space permits. No full-width duplicate path paragraph. Non-text changes show their captured metadata rather than an empty patch.

Use `Tab` to move focus between list and preview, `j/k` for the focused pane, PageUp/PageDown for pages, and `[`/`]` for preview scrolling while the list retains focus. Start at the first changed line. Selection resets the preview to that hunk; Back restores the prior hunk, list offset, focus, and preview offset.

Illustrative content hierarchy (example data only):

```text
Review  1/32 reviewed                         Tags: behavior
   96  Stop retrying after terminal sync failure    editor.tsx:467
 > 88  Cover terminal failures and recovery         editor.test.tsx:683

Preview  editor.test.tsx  RIGHT  683–776
  683  + it('stops retrying after a terminal error', async () => {
       ... captured patch; ten or more visible code lines ...

l related   h back   t tags   Tab preview   o Diffview   ? help
```

### 4. Explicit key and filter behavior

| Surface | Key | Behavior |
| --- | --- | --- |
| Queue / related list | `l` or Right | Make selected hunk the center and show related changes |
| Related list | `h` or Left / Escape | Restore previous center; at root return to queue |
| Queue | `g` | Compatibility alias for opening related changes |
| Related list | `g` | Return to queue, preserving original queue selection |
| Queue / related list | Enter or `o` | Open selected captured hunk in Diffview |
| Queue / related list | Space | Toggle selected hunk's reviewed state |
| Queue / related list | `t` | Open tag picker |
| Any review view | `?` | Show contextual keys and close help with Escape |
| Diffview view / file panel | `gq` | Resume queue popup |

The tag picker is a modal multi-select with available tags and counts, including user-defined tags. Search narrows options; arrows move, Space toggles, Enter applies, Escape cancels, and an explicit “All tags” action clears selection. Multiple chosen tags use OR; the existing status filter combines with them using AND. Counts reflect the status-filtered collection before tag filtering. Apply restores the selected hunk if visible, otherwise selects the first result; empty results offer clear-filter recovery. Picker keystrokes cannot accidentally open Diffview or mark hunks reviewed.

Store navigation history as view frames, not only center IDs. Within the session, popup hide/resume preserves mode, selection, filters, and preview position. Durable review status remains keyed to hunks. View-only events do not write progress. `q` exits normally; Ctrl+C restores the terminal and preserves already committed review work.

### 5. Night Owl theme with meaningful, redundant cues

Centralize theme roles in a small module: dark navy background, light foreground, cyan selection/focus, green additions/tests, red deletions/errors, violet symbols/definitions, and amber importance/warnings. Use textual labels and `+`/`-`/review markers alongside color. Ensure selection does not erase diff meaning. Derive the final palette from Night Owl assets during implementation and check contrast on actual terminal output.

Add optional `tui.theme` in existing JSON settings, defaulting to `night-owl`, and a `T` selector for bundled `night-owl`, `tokyo-night`, `catppuccin-mocha`, and `terminal` presets plus user-declared palettes under `tui.themes`. A custom palette declares semantic roles rather than editor-specific token scopes. Theme discovery and switching stay offline and deterministic; importing arbitrary remote editor themes is a future adapter, not startup behavior. CLI `--no-color` and nonempty `NO_COLOR` remove custom foreground/background colors while preserving focus through borders and text markers. Truecolor uses RGB, limited terminals use indexed fallback, and `TERM=dumb`/non-TTY input fails before entering raw mode with a command pointing to `graph queue`/`graph hunks`. Preserve the established user configuration location; this change does not migrate it to XDG.

### 6. Cache patches and preserve prompt responsiveness

Supply captured patches from the already loaded snapshot, index once by hunk ID, and cache parsed display lines. Avoid synchronous CLI spawning, database reads, LSP work, agent calls, and repeated graph-wide scans in the key handler. Lazily load bounded unchanged-source excerpts in a worker with loading/error state; discard late results for superseded selections. Cap parsed preview/source cache at 32 MiB and bound individual source requests to 64 KiB with explicit continuation. Large patches remain scrollable through bounded display chunks; omission is visible.

Performance acceptance on a recorded Apple Silicon host: with a prebuilt graph of at least 10,000 nodes, 10,000 edges, and 100 changed hunks, median and p95 measured over at least 200 key actions; p95 cached select/follow/back-to-render <=50 ms, first captured-hunk preview <=100 ms, and warm TUI first useful frame <=1 second. Record wall-clock UI latency, not only graph query duration. No title enrichment is included in navigation time or triggered by opening the popup.

### 7. Diffview integration and acceptance

Ship an opt-in configurable Diffview keymap integration/recipe for `gq` in both `view` and `file_panel`, with descriptions visible in `g?`. Apply it to the user's existing integration during implementation after verifying actual installed config, using normal approval for out-of-workspace writes. Do not globally remap Vim's formatting operator. Preserve explicit custom Diffview mappings and provide a documented override. Resume the existing terminal, reuse its session, and restore terminal input focus; do not spawn another ranking or TUI process.

After implementation, run the requested CLI audit across all 41 practices with statuses, source evidence, justified Rust equivalents/non-applicability, and high/medium/low recommendations with concrete fixes. High-impact checks for this flow include color opt-out (§1.4), explicit interactions (§1.5), help (§1.9), signals (§1.8), TTY gating (§3.5), JSON/diagnostic streams (§3.2/3.6), degradation (§4.2), actionable errors/exit codes (§6.1/6.2/6.4), and safe argv (§10.1). Preserve error-envelope compatibility and flag differences rather than silently moving JSON errors to another stream. npm-specific package rules do not require introducing Node. Unrelated completion, uninstall, or debug facilities become documented follow-ups, not automatic scope expansion.

## Risks / Trade-offs

- Broad symbol ranges can connect unrelated hunks → smallest-enclosing mapping, explicit contextual label, negative fixtures for shared-file-only matches.
- Partial LSP coverage leaves incomplete related changes → retain coverage status and raw evidence; never fabricate tests or classify absence as low importance.
- Model titles can be vague or stale → hunk-bound, revision-checked labels, deterministic fallback, separate title-only refresh, no title-derived identity.
- Preview IO or long lines can stall rendering → bounded chunks/cache, asynchronous source reads, display-cell clipping, measured worst-case fixtures.
- Color differs by terminal and theme → centralized roles, indexed and monochrome alternatives, rendered acceptance at multiple sizes.
- New `gq` conflicts with custom mappings → restrict to Diffview integration, configurable/opt-in mapping, verify existing setup before installation.

## Migration Plan

1. Add backward-compatible title fields, label validation, and old-state fixtures. Keep legacy graph/queue fields and finalized state readable.
2. Implement relation projection and cached previews behind the new default review view; retain evidence inspection and CLI traversal.
3. Ship updated help/ranker/Neovim configuration and validate rendering plus full review flows. Rebuild and install the local binary only in the apply phase.
4. Existing sessions display rationale-derived fallback titles immediately. Offer explicit title enrichment; never rerank automatically on resume. Existing user settings require no edits unless selecting a nondefault theme.
5. Rollback binary and bridge together; additive fields are safe for older serde readers, and review status, drafts, graph IDs, and existing scores remain unchanged. Do not modify the foundation change's pending release checklist.
