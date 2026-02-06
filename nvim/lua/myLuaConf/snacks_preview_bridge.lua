local M = {}

local function fail(reason)
  return nil, reason
end

function M.snacks_open_preview(args)
  local ok, snacks = pcall(require, "snacks")
  if not ok or not snacks then
    return fail("snacks unavailable")
  end
  local win_id = args.win
  local src = args.src
  if not (win_id and src) then
    return fail("missing required args `win` or `src`")
  end
  if not (snacks.image and snacks.image.placement and snacks.image.config and snacks.win) then
    return fail("snacks image preview API unavailable")
  end
  local ok_width, base_width = pcall(vim.api.nvim_win_get_width, win_id)
  if not ok_width then
    return fail("invalid target window")
  end
  local ok_height, base_height = pcall(vim.api.nvim_win_get_height, win_id)
  if not ok_height then
    return fail("invalid target window")
  end
  local max_width = snacks.image.config.doc.max_width or 80
  local max_height = snacks.image.config.doc.max_height or 40
  local ok_win, win = pcall(function()
    return snacks.win(snacks.win.resolve(snacks.image.config.doc, "snacks_image", {
      relative = "win",
      win = win_id,
      row = 1,
      col = 1,
      width = math.min(max_width, base_width),
      height = math.min(max_height, base_height),
      show = true,
      enter = false,
    }))
  end)
  if not ok_win or not win then
    return fail("failed to create preview window")
  end
  local ok_open, open_err = pcall(function()
    win:open_buf()
  end)
  if not ok_open then
    return fail("failed to open preview buffer: " .. tostring(open_err))
  end
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
  local ok_placement, placement = pcall(function()
    return snacks.image.placement.new(win.buf, src, opts)
  end)
  if not ok_placement or not placement then
    pcall(function()
      win:close()
    end)
    return fail("failed to create image placement")
  end
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
