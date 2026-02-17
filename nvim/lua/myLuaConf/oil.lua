local Snacks = require("snacks")
---@module "rs_plugin_util"
local plugin_util = require("rs_plugin_util")

local M = {}

---@return Options
local function hop_opts()
  ---@type Options
  local opts = vim.deepcopy(require("hop.defaults"))
  return opts
end

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
        require("hop").hint_words(hop_opts())
      end,
      ["<localleader>c"] = function()
        require("oil").save(nil)
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
  ---@param self snacks.dashboard.Class
  return function(self)
    local section = Snacks.dashboard.sections.recent_files(opts or {})(self)
    ---@type snacks.dashboard.Item[]
    local items = {}
    if type(section) == "table" then
      if vim.islist(section) then
        ---@cast section snacks.dashboard.Item[]
        items = section
      else
        ---@cast section snacks.dashboard.Item
        items = { section }
      end
    end
    for _, item in ipairs(items) do
      local path = item.file
      item.action = function()
        M.open_oil(path)
      end
    end
    ---@type snacks.dashboard.Item[]
    local section_items = {}
    if opts and opts.padding then
      ---@diagnostic disable-next-line: inject-field
      section_items.padding = opts.padding
    end
    for _, item in ipairs(items) do
      table.insert(section_items, item)
    end
    return section_items
  end
end

return M
