# Troubleshooting

Start every diagnosis with:

```sh
lgr --version
lgr doctor
lgr agent-profile resolve
```

## `lgr` is not found

The default executable directory is `$HOME/.local/bin`. Add it to `PATH`, open a new shell, and run `lgr --version` again. If installation used `LGR_INSTALL_DIR`, add that directory instead.

## Semantic analysis is unavailable

Install the required language servers:

```sh
npm install --global typescript typescript-language-server vscode-langservers-extracted
```

Confirm `typescript-language-server`, `vscode-html-language-server`, and `vscode-css-language-server` are on `PATH`. If TypeScript still fails, confirm the same npm installation contains `typescript/lib/tsserver.js`. Isolated test environments can point `LGR_LSP_BIN_DIR` at their executable directory.

## Ranking starts an interactive Codex screen

An older settings file may contain only `codex`. Replace it with the 0.1.0 non-interactive profile from [Configuration](configuration.md), then run:

```sh
lgr agent-profile resolve codex
```

The resolved command must begin with `codex exec --json`. Authenticate the Codex CLI separately if ranking reports missing credentials. After creating and indexing a session, use `lgr -p codex rank SESSION_ID --dry-run` to inspect the full invocation without starting the agent.

## Ranking fails or produces no usable result

Run `lgr doctor`, inspect the selected profile with `lgr agent-profile resolve`, and inspect a ranking invocation with `lgr rank SESSION_ID --dry-run`. Confirm the agent accepts standard input, produces machine-readable standard output, and exits non-zero on errors. Re-run ranking only after the command itself succeeds.

Captured files remain immutable. If repository content changed after capture, create a new review rather than editing session files.

## Neovim cannot load the plugin

Use Neovim 0.12 and the exact dependency revisions documented in [Neovim integration](neovim.md). For an archive installation, append this directory to `runtimepath`:

```text
${XDG_DATA_HOME:-$HOME/.local/share}/lazy-git-review/nvim
```

Run `:checkhealth`, then confirm `:LazyGitReviewUncommitted` exists. If Lazy.nvim manages the repository, keep the plugin eager and use the provided `init` callback so the nested `nvim` directory is added before configuration.

## The wrong GitHub account is selected

Do not switch global authentication. Resolve the profile for the intended repository:

```sh
lgr github-profile resolve --repository OWNER/REPOSITORY
lgr github-profile exec --repository-dir . --dry-run -- dash
```

Make match patterns mutually exclusive and bind every profile to `expected_account`. Use `--api-host` when an SSH alias hides the canonical hostname.

## A session or cache is stale

List sessions for the repository and create a fresh capture when the requested Git state has changed:

```sh
lgr session list --repository .
lgr review create --uncommitted
```

Do not manually change immutable snapshot files. Before removing a damaged session or cache, copy the entire data directory so comment drafts can be recovered.

## Recovery and support

Include the `lgr --version` and `lgr doctor` output when reporting a problem. Do not include repository source, captured diffs, access tokens, or private comments.

If an upgrade causes a regression, restore the backed-up data directory and reinstall the previously verified release archive. Installed binaries and shared assets can be replaced independently of stored sessions.
