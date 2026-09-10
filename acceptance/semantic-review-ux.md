# Semantic review UX acceptance

Accepted on 2026-09-09 on a MacBook Pro (Mac17,6, Apple M5 Max).

## Quality gates

All required checks exited successfully:

```text
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
76 passed; 0 failed; 1 large benchmark ignored by the normal suite
```

Headless Neovim acceptance covered asynchronous launch progress, title-first queue formatting, popup lifecycle/resume, exact captured Diffview revision/path selection, `gq` on Diffview's view and file panel, mapping preservation, and the installed Diffview configuration.

## Consumer flow

A finalized 32-hunk TypeScript review created before this change reopened through the installed binary without reranking. In a real xterm-compatible PTY, the flow exercised queue navigation, patch focus and scrolling, related-hunk traversal, evidence, Back, tag and theme selectors, help, review-state persistence, export, and clean Ctrl+C restoration. Review state was toggled twice and returned to its original value.

An isolated captured review then proved the mutable path through the installed binary: capture, graph build, semantic title/score, finalize, focused patch navigation, tag/theme selection, mark reviewed/unreviewed, comment creation, Markdown plus anchor export, quit, and reopen. The exported comment retained its exact captured source context. Progress reopened at revision 2 with the hunk restored to `unreviewed`.

The installed binary resolved from `~/.local/bin/lgr`, reported version `0.1.0`, and matched the locked release artifact byte-for-byte:

```text
SHA-256 26a5dfec7ca31db01999b6559373a4bb0f25fee9d24b3608ac369b45889a01e4
```

## Navigation benchmark

Release-mode warm-cache fixture:

```text
host=macos-aarch64 nodes=10000 edges=10000 hunks=100 actions=200
startup_ms=208.242 preview_ms=0.099 median_ms=0.092 p95_ms=0.100
io=0 child_processes=0 cache=warm
```

This meets the design limits of 1 second startup, 100 milliseconds first preview, and 50 milliseconds cached navigation p95.

## Presentation and compatibility

Queue and relation rows use semantic titles with compact locations and Unicode-aware left clipping. The preview renders captured additions, deletions, context, line coordinates, and non-text metadata without consulting the working tree. Built-in Night Owl, Tokyo Night, Catppuccin Mocha, and terminal palettes are available through `T`; user palettes are declared by semantic role under `tui.themes`. Truecolor, indexed, `NO_COLOR`, `--no-color`, and text-marker-only states are covered.

Old ranking/settings JSON remains loadable. Title enrichment is explicit and does not change scores, tags, rationales, finalization, graph data, drafts, or progress. The broader foundation change's live GitHub submission and release-distribution gates remain separate and pending.

See [semantic-review-ux-cli-audit.md](semantic-review-ux-cli-audit.md) for the complete 41-practice CLI audit.
