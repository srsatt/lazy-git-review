local bridge = require("lazy-git-review")
bridge.setup({
  lgr = vim.env.LGR_BIN,
  data_dir = vim.env.LGR_DATA_DIR,
})

if vim.env.LGR_DIFFVIEW_CONFIG then
  local spec = dofile(vim.env.LGR_DIFFVIEW_CONFIG)
  local opts = type(spec.opts) == "function" and spec.opts() or spec.opts
  require("diffview").setup(opts)
end

assert(bridge.attach(vim.env.LGR_SESSION), "could not attach installed review session")
bridge._open_target({
  repository = vim.env.LGR_REPOSITORY,
  before_commit = vim.env.LGR_BEFORE,
  after_commit = vim.env.LGR_AFTER,
  path = {
    display = vim.env.LGR_PATH or "example.md",
    bytes_base64 = vim.env.LGR_PATH_BASE64 or "ZXhhbXBsZS5tZA==",
  },
  side = vim.env.LGR_SIDE or "left",
  line = tonumber(vim.env.LGR_LINE) or 1,
})

local diff_window
assert(vim.wait(5000, function()
  for _, window in ipairs(vim.api.nvim_list_wins()) do
    local name = vim.api.nvim_buf_get_name(vim.api.nvim_win_get_buf(window))
    if name:find(vim.env.LGR_PATH or "example.md", 1, true) then
      diff_window = window
      return true
    end
  end
  return false
end, 20), "Diffview did not open the captured file")

vim.api.nvim_set_current_win(diff_window)
local mapping = vim.fn.maparg("gq", "n", false, true)
assert(type(mapping) == "table" and type(mapping.callback) == "function", "Diffview gq mapping is not active")
mapping.callback()

local popup
assert(vim.wait(5000, function()
  for _, window in ipairs(vim.api.nvim_list_wins()) do
    local config = vim.api.nvim_win_get_config(window)
    if config.relative == "editor" then
      popup = window
      return true
    end
  end
  return false
end, 20), "Diffview gq did not open the installed ranked queue")

local buffer = vim.api.nvim_win_get_buf(popup)
local job = vim.b[buffer].terminal_job_id
assert(type(job) == "number" and job > 0, "installed ranked queue has no terminal job")
vim.fn.chansend(job, "q")
assert(vim.wait(3000, function()
  return not vim.api.nvim_win_is_valid(popup)
end, 20), "installed ranked queue did not close")

print("INSTALLED_NVIM_FLOW_OK")
vim.cmd("qa!")
