local M = {}

local expected_code_review_commit = "ed91462e20bd08c3be71efb11a4a7d00459f0b47"
local expected_diffview_commit = "4516612fe98ff56ae0415a259ff6361a89419b0a"
local config = {
  lgr = "lgr",
  data_dir = nil,
  develop_branch = "develop",
  progress = true,
  diffview_queue_key = "gq",
}
local session
local last_target
local canonical_by_plugin = {}
local wrapped = false
local gh_dash_root
local launch_in_progress = false
local launch_status
local launch_timer
local tui_buffer
local tui_window
local tui_job
local tui_session
local hide_tui
local spinner_frames = { "⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏" }

local function status_text()
  if not launch_status then
    return ""
  end
  return string.format(
    "%s LGR %d/4 %s",
    spinner_frames[launch_status.frame],
    launch_status.step,
    launch_status.label
  )
end

local function render_status()
  if not launch_status then
    return
  end
  if config.progress then
    vim.api.nvim_echo({ { status_text(), "ModeMsg" } }, false, {})
  end
  vim.cmd.redrawstatus()
end

local function set_launch_stage(step, label)
  launch_status.step = step
  launch_status.label = label
  render_status()
end

local function begin_launch()
  launch_status = { step = 1, label = "snapshot", frame = 1 }
  render_status()
  local uv = vim.uv or vim.loop
  launch_timer = uv.new_timer()
  launch_timer:start(120, 120, vim.schedule_wrap(function()
    if not launch_status then
      return
    end
    launch_status.frame = launch_status.frame % #spinner_frames + 1
    render_status()
  end))
end

local function finish_launch(ok, message)
  if launch_timer then
    launch_timer:stop()
    launch_timer:close()
    launch_timer = nil
  end
  launch_status = nil
  launch_in_progress = false
  vim.cmd.redrawstatus()
  if config.progress then
    local text = ok and "✓ LGR ready" or "✗ LGR failed"
    if not ok and message and message ~= "" then
      text = "✗ LGR " .. tostring(message):gsub("[\r\n]+", " ")
    end
    vim.api.nvim_echo({ { text, ok and "ModeMsg" or "ErrorMsg" } }, false, {})
  end
end

local function args_with_global(args)
  local result = { config.lgr }
  if config.data_dir then
    vim.list_extend(result, { "--data-dir", config.data_dir })
  end
  vim.list_extend(result, args)
  return result
end

local function request(args, callback, error_callback)
  vim.system(args_with_global(args), { text = true }, function(result)
    vim.schedule(function()
      local ok, envelope = pcall(vim.json.decode, result.stdout or "")
      if result.code ~= 0 or not ok or not envelope.ok then
        local message = result.stderr
        if ok and envelope.errors and envelope.errors[1] then
          message = envelope.errors[1].message
        end
        vim.notify("lazy-git-review: " .. (message or "request failed"), vim.log.levels.ERROR)
        if error_callback then
          error_callback(message)
        end
        return
      end
      if callback then
        callback(envelope.result)
      end
    end)
  end)
end

local function repository_root()
  return vim.fs.root(0, ".git") or vim.fs.root(vim.fn.getcwd(), ".git")
end

local function launch_review(source_args)
  if launch_in_progress then
    vim.notify("lazy-git-review: review launch already running", vim.log.levels.WARN)
    return
  end
  local root = repository_root()
  if not root then
    vim.notify("lazy-git-review: current buffer is not in a Git repository", vim.log.levels.ERROR)
    return
  end

  launch_in_progress = true
  begin_launch()
  local function failed(message)
    finish_launch(false, message)
  end
  local create_args = { "review", "create", "--repository", root, "--reuse" }
  vim.list_extend(create_args, source_args)
  request(create_args, function(created)
    local session_id = created.session.id
    set_launch_stage(2, "graph")
    request({ "graph", "build", session_id }, function()
      set_launch_stage(3, "ranking")
      request({ "rank", session_id }, function()
        set_launch_stage(4, "opening")
        if M.attach(session_id) then
          finish_launch(true)
          M.tui(session_id)
        else
          failed("attach failed; see :messages")
        end
      end, failed)
    end, failed)
  end, failed)
