## Why

The ranked review is now navigable, but plain diff text, weak static test associations, and long Git hunks still force reviewers to leave it to understand a change. Reviewers need readable code, source-backed explanations and discussion next to that code, observable test execution evidence, and smaller independently reviewable units.

## What Changes

- Highlight captured TS/TSX, JS/JSX, HTML, and CSS using reusable syntax grammars and the existing configurable theme system.
- Add `i` to toggle the selected item's preview between Code and Context, preserving independent scroll positions. Context includes concise LLM explanations, anchored emphasis, local review comments, linked tests, and cited PR/document excerpts; a session overview exposes unassigned context.
- Extend bounded context ingestion to normalize existing PR text, review conversations, local Markdown, and explicitly supplied Folio Markdown exports. Preserve origin, freshness, anchors, and attribution.
- Add explicit, cancellable test execution and coverage import with configurable commands. Distinguish per-test-case, per-test-file, and suite coverage; surface dynamically linked unchanged tests as inspectable context as well as changed test chunks.
- Partition oversized textual hunks into deterministic semantic review chunks while retaining immutable parent patches and exact Git coordinates. Support independent ranking and completion without duplicating changed lines or moving existing comments.
- Keep rendering, context toggles, and cached traversal fast and offline. Enrichment, test execution, and remote context refresh are explicit operations.

## Capabilities

### New Capabilities

- `review-syntax-highlighting`: Captured-side token highlighting, theme roles, graceful fallback, and responsive caching.
- `hunk-review-context`: Source-backed explanations and annotations, context imports, and the Code/Context interaction.
- `test-execution-evidence`: Configurable test runs and coverage import, provenance, validity, and navigable dynamic test links.
- `semantic-review-chunks`: Deterministic smaller review units, ranking/progress compatibility, and coordinate-preserving editor/comment integration.

### Modified Capabilities

None in the current main spec tree, which is empty. Existing behavior is specified in the unarchived foundation and UX redesign changes. These new capabilities extend those contracts; the design explicitly resolves their overlap, including the foundation's one-review-item-per-Git-hunk scenario, before future spec synchronization.

## Impact

Touches snapshot/graph projection, ranking, progress, comments/editor targeting, context/GitHub ingestion, external-agent instructions, JSON settings, CLI, and Ratatui/Neovim presentation. Reuses Tree-sitter grammar/highlight assets and a normalized coverage adapter boundary; LSP remains the semantic reference provider. New serialized data is versioned and legacy sessions retain their existing queue until explicitly upgraded.

One complete release with internal milestones. Proposed initial coverage support is a configurable runner plus Istanbul JSON/LCOV import and a tested test-file isolation recipe; native Jest/Vitest per-case adapters can be selected once the user's runner is known. Aggregate coverage never creates invented individual-test links. Automatic bug review, automatic GitHub publishing, arbitrary remote-document crawling, and universal test-runner support are outside scope.
