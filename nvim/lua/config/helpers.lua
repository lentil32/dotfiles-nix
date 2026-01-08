local M = {}

local doc_preview_filetypes = {
  markdown = true,
  ["markdown.mdx"] = true,
  mdx = true,
  typst = true,
  tex = true,
  plaintex = true,
  latex = true,
}

local doc_preview_state = {}

local function get_win_id(ctx)
  local win = ctx and ctx.win
  if type(win) == "number" then
    return win
  end
  if type(win) == "table" then
    return win.win
  end
end

local function state_ok(buf, token)
  return doc_preview_state[buf] and doc_preview_state[buf].token == token
end

local function close_doc_preview(buf)
  if not (buf and vim.api.nvim_buf_is_valid(buf)) then
    return
  end
  local state = doc_preview_state[buf]
  if not state then
    return
  end
  if state.group then
    pcall(vim.api.nvim_del_augroup_by_id, state.group)
  end
  if state.img then
    pcall(function()
      state.img:close()
    end)
  end
  if state.win then
    pcall(function()
      state.win:close()
    end)
  end
  doc_preview_state[buf] = nil
end

local function attach_doc_preview(buf, path, ctx)
  if not (buf and vim.api.nvim_buf_is_valid(buf)) then
    return
  end
  local ft = vim.filetype.match({ filename = path }) or ""
  if not doc_preview_filetypes[ft] then
    close_doc_preview(buf)
    return
  end
  if vim.api.nvim_buf_get_name(buf) == "" then
    pcall(vim.api.nvim_buf_set_name, buf, path)
  end
  if vim.bo[buf].filetype ~= ft then
    vim.bo[buf].filetype = ft
  end
  local ok, snacks = pcall(require, "snacks")
  if not (ok and snacks.image and snacks.image.doc and snacks.image.terminal) then
    return
  end
  local env = snacks.image.terminal.env()
  if env.placeholders then
    close_doc_preview(buf)
    snacks.image.doc.attach(buf)
    return
  end

  close_doc_preview(buf)
  local win_id = get_win_id(ctx)
  if not (win_id and vim.api.nvim_win_is_valid(win_id)) then
    return
  end

  local group = vim.api.nvim_create_augroup("snacks.doc_preview." .. buf, { clear = true })
  vim.api.nvim_create_autocmd({ "BufWipeout", "BufHidden" }, {
    group = group,
    buffer = buf,
    callback = function()
      close_doc_preview(buf)
    end,
  })
  vim.api.nvim_create_autocmd("WinClosed", {
    group = group,
    pattern = tostring(win_id),
    callback = function()
      close_doc_preview(buf)
    end,
  })

  local token = ((doc_preview_state[buf] and doc_preview_state[buf].token) or 0) + 1
  doc_preview_state[buf] = { token = token, group = group }
  snacks.image.doc.find(buf, function(imgs)
    if not state_ok(buf, token) then
      return
    end
    if not imgs or vim.tbl_isempty(imgs) then
      return
    end
    local img = imgs[1]
    vim.schedule(function()
      if not state_ok(buf, token) then
        return
      end
      if not vim.api.nvim_win_is_valid(win_id) then
        return
      end
      local max_width = snacks.image.config.doc.max_width or 80
      local max_height = snacks.image.config.doc.max_height or 40
      local base_width = vim.api.nvim_win_get_width(win_id)
      local base_height = vim.api.nvim_win_get_height(win_id)
      local win = snacks.win(snacks.win.resolve(snacks.image.config.doc, "snacks_image", {
        relative = "win",
        win = win_id,
        row = 1,
        col = 1,
        width = math.min(max_width, base_width),
        height = math.min(max_height, base_height),
        show = true,
        enter = false,
      }))
      win:open_buf()
      local updated = false
      local opts = snacks.config.merge({}, snacks.image.config.doc, {
        inline = false,
        auto_resize = true,
        on_update_pre = function(p)
          if not updated then
            updated = true
            local loc = p:state().loc
            win.opts.width = loc.width
            win.opts.height = loc.height
            win:show()
          end
        end,
      })
      local placement = snacks.image.placement.new(win.buf, img.src, opts)
      doc_preview_state[buf] = { token = token, win = win, img = placement, group = group }
    end)
  end)
end

function M.org_action(action)
  return function()
    vim.cmd.packadd("orgmode")
    require("config.org").setup()
    require("orgmode").action(action)
  end
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

function M.ensure_other_window()
  local wins = vim.api.nvim_tabpage_list_wins(0)
  local cur = vim.api.nvim_get_current_win()
  if #wins > 1 then
    for i, win in ipairs(wins) do
      if win == cur then
        return wins[(i % #wins) + 1], false
      end
    end
  end
  vim.cmd("vsplit")
  local new_win = vim.api.nvim_get_current_win()
  vim.api.nvim_set_current_win(cur)
  return new_win, true
end

function M.focus_other_window()
  local target = M.ensure_other_window()
  if target and vim.api.nvim_win_is_valid(target) then
    vim.api.nvim_set_current_win(target)
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

function M.picker_preview(ctx)
  local Snacks = require("snacks")
  local ret = Snacks.picker.preview.file(ctx)
  local path = Snacks.picker.util.path(ctx.item)
  if not path or vim.fn.isdirectory(path) == 1 then
    close_doc_preview(ctx.buf)
    return ret
  end
  attach_doc_preview(ctx.buf, path, ctx)
  return ret
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
  local target, created = M.ensure_other_window()
  local cur = vim.api.nvim_get_current_win()
  vim.lsp.buf.definition({
    on_list = function(opts)
      local items = opts.items or {}
      if vim.tbl_isempty(items) then
        if created and target and vim.api.nvim_win_is_valid(target) then
          vim.api.nvim_win_close(target, true)
        end
        return
      end

      local item = items[1]
      if target and vim.api.nvim_win_is_valid(target) then
        vim.api.nvim_win_call(target, function()
          if item.bufnr and vim.api.nvim_buf_is_valid(item.bufnr) then
            vim.api.nvim_win_set_buf(0, item.bufnr)
          elseif item.filename then
            vim.cmd("edit " .. vim.fn.fnameescape(item.filename))
          end
          local lnum = item.lnum or 1
          local col = math.max((item.col or 1) - 1, 0)
          pcall(vim.api.nvim_win_set_cursor, 0, { lnum, col })
        end)
      end

      if #items > 1 then
        vim.fn.setqflist({}, " ", opts)
      end

      if vim.api.nvim_win_is_valid(cur) then
        vim.api.nvim_set_current_win(cur)
      end
    end,
  })
end

return M
