local M = {}

local function try_require(mod)
  local ok, ret = pcall(require, mod)
  if ok then
    return ret
  end
  return nil
end

local function doc_inline_enabled(snacks)
  if not snacks or not snacks.image or not snacks.image.config then
    return false
  end
  local doc = snacks.image.config.doc
  if not doc or doc.inline ~= true then
    return false
  end
  local terminal = snacks.image.terminal
  if not terminal or type(terminal.env) ~= "function" then
    return false
  end
  local ok, env = pcall(terminal.env)
  if not ok or not env then
    return false
  end
  return env.placeholders == true
end

local cleanup_id = 0
local cleanups = {}

local function store_cleanup(fn)
  cleanup_id = cleanup_id + 1
  cleanups[cleanup_id] = fn
  return cleanup_id
end

function M.filetype_match(path)
  return vim.filetype.match({ filename = path })
end

function M.snacks_has_doc()
  local snacks = try_require("snacks")
  if not snacks or not snacks.image or not snacks.image.doc or not snacks.image.terminal then
    return false
  end
  if doc_inline_enabled(snacks) then
    return false
  end
  return true
end

function M.snacks_doc_find(args)
  local snacks = try_require("snacks")
  if not snacks or not snacks.image or not snacks.image.doc then
    return
  end
  local ok, preview = pcall(require, "snacks_preview")
  if not ok then
    return
  end
  snacks.image.doc.find_visible(args.buf, function(imgs)
    preview.on_doc_find({
      buf = args.buf,
      token = args.token,
      win = args.win,
      imgs = imgs,
    })
  end)
end

function M.snacks_open_preview(args)
  local snacks = try_require("snacks")
  if not snacks then
    return nil
  end
  local win_id = args.win
  local src = args.src
  if not (win_id and src) then
    return nil
  end
  if not (snacks.image and snacks.image.placement and snacks.image.config and snacks.win) then
    return nil
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
  local placement = snacks.image.placement.new(win.buf, src, opts)
  local function cleanup()
    pcall(function()
      placement:close()
    end)
    pcall(function()
      win:close()
    end)
  end
  return store_cleanup(cleanup)
end

function M.snacks_close_preview(id)
  local cleanup = cleanups[id]
  if cleanup then
    cleanups[id] = nil
    pcall(cleanup)
  end
end

return M
