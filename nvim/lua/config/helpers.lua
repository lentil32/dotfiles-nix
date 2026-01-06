local M = {}

function M.org_action(action)
  return function()
    vim.cmd.packadd("orgmode")
    require("orgmode").setup({
      org_agenda_files = { "~/org/**/*" },
      org_default_notes_file = "~/org/refile.org",
    })
    require("orgmode").action(action)
  end
end

function M.agitator()
  vim.cmd.packadd("agitator.nvim")
  return require("agitator")
end

function M.open_oil(path)
  if not path or path == "" then
    return
  end
  local ok, oil = pcall(require, "oil")
  if ok and vim.fn.isdirectory(path) == 1 then
    oil.open(path)
  else
    vim.cmd("edit " .. vim.fn.fnameescape(path))
  end
end

function M.focus_other_window()
  local wins = vim.api.nvim_tabpage_list_wins(0)
  if #wins > 1 then
    local cur = vim.api.nvim_get_current_win()
    for i, win in ipairs(wins) do
      if win == cur then
        vim.api.nvim_set_current_win(wins[(i % #wins) + 1])
        return
      end
    end
  else
    vim.cmd("vsplit")
  end
end

function M.oil_select_other_window()
  local ok, oil = pcall(require, "oil")
  if not ok then
    return
  end

  local entry = oil.get_cursor_entry()
  if not entry then
    return
  end

  local dir = oil.get_current_dir()
  if not dir then
    return
  end

  local path = vim.fs.joinpath(dir, entry.name)
  M.focus_other_window()
  M.open_oil(path)
end

function M.dashboard_recent_files_with_oil(opts)
  return function()
    local Snacks = require("snacks")
    local items = Snacks.dashboard.sections.recent_files(opts or {})()
    for _, item in ipairs(items) do
      local path = item.file
      item.action = function()
        if path and vim.fn.isdirectory(path) == 1 then
          M.open_oil(path)
        else
          vim.cmd("edit " .. vim.fn.fnameescape(path))
        end
      end
    end
    local section = {}
    if opts and opts.padding then
      section.padding = opts.padding
    end
    for _, item in ipairs(items) do
      table.insert(section, item)
    end
    return section
  end
end

function M.bat_preview(ctx)
  local Snacks = require("snacks")
  if vim.fn.executable("bat") ~= 1 then
    return Snacks.picker.preview.file(ctx)
  end

  local path = Snacks.picker.util.path(ctx.item)
  if not path or vim.fn.isdirectory(path) == 1 then
    return Snacks.picker.preview.file(ctx)
  end

  local uv = vim.uv or vim.loop
  local stat = uv.fs_stat(path)
  if not stat or stat.type == "directory" then
    return Snacks.picker.preview.file(ctx)
  end
  local max_size = ctx.picker.opts.previewers.file.max_size or (1024 * 1024)
  if stat.size > max_size then
    return Snacks.picker.preview.file(ctx)
  end

  local cmd = {
    "bat",
    "--style=numbers,changes",
    "--color=always",
    "--paging=never",
  }
  if ctx.item.pos and ctx.item.pos[1] then
    local line = ctx.item.pos[1]
    table.insert(cmd, "--highlight-line")
    table.insert(cmd, tostring(line))
    table.insert(cmd, "--line-range")
    table.insert(cmd, string.format("%d:%d", math.max(1, line - 5), line + 5))
  else
    table.insert(cmd, "--line-range")
    table.insert(cmd, "1:200")
  end
  table.insert(cmd, path)

  return Snacks.picker.preview.cmd(cmd, ctx, { term = true })
end

local function resolve_project_root()
  local root
  local ok, project = pcall(require, "project")
  if ok and project.get_project_root then
    local buf = vim.api.nvim_get_current_buf()
    if vim.bo[buf].buftype ~= "terminal" then
      root = project.get_project_root(buf)
    else
      local alt = vim.fn.bufnr("#")
      if alt > 0 and vim.bo[alt].buftype ~= "terminal" then
        root = project.get_project_root(alt)
      end
    end
  end
  if not root or root == "" then
    root = vim.fn.getcwd()
  end
  return root
end

function M.project_root()
  return resolve_project_root()
end

function M.show_project_root()
  local root = resolve_project_root()
  root = vim.fn.fnamemodify(root, ":~")
  vim.notify(root, vim.log.levels.INFO, { title = "Project root" })
end

function M.goto_definition_other_window()
  M.focus_other_window()
  vim.lsp.buf.definition()
end

return M
