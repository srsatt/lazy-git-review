local bridge = require("lazy-git-review")
bridge.setup({ lgr = vim.env.LGR_FAKE_TUI })
bridge.tui("ses_popup")

local popup
assert(vim.wait(2000, function()
  for _, window in ipairs(vim.api.nvim_list_wins()) do
    local config = vim.api.nvim_win_get_config(window)
    if config.relative == "editor" then
      popup = window
      return true
    end
  end
  return false
end, 20), "ranked TUI popup did not open")

local config = vim.api.nvim_win_get_config(popup)
assert(config.style == "minimal", "ranked TUI popup is not minimal")
assert(config.border[1] ~= "", "ranked TUI popup has no border")
local buffer = vim.api.nvim_win_get_buf(popup)
local job = vim.b[buffer].terminal_job_id
assert(type(job) == "number" and job > 0, "ranked TUI terminal did not start")
vim.fn.chansend(job, "q\n")
assert(vim.wait(2000, function() return not vim.api.nvim_win_is_valid(popup) end, 20), "ranked TUI popup did not close")
