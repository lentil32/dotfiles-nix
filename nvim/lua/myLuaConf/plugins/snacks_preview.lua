local util = require("myLuaConf.util")

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
  if util.get_buf_opt(buf, "filetype", "") ~= ft then
    util.set_buf_opts(buf, { filetype = ft })
  end
  local snacks = util.try_require("snacks")
  if not (snacks and snacks.image and snacks.image.doc and snacks.image.terminal) then
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

function M.picker_preview(ctx)
  local Snacks = _G.Snacks or require("snacks")
  local ret = Snacks.picker.preview.file(ctx)
  local path = Snacks.picker.util.path(ctx.item)
  if not path or util.is_dir(path) then
    close_doc_preview(ctx.buf)
    return ret
  end
  attach_doc_preview(ctx.buf, path, ctx)
  return ret
end

return M
