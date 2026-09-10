## Why

The current review works mechanically, but its queue exposes Git headers and its graph repeats long file paths without explaining what changed or where to go next. Reviewers need semantic names, readable patches, and direct movement between related changes to review most hunks inside the TUI.

## What Changes

- Give hunks concise, evidence-backed semantic titles from the existing ranking agent, distinct from importance rationales. Existing sessions remain readable without reranking.
- Make titles the primary queue content; show score, review state, tags, and a compact filename/line as supporting information.
- Replace the default raw-neighbor table with related changed hunks grouped by tests, usages/imports, definitions, and calls. `l` opens relations for the selected hunk; selecting another hunk previews it immediately; `l` follows again and `h` goes back.
- Add a scrollable inline diff preview with at least ten code lines at normal popup size. Keep raw graph evidence and unchanged source context available through secondary inspection.
- Replace cycling on `t` with a tag picker with explicit choices, counts, apply, cancel, and clear behavior.
- Introduce a Night Owl-inspired palette, visible selection and change markers, Unicode-width-aware layout, and a usable no-color fallback.
- Add configurable Diffview `gq` access to the existing queue popup from both diff buffers and the file panel; include it in Diffview help.
- Verify real review flows and run the requested `nodejs-cli-best-practices` audit after implementation, applying its language-independent practices to Rust and recording justified exceptions.

## Capabilities

### New Capabilities

None; extend the existing review and ranking capabilities.

### Modified Capabilities

- `review-navigation`: Hunk-centered navigation, patch previews, explicit filters, theme/accessibility behavior, bounded interaction latency, and the Diffview return shortcut.
- `semantic-ranking`: Persist and expose semantic change titles with safe fallback for existing or partial rankings.

These capability paths currently exist in the unarchived `add-semantic-review-workflow` change; `openspec/specs/` has no published baseline yet. This change depends on that implemented foundation. Its additive requirements refine the earlier queue/graph behavior; when both changes are synchronized, the hunk-first presentation here governs the default review surface. The earlier raw graph API and graph inspection capability remain supported. Do not archive or complete the foundation's outstanding release gates as part of this change.

## Impact

- Rust: `src/ranking.rs`, `src/tui.rs`, `src/tui/graph.rs`, `src/cli.rs`, settings, snapshot-backed preview access, and focused new presentation modules.
- Agent contract: bundled ranker instructions, CLI examples, assessment validation, queue JSON, old-ranking fixtures, and title-only enrichment without changing scores or manual overrides.
- Neovim: bridge popup behavior, Diffview keymap recipe/integration, and isolated editor acceptance fixtures.
- Verification: deterministic relationship fixtures, actual rendered terminal output, keyboard/resize/error flows, measured cache/preview latency, and a CLI audit report. Reuse Ratatui/Crossterm and captured patches; no new model provider or visualization service.
- No GitHub publication, account changes, source-review automation, or broad CLI feature expansion. Audit findings outside this UX change are documented as follow-up work.
