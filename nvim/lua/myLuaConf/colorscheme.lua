local M = {}

---@module "monokai-pro"
---@type MonokaiPro
local monokai = require("monokai-pro")
---@module "rs_theme_switcher"
---@type rs_theme_switcher
local rust_switcher = require("rs_theme_switcher")

local DEFAULT_COLORSCHEME = "monokai-pro"

---@type MonokaiPro.Config
local monokai_opts = {
  devicons = true,
  filter = "octagon",
}

monokai.setup(monokai_opts)

---@type rs_theme_switcher.ThemeSpec[]
local CONFIGURED_THEMES = {
  { name = "Monokai Pro", colorscheme = "monokai-pro" },
  { name = "Kanagawa", colorscheme = "kanagawa" },
  { name = "Kanagawa Wave", colorscheme = "kanagawa-wave" },
  { name = "Kanagawa Dragon", colorscheme = "kanagawa-dragon" },
  { name = "Kanagawa Lotus", colorscheme = "kanagawa-lotus" },
  { name = "Modus Operandi", colorscheme = "modus_operandi" },
  { name = "Modus Operandi Tinted", colorscheme = "modus_operandi_tinted" },
  { name = "Modus Operandi Deuteranopia", colorscheme = "modus_operandi_deuteranopia" },
  { name = "Modus Operandi Tritanopia", colorscheme = "modus_operandi_tritanopia" },
  { name = "Modus Vivendi", colorscheme = "modus_vivendi" },
  { name = "Modus Vivendi Tinted", colorscheme = "modus_vivendi_tinted" },
  { name = "Modus Vivendi Deuteranopia", colorscheme = "modus_vivendi_deuteranopia" },
  { name = "Modus Vivendi Tritanopia", colorscheme = "modus_vivendi_tritanopia" },
}

---@return string
local function state_path()
  return vim.fn.stdpath("state") .. "/rs-theme-switcher/colorscheme.txt"
end

---@return table<string, true>
local function available_colorscheme_set()
  local raw_themes = vim.fn.getcompletion("", "color")
  ---@type table<string, true>
  local available = {}
  for _, name in ipairs(raw_themes) do
    local value = vim.trim(name or "")
    if value ~= "" then
      available[value] = true
    end
  end
  return available
end

---@return rs_theme_switcher.ThemeSpec[]
local function discover_themes()
  local available = available_colorscheme_set()
  ---@type rs_theme_switcher.ThemeSpec[]
  local themes = {}
  for _, theme in ipairs(CONFIGURED_THEMES) do
    if available[theme.colorscheme] then
      themes[#themes + 1] = {
        name = theme.name,
        colorscheme = theme.colorscheme,
      }
    end
  end
  return themes
end

---@param colorscheme string
---@return boolean
local function apply_colorscheme(colorscheme)
  local ok = pcall(vim.cmd.colorscheme, colorscheme)
  return ok
end

---@param path string
---@return string|nil
local function read_persisted_colorscheme(path)
  local ok, lines = pcall(vim.fn.readfile, path)
  if not ok or type(lines) ~= "table" or #lines == 0 then
    return nil
  end
  local value = vim.trim(lines[1] or "")
  if value == "" then
    return nil
  end
  return value
end

function M.apply()
  local persisted = read_persisted_colorscheme(state_path())
  local candidate = persisted or DEFAULT_COLORSCHEME
  if apply_colorscheme(candidate) then
    return
  end
  if candidate ~= DEFAULT_COLORSCHEME and apply_colorscheme(DEFAULT_COLORSCHEME) then
    vim.notify(
      ("Failed to load colorscheme '%s'; using '%s'"):format(candidate, DEFAULT_COLORSCHEME),
      vim.log.levels.WARN
    )
    return
  end
  vim.notify(("Failed to load colorscheme '%s'"):format(candidate), vim.log.levels.ERROR)
end

---@param themes rs_theme_switcher.ThemeSpec[]
---@return rs_theme_switcher.OpenArgs
local function switcher_args(themes)
  return {
    title = "Theme Switcher",
    themes = themes,
    current_colorscheme = vim.g.colors_name,
    state_path = state_path(),
  }
end

---@return rs_theme_switcher.ThemeSpec[]|nil
local function configured_themes_or_warn()
  local themes = discover_themes()
  if #themes == 0 then
    vim.notify("No configured colorschemes are available", vim.log.levels.WARN)
    return nil
  end
  return themes
end

function M.open_switcher()
  local themes = configured_themes_or_warn()
  if themes == nil then
    return
  end
  rust_switcher.open(switcher_args(themes))
end

function M.next_theme()
  local themes = configured_themes_or_warn()
  if themes == nil then
    return
  end
  rust_switcher.cycle_next(switcher_args(themes))
end

function M.prev_theme()
  local themes = configured_themes_or_warn()
  if themes == nil then
    return
  end
  rust_switcher.cycle_prev(switcher_args(themes))
end

return M
