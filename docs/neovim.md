# Neovim integration

Tested with Neovim 0.12, `sindrets/diffview.nvim` at `4516612fe98ff56ae0415a259ff6361a89419b0a`, and `choplin/code-review.nvim` at `ed91462e20bd08c3be71efb11a4a7d00459f0b47`.

Install the public release with Lazy.nvim. The bridge lives in the repository's nested `nvim` directory, so its `init` callback adds that directory before configuration:

```lua
{
  "srsatt/lazy-git-review",
  tag = "v0.1.0",
  lazy = false,
  dependencies = {
    { "sindrets/diffview.nvim", commit = "4516612fe98ff56ae0415a259ff6361a89419b0a" },
    { "choplin/code-review.nvim", commit = "ed91462e20bd08c3be71efb11a4a7d00459f0b47" },
  },
  init = function(plugin)
    vim.opt.runtimepath:append(plugin.dir .. "/nvim")
  end,
  config = function()
    require("lazy-git-review").setup({ lgr = "lgr" })
  end,
}
```

For an archive installation, append `${XDG_DATA_HOME:-$HOME/.local/share}/lazy-git-review/nvim` instead. For a source checkout, append its `nvim` directory. Configure the bridge only after that path and both dependencies are available.

The bridge uses `~/.lgr/settings.json` through the global binary. Set `data_dir` in `setup` only when intentionally overriding the configured default.

The bridge does not define mappings, so existing `code-review.nvim` mappings such as `<leader>rc` remain unchanged. Attach with `:LazyGitReviewAttach ses_...`; use `:LazyGitReviewNext`, `:LazyGitReviewPrevious`, and `:LazyGitReviewDetach`. New root comments created through `code-review.nvim` are copied to the session's canonical draft store. Diffview always opens the captured private revisions, not current working-tree content.

## Launch reviews from Neovim

The launch helpers create a snapshot, build its semantic graph, run the configured ranking agent, attach the resulting session, and open its first ranked item. They run asynchronously and reject overlapping launches. Repeating an unchanged launch reuses the session, graph, and finalized current ranking; changed Git content or review context invalidates the affected layer.

While a launch runs, the command area shows an animated four-stage indicator for snapshot, graph, ranking, and opening. `require("lazy-git-review").status()` returns the same text for a statusline component. Set `progress = false` in `setup` only when another UI consumes that function.

After ranking, the bridge opens `lgr tui` in a centered terminal popup. Queue rows lead with semantic review-unit titles and compact filename/line locations. Space toggles the selected item as reviewed and advances to the next queue item. Selecting a row previews captured code with syntax colors, visible added/removed backgrounds, dimmed surrounding source, and annotation markers. Press `i` to toggle Code/Context, `x` for linked Tests, `I` for session context, `p` for the full immutable parent hunk, `l`/Right (or queue-compatible `g`) for deduplicated related code units/usages/definitions, `h`/Left to backtrack, and `g` from a related view to return to the queue. The Tests tab separates changed test hunks from before/after runtime evidence and previews the selected test; use `j`/`k` while the preview is focused to choose a test and `[`/`]` to scroll it. `t` opens the searchable multi-tag picker, `T` chooses a built-in or configured theme, and Tab switches list/preview focus. Enter or `o` opens the exact captured unit/test range in Diffview. Use `gq`, `:LazyGitReviewQueue`, `<leader>glq`, or `require("lazy-git-review").tui()` to resume the same live popup with its pane/history offsets intact. Press `q` to finish the TUI or `<Esc><Esc>` to hide it without ending the session.

```lua
local review = require("lazy-git-review")

vim.keymap.set("n", "<leader>glu", review.launch_uncommitted, { desc = "Uncommitted changes" })
vim.keymap.set("n", "<leader>gld", review.launch_develop, { desc = "Against develop" })
vim.keymap.set("n", "<leader>glb", review.prompt_branch, { desc = "Against another branch" })
vim.keymap.set("n", "<leader>gls", review.launch_staged, { desc = "Staged changes" })
vim.keymap.set("n", "<leader>glw", review.launch_unstaged, { desc = "Unstaged changes" })
vim.keymap.set("n", "<leader>glq", review.tui, { desc = "Ranked review TUI" })
```

Install the scoped `gq` mapping in both Diffview surfaces through the bridge. Its description appears in Diffview's `g?` help. Existing explicit `gq` mappings are preserved; pass another key as the second argument to override the default.

```lua
{
  "sindrets/diffview.nvim",
  opts = function(_, opts)
    opts.keymaps = require("lazy-git-review").diffview_keymaps(opts.keymaps)
  end,
}
```

Keep the bridge plugin eager (`lazy = false`) and build these mappings inside the deferred `opts` callback. Requiring the bridge while Lazy is still collecting plugin specs can run before its local runtime directory is available.

Set `develop_branch` in `setup` when the base is named differently or should use a remote-tracking ref:

```lua
require("lazy-git-review").setup({ develop_branch = "origin/develop" })
```

Equivalent commands are `:LazyGitReviewUncommitted`, `:LazyGitReviewDevelop`, and `:LazyGitReviewBranch [base]`.

## Git Dash with directory-matched accounts

Run gh-dash through `lgr` so the current repository remote selects a GitHub profile and the expected login is verified before the dashboard starts:

```lua
{
  "johnseth97/gh-dash.nvim",
  cmd = { "GHdash", "GHdashToggle" },
  keys = {
    {
      "<leader>gD",
      function()
        require("lazy-git-review").toggle_gh_dash()
      end,
      desc = "GitHub dashboard",
    },
  },
  opts = {
    cmd = { "lgr", "github-profile", "exec", "--", "dash" },
    keymaps = {},
    border = "rounded",
    width = 0.9,
    height = 0.9,
    autoinstall = false,
  },
}
```

The bridge restarts a hidden dashboard when the current Git root changes. `lgr` resolves `origin`, selects exactly one `github_profiles` entry by `host/owner/repository`, checks its `expected_account`, and executes its structured `github_command`; it never changes global GitHub authentication. Diagnose selection without opening the dashboard:

```sh
lgr github-profile exec --repository-dir . --dry-run -- dash
```

Map dashboard repositories to their local checkouts and add the semantic-review action:

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

The helper captures the selected PR and all GitHub discussion, detects `JT-#####` in its head branch/title/body, attaches YouTrack summary and description, builds and ranks the graph, and opens the TUI. Its single progress line shows Capture PR, YouTrack context, Build graph, Rank changes, and Open review while suppressing ranker chatter. Ranking uses one agent turn and caps supplied review evidence at 128 KiB. It passes the inherited `$NVIM` server to LGR, so `o` opens the exact captured range in this Neovim instance. Draft with `<leader>rc`; closing the TUI shows a local publication preview and an explicit submit prompt. Private YouTrack issues use `YOUTRACK_TOKEN` or fall back to `YOUTRACK_API_KEY`; neither secret is inherited by LGR or its ranker. `LGR_YOUTRACK_URL` selects another instance.

Every profile command must itself be account-bound. Do not configure bare `gh` when multiple accounts exist. When a Git remote uses an SSH host alias, set the profile's canonical API hostname with `--api-host`.
