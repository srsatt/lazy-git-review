# lazy-git-review

`lgr` captures an immutable Git comparison, asks configured language servers for symbols and relationships, and exposes a bounded JSON graph that an external agent can turn into a semantic review queue. Oversized hunks become independently ranked review units without changing their raw Git patches. Ratatui provides captured syntax-highlighted code, explanations, context, runtime-linked tests, and progress; Neovim, Diffview, and code-review.nvim provide detailed viewing and comments. No model API key or service is built into the binary.

## Quick start

Install a supported `0.1.0` archive:

```sh
tar -xzf lazy-git-review-0.1.0-aarch64-apple-darwin.tar.gz
cd lazy-git-review-0.1.0-aarch64-apple-darwin
./install.sh
lgr --version
lgr doctor
```

Linux x86_64 uses `lazy-git-review-0.1.0-x86_64-unknown-linux-gnu.tar.gz`. Source builds use the same installer from the repository root:

```sh
./scripts/install.sh
```

Create and rank a local review:

```sh
lgr review create --repository . --uncommitted
lgr graph build SESSION
lgr rank SESSION
lgr tui SESSION
```

Copy `SESSION` from the first command's JSON result. New installations configure noninteractive Codex ranking by default; any external agent remains user-selected and separately authenticated. Neovim, gh-dash, GitHub publication, runtime test execution, and ticket hooks are optional integrations.

## Documentation

- [Documentation index](docs/README.md)
- [Installation and upgrades](docs/installation.md)
- [Configuration and agent/GitHub profiles](docs/configuration.md)
- [Neovim, Diffview, code-review.nvim, and gh-dash](docs/neovim.md)
- [Troubleshooting, backup, and recovery](docs/troubleshooting.md)
- [0.1.0 release notes](CHANGELOG.md)

## Capture inputs

Every successful command emits one versioned JSON envelope on stdout. Choose one source:

```sh
lgr review create --repository . --base main --head feature
lgr review create --repository . --base BASE_SHA --head HEAD_SHA --direct
lgr review create --repository . feature-A..origin/develop
lgr review create --repository . origin/develop...feature-A
lgr review create --repository . --uncommitted
lgr review create --repository . --staged
lgr review create --repository . --unstaged --include-untracked
lgr github --repository OWNER/REPO create PR_NUMBER --local-repository .
```

Capture materializes private before/after trees and private commits. It does not check out or modify source refs, files, index, or hooks. `review refresh SESSION` creates another snapshot and transfers only unambiguous unchanged hunk identities.
`--uncommitted` compares `HEAD` with the working tree and includes staged, unstaged, and untracked changes.
Add `--reuse` to return the newest session for exactly unchanged selected Git content. Neovim launch actions do this automatically, preserving its graph, ranking, progress, and drafts.

## Context and hooks

Local Markdown and inline intent work offline:

```sh
lgr context add SESSION --file design.md
lgr context add SESSION --file folio-report.md --folio --source-url folio://report/ID
lgr context add SESSION --note 'Security boundary: device tokens are scoped per device.'
```

A pre-review hook is an explicitly configured absolute executable plus argument list. It receives review identity as JSON stdin and must emit Markdown stdout:

```sh
lgr context hook SESSION --executable /absolute/path/fetch-ticket --argv PROJECT-123 --timeout 30 --max-bytes 262144
```

Failures stop ranking unless the user retries or passes `--continue-without-context`. Markdown, PR bodies, links, and repository content never activate executables. Context digests make finalized rankings visibly stale after intent changes.

GitHub PR sessions capture the title/body, review summaries, issue discussion, and inline review threads with stable source keys, authors, URLs, timestamps, reply IDs, and exact captured anchors where GitHub still supplies current coordinates. Partial refreshes retain missing older material as partial session context instead of silently deleting it.

## Graph and ranking

```sh
lgr graph build SESSION --time-budget 180 --max-files 500 --max-symbols 12000
lgr graph evidence SESSION --max-bytes 131072
lgr graph overview SESSION
lgr graph units SESSION
lgr graph units SESSION --parent HUNK_ID
lgr graph nodes SESSION ID... --fields id,kind,name,locations,hunk_ids
lgr graph walk SESSION --seeds ID,ID --edges runtime_test,test_reference,calls --depth 2 --limit 100
lgr graph hunks SESSION HUNK_OR_UNIT_ID... --max-bytes 65536
lgr graph source SESSION NODE_ID... --max-bytes 65536
lgr graph expand SESSION --time-budget 60 --max-symbols 12000
```

