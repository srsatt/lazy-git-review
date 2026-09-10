# CLI best-practices audit

Audit date: 2026-09-09. Reference: all 41 practices in `nodejs-cli-best-practices`, interpreted for a compiled Rust CLI. Status: 27 followed, 6 need attention, 4 missing, 4 not applicable.

## Enrichment command re-audit

The review-unit, test-evidence, context, and explanation additions were rechecked against the same 41 practices after implementation. `review partition`, `graph units`, `tests run/import/show`, `context add --folio`, and `explain` use Clap's POSIX argument model, contextual `--help`, schema-versioned JSON, stable exit handling, structured executable-plus-argv execution, bounded files/output, and explicit dry-run/force semantics. Test execution owns its child process group and handles timeout or cancellation without leaving children.

Two in-scope findings were fixed before acceptance:

- `explain` previously accepted ambiguous mode combinations such as `--show --note`; Clap now rejects mutually exclusive read, launch, batch-write, and manual-note modes before session access.
- Partition and unit inspection previously serialized internal per-row ownership and could emit hundreds of kilobytes for a large repository. Normal output now returns compact parent/unit/range mappings and projection metadata; durable exact ownership remains in the versioned session artifact.

The status count below is unchanged because the remaining attention/missing items predate this enrichment and are not required by its explicit workflow.

## 1. Command-line experience

| # | Practice | Status | Evidence |
|---|---|---|---|
| 1.1 | POSIX arguments | Followed | Clap derives long flags, short aliases, positionals, conflicts, and contextual usage from `src/cli.rs`. |
| 1.2 | Empathic recovery | Attention | Actionable errors and Neovim selectors cover common flows, but missing required values do not become terminal prompts. This is intentional for the JSON automation contract. |
| 1.3 | Stateful data | Followed | `src/settings.rs`, `src/storage.rs`, and `src/progress.rs` persist settings, sessions, and review state. `~/.lgr` is an explicit product decision rather than XDG. |
| 1.4 | Color with opt-out | Followed | `src/tui/theme.rs` centralizes roles; `NO_COLOR`, `tui --no-color`, indexed colors, and terminal palette are supported. |
| 1.5 | Rich interactions | Followed | Ratatui provides lists, patch preview, tag/theme selectors, help, focus, and relation traversal; Neovim supplies launch progress. |
| 1.6 | Terminal hyperlinks | Not applicable | Machine output is JSON and source navigation is delegated to the Neovim bridge; the TUI does not emit external URLs. |
| 1.7 | Zero configuration | Followed | Repository/session lookup, default settings, agent profiles, and GitHub profile matching avoid routine flags. |
| 1.8 | POSIX signals | Followed | `src/tui.rs` handles Ctrl+C plus SIGTERM/SIGHUP and restores terminal state through `TerminalGuard`; test execution terminates its process group on timeout or cancellation. Package scripts trap exit signals. |
| 1.9 | Helpful help | Followed | Clap supplies root/subcommand `-h`/`--help`; enrichment commands describe execution, bounds, revisions, source sides, and cache controls; TUI `?` documents contextual keys. |

## 2. Distribution

| # | Practice | Status | Evidence |
|---|---|---|---|
| 2.1 | Small dependency footprint | Followed | Dependencies are focused; release builds use thin LTO and stripping, producing one native executable. |
| 2.2 | Locked transitive dependencies | Followed | `Cargo.lock` is committed and install/package scripts build with `--locked`, the Rust shrinkwrap equivalent. |
| 2.3 | Configuration cleanup | Missing | No uninstall command removes `~/.lgr`. Add `lgr config uninstall --dry-run`, require explicit confirmation for deletion, and list preserved review exports before mutation. |

## 3. Interoperability

| # | Practice | Status | Evidence |
|---|---|---|---|
| 3.1 | Accept stdin | Attention | Pre-review hooks receive JSON stdin, but user Markdown/comment imports require `--note` or a file. A future `--file -` should read bounded stdin without entering the TUI. |
| 3.2 | Structured output | Followed | Every non-completion command emits one schema-versioned JSON envelope; review-unit responses expose compact public mappings instead of internal row storage; tests parse stdout directly. |
| 3.3 | Cross-platform etiquette | Attention | Path handling is structured, but `std::os::unix`, POSIX signals, shell scripts, and Neovim scope the release to macOS/Linux. Document this support matrix before claiming Windows. |
| 3.4 | Configuration precedence | Attention | CLI overrides user settings, and selected standard environment variables override presentation. Project/system config tiers and general environment overrides are not implemented. |
| 3.5 | Gate interactivity | Followed | TUI requires interactive stdin and stdout and rejects `TERM=dumb` before raw mode; a pipeline regression test asserts no escape output. |
| 3.6 | stdout/stderr separation | Followed | Primary success/failure envelopes remain isolated on stdout; optional `--debug` diagnostics go to stderr. This deliberately treats structured failures as primary output. |
| 3.7 | Shell completion | Missing | The large command tree has no generated completion. Add `lgr completion <bash|zsh|fish>` using `clap_complete`, with raw candidate/script output and diagnostics only on stderr. |

## 4. Accessibility

