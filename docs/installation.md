# Installation

lazy-git-review 0.1.0 supports macOS on Apple silicon and Linux on x86-64.

## Prerequisites

All installations need:

- Git.
- One supported ranking agent. The default profile uses the Codex CLI; OpenCode and custom commands are also supported.
- Language servers for semantic TypeScript/JavaScript, HTML, and CSS analysis.

Install the language servers with npm:

```sh
npm install --global typescript typescript-language-server vscode-langservers-extracted
```

Neovim integration additionally needs Neovim 0.12 and the pinned plugins described in [Neovim integration](neovim.md). Building from source needs the Rust toolchain pinned in `rust-toolchain.toml` (Rust 1.96.0).

## Install a release archive

Download the archive and checksum for your platform from the v0.1.0 GitHub release:

- macOS Apple silicon: `lazy-git-review-0.1.0-aarch64-apple-darwin.tar.gz`
- Linux x86-64: `lazy-git-review-0.1.0-x86_64-unknown-linux-gnu.tar.gz`

Verify the checksum before extracting it:

```sh
shasum -a 256 -c lazy-git-review-0.1.0-aarch64-apple-darwin.tar.gz.sha256
# Linux alternative:
sha256sum -c lazy-git-review-0.1.0-x86_64-unknown-linux-gnu.tar.gz.sha256
```

Extract and install:

```sh
tar -xzf lazy-git-review-0.1.0-aarch64-apple-darwin.tar.gz
cd lazy-git-review-0.1.0-aarch64-apple-darwin
./install.sh
```

The installer writes executables to `${XDG_BIN_HOME:-$HOME/.local/bin}` and supporting files to `${XDG_DATA_HOME:-$HOME/.local/share}/lazy-git-review`. Add the executable directory to `PATH` if necessary:

```sh
export PATH="$HOME/.local/bin:$PATH"
```

## Build and install from source

```sh
git clone https://github.com/srsatt/lazy-git-review.git
cd lazy-git-review
git checkout v0.1.0
./scripts/install.sh
```

The source installer builds a release binary, installs the helpers, and copies the Neovim plugin, documentation, ranking skills, notices, and parser assets.

## Installer options

Set these environment variables before running `install.sh`:

| Variable | Purpose | Default |
| --- | --- | --- |
| `LGR_INSTALL_DIR` | Executable destination | `${XDG_BIN_HOME:-$HOME/.local/bin}` |
| `LGR_SHARE_DIR` | Documentation, plugin, and asset destination | `${XDG_DATA_HOME:-$HOME/.local/share}/lazy-git-review` |
| `LGR_TARGET` | Rust target used for a source build | Current host target |
| `LGR_SKIP_BUILD=1` | Install an already-built source binary | Build first |
| `LGR_SKIP_CONFIG_INIT=1` | Do not create the initial settings file | Initialize settings |

Example project-local installation:

```sh
LGR_INSTALL_DIR="$PWD/.local/bin" \
LGR_SHARE_DIR="$PWD/.local/share/lazy-git-review" \
./scripts/install.sh
```

## Verify the installation

```sh
lgr --version
lgr doctor
lgr agent-profile resolve
```

`lgr doctor` reports missing agent commands, language servers, and invalid configuration. Fix every reported error before starting a review.

Create a first local review from a clean test repository or disposable branch:

```sh
lgr review create --uncommitted
lgr session list --repository .
```

Use `--staged`, `--unstaged`, `--base <branch>`, or explicit revisions when they better match the review.

## Upgrade

Back up `~/.lgr` (or the directory selected by `data_dir`) before upgrading. Download and verify the new archive, then run its `install.sh`; it replaces installed executables and shared files but preserves settings, sessions, drafts, and caches.

Review the release notes for configuration migrations. Existing agent profiles are intentionally preserved, so defaults introduced by a newer release do not overwrite them.

## Uninstall

Remove the installed files:

```sh
rm "$HOME/.local/bin/lgr" "$HOME/.local/bin/lgr-gh-dash"
rm -r "$HOME/.local/share/lazy-git-review"
```

Adjust those paths if installer variables were used. Session data and configuration remain in `~/.lgr`; delete that directory only after backing up any drafts or reviews you want to keep.
