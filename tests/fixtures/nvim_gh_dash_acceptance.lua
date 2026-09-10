local closed = 0
local roots = {}

package.preload["gh_dash"] = function()
  return {
    close = function()
      closed = closed + 1
    end,
    toggle = function()
      table.insert(roots, vim.fn.getcwd())
    end,
  }
end

local bridge = require("lazy-git-review")
local original = vim.fn.getcwd()

vim.api.nvim_buf_set_name(0, vim.env.LGR_ROOT_ONE .. "/synthetic")
bridge.toggle_gh_dash()
assert(vim.fn.getcwd() == original, "first toggle changed Neovim cwd")

vim.api.nvim_buf_set_name(0, vim.env.LGR_ROOT_TWO .. "/synthetic")
bridge.toggle_gh_dash()
assert(vim.fn.getcwd() == original, "second toggle changed Neovim cwd")
assert(closed == 1, "dashboard was not closed exactly once after Git-root change")
assert(vim.fs.normalize(roots[1]) == vim.fs.normalize(vim.env.LGR_ROOT_ONE))
assert(vim.fs.normalize(roots[2]) == vim.fs.normalize(vim.env.LGR_ROOT_TWO))

local ids, skipped, message = bridge._pending_github_drafts({
  drafts = {
    cmt_publish = { remote_id = nil, anchor = { path = "a.ts" }, stale = false },
    cmt_stale = { remote_id = nil, anchor = { path = "b.ts" }, stale = true },
    cmt_remote = { remote_id = "123", anchor = { path = "c.ts" }, stale = false },
  },
  conflicts = {},
})
assert(message == nil)
assert(vim.deep_equal(ids, { "cmt_publish" }))
assert(skipped == 1)

local conflict_ids, _, conflict_message = bridge._pending_github_drafts({
  drafts = {},
  conflicts = { { comment_id = "cmt_conflict" } },
})
assert(conflict_ids == nil)
assert(conflict_message:find("unresolved comment conflicts", 1, true))
