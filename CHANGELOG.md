# Changelog

All notable changes to lazy-git-review are documented here.

## 0.1.0 — 2026-09-10

First public proof-of-concept release.

### Included

- Immutable local Git captures for revisions, branches, staged, unstaged, untracked, and combined working-tree changes.
- Semantic TypeScript/JavaScript, TSX/JSX, HTML, and CSS graphs backed by language servers.
- One-turn external-agent ranking with bounded evidence, persistent progress, context, linked tests, and local comment drafts.
- Ratatui review queue plus a Neovim 0.12 bridge for pinned Diffview and code-review.nvim revisions.
- Optional repository-bound GitHub and gh-dash flow with explicit preview and publication.
- Versioned macOS Apple silicon and Linux x86-64 archives, checksums, EUPL-1.2 terms, dependency notices, and setup documentation.

### Known limits

- Only macOS Apple silicon and Linux x86-64 release archives are produced.
- Neovim integration is qualified against the exact versions documented in `docs/neovim.md`.
- GitHub publication is optional and requires an explicitly configured account-bound command.
- HTML export and a native desktop application are outside the 0.1.0 scope.