Responses expose graph revision, per-project/side/method coverage, incomplete frontiers, page offsets, continuation tokens, and per-item errors. Ordinary reads never start language servers. Expansion does and creates a new revision.
Repeated graph builds reuse a matching analysis fingerprint. A finalized ranking is also reused while its graph revision and context digest remain current. Responses expose `cache_hit`; use `graph build SESSION --force` or `rank SESSION --force` to bypass the relevant cache.

`lgr rank` is one model turn: LGR embeds at most 128 KiB of internally collected evidence in the prompt, accepts one JSON assessment batch, validates complete coverage, then applies and finalizes the ranking locally. Bundled instructions tell the agent to use only that evidence and not call tools; the default Codex profile also uses a read-only sandbox. The selected agent and model remain controlled by the user. `graph evidence`, `graph score`, and `graph finalize` remain available for manual/API workflows. `A..B` captures direct endpoints; `A...B` captures merge-base-to-`B`. `graph queue` includes every active review leaf and non-text change. Direct unit scores override inherited parent/symbol scores; manual scores override later model updates.

New sessions automatically partition textual hunks above 120 changed lines into stable, exact review units targeting roughly 80 and never exceeding 120 changed lines. Raw hunk IDs and `graph hunks` remain available. Legacy sessions migrate only on explicit apply, with revision checks and rollback:

```sh
lgr review partition SESSION
lgr review partition SESSION --apply --expected-revision 0
lgr review partition SESSION --rollback --expected-revision 1
```

Apply migrates reviewed parents to all children and preserves parent assessments for inheritance. Rollback restores ranking/progress while keeping source-anchored drafts created during chunk review.

Scores can include concise semantic hunk titles. Existing finalized rankings remain valid and use deterministic fallback titles; enrich only missing titles without reranking via `lgr rank SESSION --titles-only`, or refresh model titles with `--titles-only --force`.

Explanations are a separate, bounded artifact. They never alter scores, progress, drafts, or source. Launch the selected agent explicitly, inspect current data, add a manual note, or apply a revision-checked JSON batch:

```sh
lgr explain SESSION --dry-run
lgr explain SESSION
lgr explain SESSION --show
lgr explain SESSION --item UNIT_ID --note 'Check compatibility' --expected-revision 0
lgr explain SESSION --graph-revision grf_... --expected-revision 1 --updates-file explanations.json
```

```sh
lgr graph queue SESSION
lgr graph finalize SESSION [--compact] [--require-complete]
lgr session list
lgr tui
lgr tui SESSION
```

Ranking output reports query count, returned bytes, source bytes, elapsed query time, assessed count, inherited assessments, graph/context revisions, stale state, and whether coverage is complete.

## Runtime test evidence

Tests run only through explicit `tests run`; browsing and graph traversal never execute them. Profiles are structured argv in `~/.lgr/settings.json`, run against a disposable copy of the captured left/right tree, and may opt into an explicit preparation command. No shell interpolation or implicit dependency installation occurs.

Use `{repository}` in the executable or argv to resolve tools and dependency directories from the worktree captured by the session. This keeps one profile reusable across worktrees. `{workspace}` resolves to the disposable captured tree, `{report}` to its configured report path, and `{selection}` expands to the explicit test selectors.

```json
{
  "test_profiles": {
    "jest-file": {
      "executable": "./node_modules/.bin/jest",
      "argv": ["--runTestsByPath", "{selection}", "--coverage", "--coverageReporters=json", "--coverageDirectory=coverage"],
      "prepare_executable": "npm",
      "prepare_argv": ["install", "--ignore-scripts"],
      "project_root": ".",
      "report_format": "istanbul-json",
      "report_path": "coverage/coverage-final.json",
      "attribution": "file"
    }
  }
}
```

