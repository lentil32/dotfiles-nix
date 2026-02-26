local M = {}

---@class SnacksPreviewBridge.Args
---@field win integer
---@field src string

---@class SnacksPreviewBridge.PlacementLoc
---@field width integer
---@field height integer

---@class SnacksPreviewBridge.PlacementState
---@field loc SnacksPreviewBridge.PlacementLoc

---@class SnacksPreviewBridge.Placement
---@field close fun(self: SnacksPreviewBridge.Placement)
---@field state fun(self: SnacksPreviewBridge.Placement): SnacksPreviewBridge.PlacementState

---@class SnacksPreviewBridge.DocConfig
---@field max_width? integer
---@field max_height? integer

---@class SnacksPreviewBridge.ImageConfig
---@field doc SnacksPreviewBridge.DocConfig

---@class SnacksPreviewBridge.ImagePlacementApi
---@field new fun(buf: integer, src: string, opts: table): SnacksPreviewBridge.Placement

---@class SnacksPreviewBridge.ImageModule
---@field config SnacksPreviewBridge.ImageConfig
---@field placement SnacksPreviewBridge.ImagePlacementApi

---@class SnacksPreviewBridge.Win
---@field buf integer
---@field opts { width?: integer, height?: integer }
---@field open_buf fun(self: SnacksPreviewBridge.Win): integer
---@field show fun(self: SnacksPreviewBridge.Win): SnacksPreviewBridge.Win
---@field close fun(self: SnacksPreviewBridge.Win, opts?: { buf: boolean })

---@class SnacksPreviewBridge.WinModule
---@field resolve fun(...: table|string): table
---@overload fun(opts?: table): SnacksPreviewBridge.Win

---@param reason string
---@return nil
---@return string
local function fail(reason)
  return nil, reason
end

---@param args SnacksPreviewBridge.Args
---@return fun()|nil cleanup
---@return string|nil err
function M.snacks_open_preview(args)
  local win_id = args.win
  local src = args.src
  if type(win_id) ~= "number" or type(src) ~= "string" or src == "" then
    return fail("missing required args `win` or `src`")
  end
  if not vim.api.nvim_win_is_valid(win_id) then
    return fail("invalid target window")
  end

  local win_module = require("snacks.win")
  local image_module = require("snacks.image")
  if type(win_module) ~= "table" or type(image_module) ~= "table" then
    return fail("snacks image preview API unavailable")
  end
  ---@cast win_module SnacksPreviewBridge.WinModule
  ---@cast image_module SnacksPreviewBridge.ImageModule
  if not (win_module.resolve and image_module.placement and image_module.placement.new and image_module.config) then
    return fail("snacks image preview API unavailable")
  end

  local base_width = vim.api.nvim_win_get_width(win_id)
  local base_height = vim.api.nvim_win_get_height(win_id)
  if type(base_width) ~= "number" or type(base_height) ~= "number" then
    return fail("invalid target window")
  end

  local doc = image_module.config.doc or {}
  local max_width = tonumber(doc.max_width) or 80
  local max_height = tonumber(doc.max_height) or 40
  local width = math.min(max_width, base_width)
  local height = math.min(max_height, base_height)
  local resolved = win_module.resolve(doc, "snacks_image", {
    relative = "win",
    win = win_id,
    row = 1,
    col = 1,
    width = width,
    height = height,
    show = true,
    enter = false,
  })

  local ok_win, win = pcall(function()
    return win_module(resolved)
  end)
  if not ok_win or not win or type(win) ~= "table" then
    return fail("failed to create preview window")
  end
  ---@cast win SnacksPreviewBridge.Win

  local ok_open, open_err = pcall(function()
    win:open_buf()
  end)
  if not ok_open then
    return fail("failed to open preview buffer: " .. tostring(open_err))
  end

  local updated = false
  local opts = vim.tbl_deep_extend("force", {}, doc, {
    inline = false,
    auto_resize = true,
    ---@param p SnacksPreviewBridge.Placement
    on_update_pre = function(p)
      if not updated then
        updated = true
        local state = p:state()
        local loc = state.loc
        win.opts.width = loc.width
        win.opts.height = loc.height
        win:show()
      end
    end,
  })

  local ok_placement, placement = pcall(function()
    return image_module.placement.new(win.buf, src, opts)
  end)
  if not ok_placement or not placement or type(placement) ~= "table" then
    pcall(function()
      win:close()
    end)
    return fail("failed to create image placement")
  end
  ---@cast placement SnacksPreviewBridge.Placement

  local function cleanup()
    pcall(function()
      placement:close()
    end)
    pcall(function()
      win:close()
    end)
  end
  return cleanup, nil
end

return M
