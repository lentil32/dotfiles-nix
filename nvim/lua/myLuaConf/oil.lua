local Snacks = require("snacks")
local plugin_util = require("rs_plugin_util")

local M = {}

function M.oil_winbar()
  return plugin_util.oil_winbar()
end

function M.setup()
  local oil_opts = {
    default_file_explorer = true,
    watch_for_changes = true,
    columns = { "icon", "permissions", "size", "mtime" },
    win_options = {
      winbar = "%!v:lua.require'myLuaConf.oil'.oil_winbar()",
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
  plugin_util.patch_oil_parse_url()
end

function M.open_oil(path)
  plugin_util.open_oil(path)
end

function M.open(path)
  return M.open_oil(path)
end

function M.oil_select_other_window()
  plugin_util.oil_select_other_window()
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