```sh
lgr tests run SESSION --profile jest-file --side right --select tests/a.test.ts --dry-run
lgr tests run SESSION --profile jest-file --side right --select tests/a.test.ts
lgr tests show SESSION
lgr tests import SESSION coverage.lcov --format lcov --test-file tests/a.test.ts --captured-source-report
```

Completed runs cache by snapshot, side, source/dependency fingerprints, runner configuration, and selected tests; use `--force` to rerun. Istanbul JSON, LCOV, and an attributed manifest format are supported. Only compatible hit ranges with file/case attribution create navigable runtime-test edges. Suite coverage stays informational, and execution evidence never claims which assertion proved a line. See [the Jest isolation example](examples/jest-file-isolation/README.md).

## Comments and Neovim

`lgr session list` shows recent sessions for the current repository with capture/index/ranking state and file/change/assessed/reviewed counts. `lgr tui` opens the newest indexed session for the current repository; pass a session ID to select another one.

The TUI owns ordering, filtering, priority, and reviewed state. Rows lead with semantic titles and compact locations; a scrollable captured patch stays visible below. Added and removed lines retain syntax colors while using distinct diff gutters and backgrounds, and extra captured source context is dimmed around each hunk. Space toggles the selected item as reviewed and advances to the next queue item. `i` toggles selected Code/Context, `x` opens a linked Tests tab, `I` opens all session context, and `p` toggles a unit/full-parent patch. The Tests tab separates changed test hunks from before/after runtime evidence and previews the selected test; case-level producers show exact test names while file-level coverage remains labeled as file-level evidence. `l`/Right follows deduplicated related code changes, usages, and definitions; runtime test nodes live in the Tests tab instead of the related-code list. `h`/Left returns. `t` opens searchable multi-tag filtering, `T` selects a theme, and Tab changes list/preview focus. In the Tests tab, `j`/`k` chooses a test and `[`/`]` scrolls its preview. `o` or Enter opens the exact captured range in Neovim. Navigation uses in-memory indexes and starts no language server, agent, network request, database query, or test process. See [docs/neovim.md](docs/neovim.md).

The default theme is `night-owl`; `tokyo-night`, `catppuccin-mocha`, and `terminal` are bundled. Select the startup theme or declare a named semantic palette in `~/.lgr/settings.json`:

```json
{
  "tui": {
    "theme": "my-theme",
    "themes": {
      "my-theme": {
        "background": "#011627", "foreground": "#D6DEEB",
        "muted": "#637777", "border": "#5C7E8D", "focus": "#7FDBCA",
        "selection": "#7FDBCA", "important": "#FFCB8B", "test": "#ADDB67",
        "usage": "#7FDBCA", "definition": "#C792EA", "addition": "#ADDB67",
        "deletion": "#EF5350", "warning": "#FFCB8B", "error": "#EF5350"
      }
    }
  }
}
```

This object belongs alongside the existing settings fields rather than replacing them. Optional `syntax` roles are `comment`, `string`, `number`, `keyword`, `function`, `type`, `property`, and `variable`; omitted roles use Night Owl defaults. Set `tui.syntax_highlighting` to `false` for plain previews. Theme switching reuses cached tokens and is offline. Unsupported/binary/oversized sources, `--no-color`, and `NO_COLOR=1` keep all state and diff markers readable without custom colors.

Neovim launch actions open this TUI in a resumable floating terminal. Selecting a ranked item hides the popup and opens the full captured comparison in the same editor; `<leader>glq` reopens it in the supplied configuration.

```sh
lgr comment add SESSION --node HUNK_OR_UNIT_ID --body 'Check this boundary.'
lgr comment export SESSION --output review.md
lgr comment import SESSION review.md
```

`review.md` is readable alone; `review.md.anchors.json` carries machine anchors. Missing or damaged metadata never guesses an inline location. Concurrent edits preserve both versions as conflicts. Deletion is explicit. Saving or exporting never publishes.

## GitHub review

GitHub profiles select an account-bound CLI wrapper from repository identity. Public defaults contain no accounts; configure each identity in user-owned settings. Profile commands store no token and never change GitHub CLI's global active account:

```sh
lgr github-profile set primary \
  --match 'github.com/OWNER/*' \
  --github-command gh-account \
  --expected-account LOGIN
lgr github-profile resolve --repository OWNER/REPO
```