end

local function module_root(module_name)
  local ok, module = pcall(require, module_name)
  if not ok then
    return nil
  end
  local entry = module.setup or module.open
  if type(entry) ~= "function" then
    return nil
  end
  local source = debug.getinfo(entry, "S").source:gsub("^@", "")
  local git_dir = vim.fs.find(".git", { path = vim.fs.dirname(source), upward = true })[1]
  return git_dir and vim.fs.dirname(git_dir) or nil
end

local function require_commit(module_name, expected)
  local root = module_root(module_name)
  if not root then
    return false, module_name .. " is not installed"
  end
  local result = vim.system({ "git", "-C", root, "rev-parse", "HEAD" }, { text = true }):wait()
  local actual = vim.trim(result.stdout or "")
  if result.code ~= 0 or actual ~= expected then
    return false, string.format("%s commit %s is required; found %s", module_name, expected, actual)
  end
  return true
end

local function decode_path(path)
  if vim.base64 and vim.base64.decode then
    return vim.base64.decode(path.bytes_base64)
  end
  return path.display
end

local function load_comment_mappings()
  request({ "comment", "list", session }, function(result)
    canonical_by_plugin = {}
    for id, draft in pairs(result.drafts or {}) do
      if draft.plugin_id then
        canonical_by_plugin[draft.plugin_id] = { id = id, revision = draft.body_revision }
      end
    end
  end)
end

local function wrap_code_review()
  if wrapped then
    return
  end
  local state = require("code-review.state")
  local original_add = state.add_comment
  local original_update = state.update_comment
  local original_delete = state.delete_comment

  state.add_comment = function(comment)
    local plugin_id = original_add(comment)
    if session and last_target and comment.comment and not comment.parent_id then
      request({
        "comment", "add", session,
        "--body", comment.comment,
        "--path", decode_path(last_target.path),
        "--side", last_target.side,
        "--start-line", tostring(comment.line_start),
        "--end-line", tostring(comment.line_end),
        "--plugin-id", plugin_id,
      }, function(result)
        canonical_by_plugin[plugin_id] = { id = result.comment_id, revision = 1 }
      end)
    end
    return plugin_id
  end

  state.update_comment = function(plugin_id, updates)
    local success = original_update(plugin_id, updates)
    local mapped = canonical_by_plugin[plugin_id]
    if success and mapped and updates.comment then
      request({
        "comment", "update", session, mapped.id,
        "--body", updates.comment,
        "--expected-revision", tostring(mapped.revision),
      }, function()
        mapped.revision = mapped.revision + 1
      end)
    end
    return success
  end

  state.delete_comment = function(plugin_id)
    local success = original_delete(plugin_id)
    local mapped = canonical_by_plugin[plugin_id]
    if success and mapped then
      request({ "comment", "delete", session, mapped.id }, function()
        canonical_by_plugin[plugin_id] = nil
      end)
    end
    return success
  end
  wrapped = true
end

local function focus_diffview_line(path, line, commit)
  local revision = commit:sub(1, 11)
  local autocmd
  local function focus()
    for _, window in ipairs(vim.api.nvim_list_wins()) do
      local buffer = vim.api.nvim_win_get_buf(window)
      local name = vim.api.nvim_buf_get_name(buffer)
      if name:find(path, 1, true) and name:find(revision, 1, true) then
        vim.api.nvim_set_current_win(window)
        pcall(vim.api.nvim_win_set_cursor, window, { line, 0 })
        if autocmd then
          pcall(vim.api.nvim_del_autocmd, autocmd)
        end
        return true
      end
    end
    return false
  end
  autocmd = vim.api.nvim_create_autocmd("User", {
    pattern = { "DiffviewDiffBufRead", "DiffviewViewPostLayout" },
    callback = function()
      vim.schedule(focus)
    end,
  })
  vim.defer_fn(function()
    if not focus() then
      pcall(vim.api.nvim_del_autocmd, autocmd)
    end
  end, 2000)
end

