require("code-review").setup({})
local bridge = require("lazy-git-review")
bridge.setup({
  lgr = vim.env.LGR_BIN,
  data_dir = vim.env.LGR_DATA_DIR,
})
assert(vim.fn.maparg("<leader>rc", "n") ~= "", "code-review mapping is missing")
assert(bridge.attach(vim.env.LGR_SESSION), "bridge attach failed")
bridge.open(vim.env.LGR_NODE)
assert(vim.wait(5000, bridge._has_target, 20), "captured target did not open")
vim.wait(300, function() return false end, 20)
local state = require("code-review.state")
local before = #state.get_comments()
vim.api.nvim_feedkeys(vim.api.nvim_replace_termcodes("<leader>rc", true, false, true), "x", false)
assert(vim.wait(2000, function()
  return vim.api.nvim_buf_get_name(0):match("^codereview://input/") ~= nil
end, 20), "<leader>rc did not open comment input")
vim.api.nvim_buf_set_lines(0, 0, -1, false, { "Neovim interactive acceptance comment" })
vim.cmd("stopinsert")
vim.api.nvim_feedkeys(vim.api.nvim_replace_termcodes("<C-CR>", true, false, true), "x", false)
assert(vim.wait(2000, function() return #state.get_comments() == before + 1 end, 20), "comment was not saved")
vim.wait(1000, function() return false end, 20)
