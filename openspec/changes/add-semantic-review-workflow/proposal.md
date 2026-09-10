## Why

File-ordered diffs bury important behavior changes inside large files and mechanical edits. Reviewers need a queue of changed hunks and symbols, prioritized using PR intent, non-trivial logic, security, and related code, before beginning their normal review.

## What Changes

- Deliver public version `0.1.0` of a Rust CLI/binary with a lightweight Ratatui review queue, an agent skill, and Neovim integration. Publish project source and release bundles under EUPL-1.2. Implementation milestones are not separate reduced-scope releases.
- Capture GitHub PRs, local branch comparisons, staged changes, and unstaged changes as reproducible review snapshots without changing the source checkout.
- Accept Diffview-style `A..B` direct comparisons and `A...B` merge-base comparisons alongside explicit revision flags.
- Build a snapshot-bound graph from Git hunks and LSP symbols/references. Initially qualify TypeScript/JavaScript, TSX/JSX, HTML, and CSS; retain every changed hunk when semantic support is incomplete. Model tests as a distinct reference type with evidence.
- Expose compact, bounded batch graph traversal, recursive neighborhood queries, on-demand hunks/source, and atomic weight/tag updates. An external, inexpensive model follows the supplied skill to rank changes; a separate reviewer or model consumes the resulting queue.
- Launch ranking with either an ad-hoc `--agent/-a` executable or a named `--profile/-p`, injecting the embedded ranking skill so wrappers remain generic.
- Prioritize individual hunks and symbols using importance scores, evidence, and tags such as security and non-trivial logic. Preserve manual overrides and show unranked items.
- Combine PR metadata and Markdown context, with an optional user-configured pre-review hook for fetching extra material.
- Let the TUI own ordering, filtering, navigation, and progress while Neovim/Diffview displays detailed diffs and reuses the user's existing comment plugin.
- Support readable Markdown feedback for a subsequent LLM review and persistent anchored drafts for explicit GitHub review submission. Read existing PR comments and support COMMENT, APPROVE, and REQUEST_CHANGES.
- Provide complete setup documentation for release archives and source builds, external dependencies, agent and GitHub profiles, Neovim/Diffview/code-review.nvim, gh-dash, verification, upgrades, uninstall, and common recovery paths.
- Prove the complete workflow against a pinned Inker comparison, including mixed frontend/backend projects and adjacent tests, within a few-minute indexing budget.

## Capabilities

### New Capabilities

- `review-snapshots`: Immutable Git inputs, source coordinates, resumable sessions, freshness, and non-destructive refresh.
- `semantic-change-graph`: LSP-backed hunk/symbol graph, typed test references, multi-project indexing, and explicit coverage limits.
- `review-context`: PR metadata, Markdown attachments, and bounded pre-review hook output.
- `semantic-ranking`: Agent-facing graph CLI, efficient traversal skill, scores/tags, deterministic queue projection, and reviewer handoff.
- `review-navigation`: Rust distribution, lightweight TUI, shared review progress, and Neovim/Diffview navigation.
- `review-comments`: Reuse of the installed comment plugin, snapshot-aware anchors, and editable Markdown exchange.
- `github-reviews`: Existing-comment import, review preview, explicit submission, stale-anchor checks, and duplicate-safe recovery.

### Modified Capabilities

None. The repository currently contains a README and OpenSpec scaffolding, with no application or existing capability specifications.

## Impact

New Rust application modules, a small Lua bridge, an agent skill, EUPL-1.2 licensing metadata and text, task-oriented setup documentation, fixtures, and release checks will be introduced during implementation. Prefer existing Git, language servers, Ratatui, Diffview, the installed review-comment plugin, and a GitHub CLI adapter over replacement implementations. Git, Neovim/plugins, and language-server runtimes remain external dependencies; a Rust binary does not bundle those tools.

The source repository and Inker are inspection inputs only during planning. This proposal does not change their runtime, install plugins, or publish a GitHub review. Diffscope is an architectural reference; direct reuse depends on the suitability of its exposed interfaces and license.