local function open_target(target)
  last_target = target
  local path = decode_path(target.path)
  local revision = target.before_commit .. ".." .. target.after_commit
  local selected_file = target.repository .. "/" .. path
  local commit = target.side == "left" and target.before_commit or target.after_commit
  hide_tui()
  focus_diffview_line(path, target.line, commit)
  vim.api.nvim_cmd({
    cmd = "DiffviewOpen",
    args = { "-C=" .. target.repository, revision, "--selected-file=" .. selected_file },
  }, {})
end

function M.open(node)
  if not session then
    vim.notify("lazy-git-review: attach a session first", vim.log.levels.ERROR)
    return
  end
  request({ "editor", "target", session, node }, function(target)
    open_target(target)
  end)
end

local function format_queue_item(item)
  local score = item.score and string.format("%3d", item.score) or "  --"
  local tags = #(item.tags or {}) > 0 and " [" .. table.concat(item.tags, ",") .. "]" or ""
  return string.format("%s%s  %s  %s", score, tags, item.title or item.name, item.path or "(general)")
end

function M.queue()
  if not session then
    vim.notify("lazy-git-review: attach a session first", vim.log.levels.ERROR)
    return
  end
  request({ "graph", "queue", session }, function(queue)
    if #(queue.items or {}) == 0 then
      vim.notify("lazy-git-review: review queue is empty", vim.log.levels.WARN)
      return
    end
    vim.ui.select(queue.items, {
      prompt = "Ranked review",
      format_item = format_queue_item,
      kind = "lazy_git_review",
    }, function(item)
      if item then
        M.open(item.node_id)
      end
    end)
  end)
end

local function ensure_server()
  if vim.v.servername ~= "" then
    return vim.v.servername
  end
  local address = vim.fn.stdpath("run") .. "/lgr-" .. vim.fn.getpid() .. ".sock"
  local ok, result = pcall(vim.fn.serverstart, address)
  if not ok or result == "" then
    vim.notify("lazy-git-review: could not start Neovim RPC server", vim.log.levels.ERROR)
    return nil
  end
  return result
end

local function popup_config()
  local width = math.max(1, math.min(120, vim.o.columns - 4))
  local height = math.max(1, math.min(40, vim.o.lines - 4))
  return {
    relative = "editor",
    style = "minimal",
    border = "rounded",
    title = " LGR ranked review ",
    title_pos = "center",
    footer = " l related · t tags · T theme · Tab preview · ? help · q close ",
    footer_pos = "center",
    width = width,
    height = height,
    col = math.floor((vim.o.columns - width) / 2),
    row = math.floor((vim.o.lines - height) / 2),
  }
end

hide_tui = function()
  if tui_window and vim.api.nvim_win_is_valid(tui_window) then
    vim.api.nvim_win_close(tui_window, false)
  end
  tui_window = nil
end

local function show_tui(session_id, exit_callback)
  local server = ensure_server()
  if not server then
    return
  end
  if tui_buffer and vim.api.nvim_buf_is_valid(tui_buffer) and tui_session == session_id then
    tui_window = vim.api.nvim_open_win(tui_buffer, true, popup_config())
    vim.api.nvim_set_current_win(tui_window)
    vim.cmd.startinsert()
    return
  end
  if tui_job then
    vim.fn.jobstop(tui_job)
  end
  tui_buffer = vim.api.nvim_create_buf(false, true)
  tui_session = session_id
  tui_window = vim.api.nvim_open_win(tui_buffer, true, popup_config())
  local command = args_with_global({ "tui", session_id, "--nvim-server", server })
  local job
  job = vim.fn.termopen(command, {
    on_exit = function()
      vim.schedule(function()
        if tui_job ~= job then
          return
        end
        hide_tui()
        if tui_buffer and vim.api.nvim_buf_is_valid(tui_buffer) then
          vim.api.nvim_buf_delete(tui_buffer, { force = true })
        end
        tui_buffer = nil
        tui_job = nil
        tui_session = nil
        if exit_callback then
          exit_callback()
        end
      end)
    end,
  })
  tui_job = job
  if tui_job <= 0 then
    hide_tui()
    vim.notify("lazy-git-review: could not open review TUI", vim.log.levels.ERROR)
    return
  end
  vim.keymap.set("t", "<Esc><Esc>", hide_tui, {
    buffer = tui_buffer,
    desc = "Hide ranked review",
  })
  vim.api.nvim_create_autocmd("WinLeave", {
    buffer = tui_buffer,
    callback = function()
      local window = tui_window
      vim.schedule(function()
        if window and vim.api.nvim_win_is_valid(window) and vim.api.nvim_get_current_win() ~= window then
          hide_tui()
        end
      end)
    end,
  })
  vim.cmd.startinsert()
