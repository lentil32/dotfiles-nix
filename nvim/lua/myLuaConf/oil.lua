local Snacks = require("snacks")
local util = require("myLuaConf.util")

local M = {}

local function focus_other_window()
  local target = util.get_or_create_other_window()
  if target and vim.api.nvim_win_is_valid(target) then
    vim.api.nvim_set_current_win(target)
  end
end

local function oil_winbar()
  local oil = util.try_require("oil")
  if not oil then
    return ""
  end
  local winid = tonumber(util.get_var(nil, "statusline_winid"))
  if not winid or winid == 0 then
    return ""
  end
  local bufnr = vim.api.nvim_win_get_buf(winid)
  local dir = oil.get_current_dir(bufnr)
  if dir then
    return vim.fn.fnamemodify(dir, ":~")
  end
  return ""
end

function M.setup()
  if not _G.get_oil_winbar then
    _G.get_oil_winbar = oil_winbar
  end

  local oil_opts = {
    default_file_explorer = true,
    watch_for_changes = true,
    columns = { "icon", "permissions", "size", "mtime" },
    win_options = {
      winbar = "%!v:lua.get_oil_winbar()",
      number = false,
      relativenumber = false,
    },
    keymaps = {
      ["q"] = "actions.close",
      ["<BS>"] = "actions.parent",
      ["."] = "actions.toggle_hidden",
      ["<S-CR>"] = { "actions.select", opts = { vertical = true, split = "belowright" } },
      ["gs"] = function()
        require("hop").hint_words()
      end,
      ["<localleader>c"] = function()
        require("oil").save()
      end,
      ["<localleader>k"] = function()
        require("oil").discard_all_changes()
      end,
      ["<localleader>p"] = function()
        require("oil").open_preview()
      end,
    },
  }

  require("oil").setup(oil_opts)
  M.patch_parse_url()
end

function M.patch_parse_url()
  local oil_util = util.try_require("oil.util")
  if not oil_util or oil_util._strict_parse_url then
    return
  end

  oil_util._strict_parse_url = true
  oil_util.parse_url = function(url)
    return url:match("^([%a][%w+.-]*://)(.*)$")
  end

  local config = util.try_require("oil.config")
  if not config then
    return
  end

  oil_util.get_adapter = function(bufnr, silent)
    local bufname = vim.api.nvim_buf_get_name(bufnr)
    local scheme = oil_util.parse_url(bufname)
    if not scheme then
      return nil
    end
    local adapter = config.get_adapter_by_scheme(scheme)
    if not adapter and not silent then
      vim.notify_once(string.format("[oil] could not find adapter for buffer '%s://'", bufname), vim.log.levels.ERROR)
    end
    return adapter
  end
end

function M.open_oil(path)
  if not path or path == "" then
    return
  end
  local oil = util.try_require("oil")
  if oil and util.is_dir(path) then
    oil.open(path)
  else
    util.edit_path(path)
  end
end

function M.open(path)
  return M.open_oil(path)
end

function M.oil_select_other_window()
  local oil = util.try_require("oil")
  if not oil then
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
  focus_other_window()
  M.open_oil(path)
end

function M.dashboard_recent_files_with_oil(opts)
  return function(self)
    local items = Snacks.dashboard.sections.recent_files(opts or {})(self)
    for _, item in ipairs(items) do
      local path = item.file
      item.action = function()
        M.open_oil(path)
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

return M
