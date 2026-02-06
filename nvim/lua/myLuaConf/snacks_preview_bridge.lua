local M = {}

function M.snacks_open_preview(args)
  local ok, snacks = pcall(require, "snacks")
  if not ok then
    return nil
  end
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
  return cleanup
end

return M