end

local function pending_github_drafts(result)
  if #(result.conflicts or {}) > 0 then
    return nil, 0, "review has unresolved comment conflicts; drafts were retained"
  end
  local ids = {}
  local skipped = 0
  for id, draft in pairs(result.drafts or {}) do
    if draft.remote_id == nil and draft.anchor and not draft.stale and draft.orphaned_reason == nil then
      table.insert(ids, id)
    elseif draft.remote_id == nil then
      skipped = skipped + 1
    end
  end
  table.sort(ids)
  return ids, skipped, nil
end

function M.publish_github_review(session_id, repository, number)
  request({ "comment", "list", session_id }, function(result)
    local ids, skipped, message = pending_github_drafts(result)
    if message then
      vim.notify("lazy-git-review: " .. message, vim.log.levels.ERROR)
      return
    end
    if #ids == 0 then
      vim.notify("lazy-git-review: no unpublished anchored comments; review remains local", vim.log.levels.INFO)
      return
    end
    request({
      "github", "--repository", repository, "preview", session_id, tostring(number),
      "--comments", table.concat(ids, ","), "--event", "comment",
    }, function(preview)
      local count = #(preview.comments or {})
      local choices = {
        { label = string.format("Submit %d inline comment(s)", count), submit = true },
        { label = "Keep drafts local", submit = false },
      }
      local suffix = skipped > 0 and string.format("; %d unpublishable draft(s) stay local", skipped) or ""
      vim.ui.select(choices, {
        prompt = string.format("Publish review to %s#%d%s?", repository, number, suffix),
        format_item = function(item)
          return item.label
        end,
      }, function(choice)
        if not choice or not choice.submit then
          vim.notify("lazy-git-review: review not submitted; drafts retained locally", vim.log.levels.INFO)
          return
        end
        request({ "github", "--repository", repository, "submit", session_id }, function(response)
          local detail = response.id and (" " .. tostring(response.id)) or ""
          vim.notify("lazy-git-review: submitted GitHub review" .. detail, vim.log.levels.INFO)
        end)
      end)
    end)
  end)
end

function M.open_github_session(input)
  if type(input) ~= "table"
    or type(input.session) ~= "string"
    or type(input.repository) ~= "string"
    or type(input.number) ~= "number"
  then
    vim.notify("lazy-git-review: invalid GitHub review launch request", vim.log.levels.ERROR)
    return false
  end
  if not M.attach(input.session) then
    return false
  end
  local exit_callback
  if input.publish == 1 then
    exit_callback = function()
      M.publish_github_review(input.session, input.repository, input.number)
    end
  end
  show_tui(input.session, exit_callback)
  return true
end

function M.tui(session_id)
  if session_id then
    show_tui(session_id)
    return
  end
  if session then
    show_tui(session)
    return
  end
  local root = repository_root()
  if not root then
    vim.notify("lazy-git-review: current buffer is not in a Git repository", vim.log.levels.ERROR)
    return
  end
  request({ "session", "list", "--repository", root }, function(result)
    local latest
    for _, candidate in ipairs(result.sessions or {}) do
      if candidate.graph_revision then
        latest = candidate
        break
      end
    end
    if not latest then
      vim.notify("lazy-git-review: no indexed review session; launch a review first", vim.log.levels.WARN)
      return
    end
    if M.attach(latest.id) then
      show_tui(latest.id)
    end
  end)
end

function M.toggle_gh_dash()
  local ok, dash = pcall(require, "gh_dash")
  if not ok then
    vim.notify("lazy-git-review: gh-dash.nvim is not installed", vim.log.levels.ERROR)
    return
  end
  local root = vim.fs.root(0, ".git") or vim.fn.getcwd()
  if gh_dash_root and gh_dash_root ~= root then
    dash.close()
  end
  gh_dash_root = root

  local previous = vim.fn.getcwd()
  vim.api.nvim_set_current_dir(root)
  local toggled, message = pcall(dash.toggle)
  vim.api.nvim_set_current_dir(previous)
  if not toggled then
    vim.notify("lazy-git-review: " .. tostring(message), vim.log.levels.ERROR)
  end