| # | Practice | Status | Evidence |
|---|---|---|---|
| 4.1 | Container image | Not applicable | Native archives are the intended desktop/editor distribution; a container would not provide the TTY/Neovim consumer path. |
| 4.2 | Graceful degradation | Followed | Noninteractive graph/queue/hunk JSON commands replace the TUI in pipes; monochrome/indexed themes and minimum-size recovery are present. |
| 4.3 | Runtime compatibility declaration | Followed | `Cargo.toml` declares `rust-version = 1.88`; packaged binaries remove an end-user runtime requirement. |
| 4.4 | Runtime shebang | Not applicable | `lgr` is a native binary. Bundled POSIX helper scripts use `/bin/sh` and are packaging/test assets, not the executable entrypoint. |

## 5. Testing

| # | Practice | Status | Evidence |
|---|---|---|---|
| 5.1 | Locale-independent tests | Followed | Core assertions use JSON fields, IDs, state, coordinates, and rendered semantic content generated by fixtures rather than operating-system locale output. |

## 6. Errors

| # | Practice | Status | Evidence |
|---|---|---|---|
| 6.1 | Trackable errors | Followed | `src/error.rs` maps every failure to a stable code included in `ErrorBody`. |
| 6.2 | Actionable errors | Attention | User-input and TTY errors include corrective commands; generic database/I/O/serialization wrappers can still lack a recovery hint. Add operation context at conversion boundaries. |
| 6.3 | Debug mode | Followed | Global `--debug` emits the package version, stable code, and debug representation to stderr without corrupting JSON. |
| 6.4 | Exit codes | Followed | `src/error.rs` documents behavior through stable mappings: 0 success, 2 invalid input, 4 missing session, 5 revision conflict, 1 other failure. Contract tests verify them. |
| 6.5 | Effortless bug reports | Missing | Unexpected failures do not provide a prefilled report link. Add a project-neutral `support_url` build setting and append a URL containing version, platform, and error code only in debug output. |

## 7. Development

| # | Practice | Status | Evidence |
|---|---|---|---|
| 7.1 | Explicit executable mapping | Followed | `Cargo.toml` uses an explicit `[[bin]]` name/path pair, the Rust equivalent of npm's bin object. |
| 7.2 | Correct relative paths | Followed | User paths use `PathBuf` relative to cwd; bundled ranker instructions use `include_str!` relative to the crate at compile time. |
| 7.3 | Publication allowlist | Followed | `scripts/package.sh` explicitly stages only the binary, gh-dash/YouTrack helpers, skill, Neovim bridge, docs, and README. |

## 8. Analytics

| # | Practice | Status | Evidence |
|---|---|---|---|
| 8.1 | Strict opt-in analytics | Followed | No telemetry or analytics path exists. Local ranking metrics describe CLI query cost and never leave the data directory. |

## 9. Versioning

| # | Practice | Status | Evidence |
|---|---|---|---|
| 9.1 | Version flag | Followed | Clap exposes `-V`/`--version`; contract verification exercises it. |
| 9.2 | Semantic versioning | Followed | Cargo package version is `0.1.0`, valid SemVer. |
| 9.3 | Single version source | Followed | Cargo package metadata is authoritative and `env!("CARGO_PKG_VERSION")` supplies diagnostics. |
| 9.4 | Version in errors | Attention | Debug errors include the version; ordinary structured failures omit it to preserve schema. Consider an additive top-level `cli_version` in schema v2. |
| 9.5 | Backward compatibility | Followed | Serde defaults/aliases preserve old rankings/settings, legacy `harness`/`profile` aliases remain, and title fields are additive. |
| 9.6 | npm releases | Not applicable | This is a Rust binary distributed as versioned native archives, not an npm executable. |
| 9.7 | Version documents | Missing | No `CHANGELOG.md` exists. Add Keep-a-Changelog sections and require a release-note update when `Cargo.toml` version changes. |

## 10. Security

| # | Practice | Status | Evidence |
|---|---|---|---|
| 10.1 | Argument injection | Followed | Git, agents, hooks, GitHub wrappers, and Neovim RPC use executable-plus-argv APIs; shell interpolation is avoided and node IDs/profile inputs are validated. |

## Unrelated follow-ups, not enrichment blockers

High priority: none. The implemented review flow has color opt-out, explicit interactions/help, TTY gating, signal cleanup, debug diagnostics, stable exit behavior, graceful noninteractive access, and safe relation navigation.

Medium priority:

1. **3.7 shell completion** — add a raw-output completion path before normal JSON envelope dispatch:
   ```rust
   clap_complete::generate(shell, &mut Cli::command(), "lgr", &mut std::io::stdout());
   ```
2. **3.1 bounded stdin** — interpret `--file -` only on non-TTY stdin and enforce the existing context byte limit.
3. **6.2 operation context** — replace bare `?` at storage/network boundaries with stable, actionable variants such as `snapshot_read_failed` plus retry/doctor guidance.
4. **3.3 support matrix** — keep release automation macOS/Linux-only until Windows process, signal, path-byte, and editor fixtures exist.

Low priority:

1. **2.3 uninstall** — implement a dry-run-first `config uninstall`; never delete review exports implicitly.
2. **6.5 bug reports** — emit a configurable support URL only for unexpected/debug failures; do not include repository content or credentials.
3. **9.7 changelog** — add `CHANGELOG.md` and a release checklist gate.
4. **3.4 precedence / 9.4 version field** — document the intentionally small precedence model now; evolve the JSON schema additively if broader configuration or ordinary-error versioning becomes useful.
