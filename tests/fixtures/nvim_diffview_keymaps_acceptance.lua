local spec = dofile(vim.env.LGR_DIFFVIEW_CONFIG)
local opts = type(spec.opts) == "function" and spec.opts() or spec.opts
local keymaps = opts.keymaps

local function find(surface, lhs)
  for _, mapping in ipairs(keymaps[surface] or {}) do
    if mapping[2] == lhs then
      return mapping
    end
  end
end

for _, surface in ipairs({ "view", "file_panel" }) do
  local mapping = find(surface, "gq")
  assert(mapping, surface .. " has no gq ranked-queue mapping")
  assert(mapping[4].desc == "Open ranked review queue", surface .. " gq is absent from help")
end

assert(not find("view", "<leader>glq"), "old leader mapping still shadows the direct bridge")
