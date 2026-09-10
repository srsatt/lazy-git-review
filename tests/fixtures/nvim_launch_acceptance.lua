local root = vim.env.LGR_TEST_ROOT
local calls = {}
local attached
local queued = 0

vim.notify = function() end
vim.fs.root = function()
  return root
end
vim.system = function(args, _, callback)
  table.insert(calls, vim.deepcopy(args))
  local result = {}
  if args[2] == "review" then
    result = { session = { id = "ses_launch" } }
  end
  callback({ code = 0, stdout = vim.json.encode({ ok = true, result = result }), stderr = "" })
  return {}
end

local bridge = require("lazy-git-review")
bridge.attach = function(session_id)
  attached = session_id
  return true
end
bridge.tui = function()
  queued = queued + 1
end

bridge.launch_uncommitted()
assert(vim.wait(2000, function() return not bridge._launching() end, 20), "uncommitted launch timed out")
assert(vim.deep_equal(calls[1], { "lgr", "review", "create", "--repository", root, "--reuse", "--uncommitted" }))
assert(vim.deep_equal(calls[2], { "lgr", "graph", "build", "ses_launch" }))
assert(vim.deep_equal(calls[3], { "lgr", "rank", "ses_launch" }))
assert(attached == "ses_launch")
assert(queued == 1)

calls = {}
attached = nil
bridge.launch_branch("origin/develop")
assert(vim.wait(2000, function() return not bridge._launching() end, 20), "branch launch timed out")
assert(vim.deep_equal(calls[1], {
  "lgr", "review", "create", "--repository", root, "--reuse", "origin/develop...HEAD",
}))
assert(attached == "ses_launch")
assert(queued == 2)

local original_system = vim.system
local pending
calls = {}
vim.system = function(args, _, callback)
  table.insert(calls, vim.deepcopy(args))
  pending = callback
  return {}
end
bridge.launch_staged()
bridge.launch_unstaged()
assert(#calls == 1, "overlapping launch started another process")
assert(bridge._launching(), "launch did not remain active")
local initial_status = bridge.status()
assert(initial_status:match("LGR 1/4 snapshot$"), "snapshot status was not visible")
assert(vim.wait(1000, function() return bridge.status() ~= initial_status end, 20), "spinner did not animate")
pending({
  code = 0,
  stdout = vim.json.encode({ ok = true, result = { session = { id = "ses_pending" } } }),
  stderr = "",
})
assert(vim.wait(2000, function() return bridge.status():match("LGR 2/4 graph$") end, 20), "graph status missing")
pending({ code = 0, stdout = vim.json.encode({ ok = true, result = {} }), stderr = "" })
assert(vim.wait(2000, function() return bridge.status():match("LGR 3/4 ranking$") end, 20), "ranking status missing")
pending({
  code = 2,
  stdout = vim.json.encode({ ok = false, errors = { { message = "ranking failed" } } }),
  stderr = "",
})
assert(vim.wait(2000, function() return not bridge._launching() end, 20), "failed launch remained active")
assert(bridge.status() == "", "failed launch retained status")
vim.system = original_system

assert(bridge._format_queue_item({
  score = 90,
  tags = { "security", "api" },
  path = "src/auth.ts",
  name = "authenticate",
  title = "Validate authentication",
}) == " 90 [security,api]  Validate authentication  src/auth.ts")
assert(vim.deep_equal(
  bridge._tui_command("ses_ranked", "/tmp/nvim.sock"),
  { "lgr", "tui", "ses_ranked", "--nvim-server", "/tmp/nvim.sock" }
))
local popup = bridge._popup_config()
assert(popup.relative == "editor" and popup.border == "rounded", "TUI is not an editor popup")
assert(popup.footer:find("l related", 1, true), "popup footer does not expose related navigation")

local existing = {
  view = { { "n", "gx", function() end, { desc = "Custom" } } },
  file_panel = { { "n", "gq", function() end, { desc = "Keep custom queue key" } } },
}
local mappings = bridge.diffview_keymaps(existing)
assert(#mappings.view == 2, "view gq mapping missing")
assert(mappings.view[2][2] == "gq" and mappings.view[2][4].desc == "Open ranked review queue")
assert(#mappings.file_panel == 1, "explicit file-panel gq mapping was overwritten")
assert(#existing.view == 1, "input Diffview mappings were mutated")

local opened
local selected_again = false
local original_nvim_cmd = vim.api.nvim_cmd
vim.api.nvim_cmd = function(command)
  opened = command
end
package.loaded["diffview.lib"] = {
  get_current_view = function()
    return {
      set_file_by_path = function()
        selected_again = true
      end,
    }
  end,
}
bridge._open_target({
  repository = "/captured/workspace",
  before_commit = "before",
  after_commit = "after",
  path = { display = "src/auth.ts", bytes_base64 = "c3JjL2F1dGgudHM=" },
  line = 7,
})
assert(vim.deep_equal(opened.args, {
  "-C=/captured/workspace",
  "before..after",
  "--selected-file=/captured/workspace/src/auth.ts",
}), "Diffview did not receive its initial selected file")
assert(not selected_again, "ranked file was selected again after Diffview started")
vim.api.nvim_cmd = original_nvim_cmd