Use repeated `--match` options for multiple owners and repeated `--github-arg` options when a wrapper needs fixed arguments. Matching uses `HOST/OWNER/REPO`. An ambiguous or unmatched repository fails before any GitHub request; global `--profile NAME` is the explicit override for GitHub operations too. GitHub and agent profiles use separate settings collections, so matching names such as `work` is safe. `lgr profile` remains an alias for `lgr github-profile`; `--profile-config FILE` remains a standalone override for automation.

Run arbitrary GitHub CLI extensions through the directory-matched profile without changing global authentication:

```sh
lgr github-profile exec --repository-dir . --dry-run -- dash
lgr github-profile exec --repository-dir . -- dash
```

Before live execution, `lgr` verifies the wrapper's authenticated login against `expected_account`, then replaces itself with the structured profile command. This supports a repo-aware `gh-dash.nvim` launcher; see [docs/neovim.md](docs/neovim.md).

Add a PR keybinding to gh-dash after configuring its `repoPaths` mapping:

```yaml
keybindings:
  prs:
    - key: R
      name: semantic review
      command: >
        lgr-gh-dash --repository {{.RepoName}} --number {{.PrNumber}}
        --local-repository {{.RepoPath}}
repoPaths:
  OWNER/REPOSITORY: /absolute/path/to/repository
```

`R` captures the PR title, body, review summaries, issue comments, and inline threads. A single animated progress line moves through Capture PR, YouTrack context, Build graph, Rank changes, and Open review; ranker logs stay hidden unless needed for a concise failure. The helper detects up to five unique `JT-#####` references in the head branch, title, and body; fetches each issue's summary and description from `https://youtrack.jetbrains.com`; builds and ranks the graph; then opens the LGR TUI. Private issues use `YOUTRACK_TOKEN` or, as a fallback, `YOUTRACK_API_KEY`; the secret is removed from all LGR, ranker, GitHub, TUI, and Neovim client subprocesses. Override the instance with `LGR_YOUTRACK_URL`. A linked issue fetch failure stops before ranking unless the command includes `--continue-without-youtrack`.

Use `o` in LGR to open a captured range in Neovim and the configured `<leader>rc` flow to draft comments. After the TUI closes, `lgr-gh-dash` previews every new anchored draft and asks before submitting one GitHub `COMMENT` review. Declining or pressing Enter keeps all drafts local; `--no-publish-prompt` skips the preview and prompt.

```sh
lgr github --repository OWNER/REPO preview SESSION PR_NUMBER \
  --comments cmt_ID,cmt_ID --event comment --summary 'Review summary'
lgr github --repository OWNER/REPO submit SESSION
lgr github --repository OWNER/REPO reconcile SESSION
```

Preview validates LEFT/RIGHT and multiline anchors against the captured diff. Submit rechecks the resolved profile identity, PR head, and draft revisions, then journals intent before one mutation. A lost response enters uncertain state; `reconcile` queries remote reviews instead of retrying blindly. Network/auth/API failures leave local drafts and Markdown available. Inline drafts are never silently converted to summary feedback.

## Storage and development

`~/.lgr/data` is the default per-user data directory; `--data-dir DIR` overrides it for one command. SQLite session migrations create timestamped backups before upgrading. Snapshot directories contain immutable blobs and before/after trees plus graph, review-unit, context, explanation, test-evidence, ranking, comment, and progress files and GitHub preview/intent journals. Back up the entire data directory.

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

Set `LGR_LSP_BIN_DIR` to qualified real server executables to enable real-server tests.

Recorded consumer and performance evidence lives in [acceptance/semantic-review-ux.md](acceptance/semantic-review-ux.md). Enriched review-unit, syntax, test-evidence, context, and installed-editor proof lives in [acceptance/enrich-review-context-and-test-evidence.md](acceptance/enrich-review-context-and-test-evidence.md); the companion [CLI audit](acceptance/semantic-review-ux-cli-audit.md) evaluates all 41 applicable Node.js CLI practices through their Rust equivalents.

## Licence

Copyright 2026 Pavel Reutov. Licensed under the [EUPL-1.2](LICENSE). Third-party terms are recorded in [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).