end

function M.launch_uncommitted()
  launch_review({ "--uncommitted" })
end

function M.launch_staged()
  launch_review({ "--staged" })
end

function M.launch_unstaged()
  launch_review({ "--unstaged", "--include-untracked" })
end

function M.launch_branch(base)
  base = vim.trim(base or "")
  if base == "" then
    vim.notify("lazy-git-review: base revision is required", vim.log.levels.ERROR)
    return
  end
  launch_review({ base .. "...HEAD" })
end

function M.launch_develop()
  M.launch_branch(config.develop_branch)
end

function M.prompt_branch()
  vim.ui.input({ prompt = "Base revision: ", default = config.develop_branch }, function(base)
    if base then
      M.launch_branch(base)
    end
  end)
end

local function move(previous)
  request({ "progress", "show", session }, function(current)
    request({
      "progress", "next", session,
      "--expected-revision", tostring(current.progress.revision),
      previous and "--previous" or nil,
    }, function(updated)
      if updated.progress.selected then
        M.open(updated.progress.selected)
      end
    end)
  end)
end

function M.next()
  move(false)
end

function M.previous()
  move(true)
end

function M.attach(session_id)
  local ok, message = require_commit("code-review", expected_code_review_commit)
  if not ok then
    vim.notify("lazy-git-review: " .. message, vim.log.levels.ERROR)
    return false
  end
  ok, message = require_commit("diffview", expected_diffview_commit)
  if not ok then
    vim.notify("lazy-git-review: " .. message, vim.log.levels.ERROR)
    return false
  end
  session = session_id
  wrap_code_review()
  load_comment_mappings()
  return true
end

function M.detach()
  session = nil
  last_target = nil
end

function M.status()
  return status_text()
end

local function has_keymap(mappings, lhs)
  for _, mapping in ipairs(mappings or {}) do
    if mapping[2] == lhs then
      return true
    end
  end
  return false
end

function M.diffview_keymaps(existing, key)
  local result = vim.deepcopy(existing or {})
  local lhs = key or config.diffview_queue_key
  if not lhs or lhs == "" then
    return result
  end
  for _, surface in ipairs({ "view", "file_panel" }) do
    result[surface] = result[surface] or {}
    if not has_keymap(result[surface], lhs) then
      table.insert(result[surface], {
        "n",
        lhs,
        function()
          M.tui()
        end,
        { desc = "Open ranked review queue" },
      })
    end
  end
  return result
end

function M.setup(options)
  config = vim.tbl_deep_extend("force", config, options or {})
  vim.api.nvim_create_user_command("LazyGitReviewAttach", function(command)
    M.attach(command.args)
  end, { nargs = 1 })
  vim.api.nvim_create_user_command("LazyGitReviewNext", M.next, {})
  vim.api.nvim_create_user_command("LazyGitReviewPrevious", M.previous, {})
  vim.api.nvim_create_user_command("LazyGitReviewQueue", M.tui, {})
  vim.api.nvim_create_user_command("LazyGitReviewDetach", M.detach, {})
  vim.api.nvim_create_user_command("LazyGitReviewUncommitted", M.launch_uncommitted, {})
  vim.api.nvim_create_user_command("LazyGitReviewDevelop", M.launch_develop, {})
  vim.api.nvim_create_user_command("LazyGitReviewBranch", function(command)
    M.launch_branch(command.args ~= "" and command.args or config.develop_branch)
  end, { nargs = "?" })
end

M._request = request
M._decode_path = decode_path
M._format_queue_item = format_queue_item
M._open_target = open_target
M._popup_config = popup_config
M._pending_github_drafts = pending_github_drafts
M._tui_command = function(session_id, server)
  return args_with_global({ "tui", session_id, "--nvim-server", server })
end
M._has_target = function()
  return last_target ~= nil
end
M._launching = function()
  return launch_in_progress
end

return M
