# Configuration

Run `lgr doctor` after each configuration change. Settings are stored in `~/.lgr/settings.json` unless `data_dir` is overridden.

## Ranking agent

A fresh 0.1.0 installation selects this Codex profile:

```text
codex exec --json --sandbox read-only --ephemeral
```

Authenticate the Codex CLI separately, then confirm the selected command:

```sh
lgr agent-profile resolve
```

Settings created by an older build are preserved. Replace an old interactive `codex` command with the non-interactive profile:

```sh
lgr agent-profile set codex --command codex \
  --arg=exec \
  --arg=--json \
  --arg=--sandbox \
  --arg=read-only \
  --arg=--ephemeral \
  --select
```

Fresh settings also include the non-interactive `opencode run` profile. Select it with:

```sh
lgr agent-profile select opencode
```

For another agent, pass its executable with `--command` and each fixed argument with `--arg`.

The command must:

- read its task from standard input;
- run without an interactive terminal;
- write its result to standard output;
- return a non-zero exit status on failure;
- avoid modifying the reviewed repository.

After creating and indexing a session, inspect the complete ranking invocation without starting the agent:

```sh
lgr -p codex rank SESSION_ID --dry-run
```

## Review workflow

Create a session, rank it, and open the terminal UI:

```sh
lgr review create --uncommitted
lgr rank SESSION_ID
lgr tui SESSION_ID
```

The create command prints the session ID. Other useful capture modes are `--staged`, `--unstaged`, `--base <branch>`, and explicit revision arguments. List resumable sessions with:

```sh
lgr session list --repository .
```

Export local draft comments without publishing them:

```sh
lgr comment export SESSION_ID --output review.md
```

## GitHub accounts

GitHub commands are configured as structured profiles. Each profile matches a repository and binds the command to an expected account; lazy-git-review never switches global GitHub authentication.

```sh
lgr github-profile set personal \
  --match 'github.com/YOUR_LOGIN/*' \
  --github-command gh-personal \
  --expected-account YOUR_LOGIN

lgr github-profile resolve --repository YOUR_LOGIN/REPOSITORY
```

Use an account-bound wrapper such as `gh-personal` when multiple GitHub accounts exist. Do not configure bare `gh` in that situation. For SSH remote aliases, add `--api-host github.com` so account verification uses the canonical API host.

Publishing comments changes remote state. First export and inspect the local preview, then use the explicit publish command only against the intended pull request.

## Paths and environment

The data directory contains settings, captured sessions, immutable source snapshots, cached graphs, rankings, and comment drafts. Back it up before upgrades or manual repair.

Language-server executables are discovered from `PATH`. Test and isolated environments may set `LGR_LSP_BIN_DIR` to a directory containing `typescript-language-server`, `vscode-html-language-server`, and `vscode-css-language-server`. TypeScript analysis also needs the `typescript` package and its `tsserver.js` installation.

The GitHub/Neovim helper inherits the active Neovim server address so exact captured ranges can open in the existing editor. Private YouTrack context uses `YOUTRACK_TOKEN`, falling back to `YOUTRACK_API_KEY`; `LGR_YOUTRACK_URL` selects a different server.

## Neovim

Install and configure the editor bridge after the CLI passes `lgr doctor`. See [Neovim integration](neovim.md) for the complete Lazy.nvim specification and pinned dependency versions.
