local util = require("myLuaConf.util")

local M = {}

function M.setup()
  local function is_dir(path)
    if not path or path == "" then
      return false
    end
    local snacks_util = util.try_require("snacks.util")
    if snacks_util and snacks_util.path_type then
      return snacks_util.path_type(path) == "directory"
    end
    return vim.fn.isdirectory(path) == 1
  end

  local function set_win_cwd(win, dir)
    if not dir or dir == "" or not is_dir(dir) then
      return
    end
    if win == -1 or not vim.api.nvim_win_is_valid(win) then
      return
    end
    vim.api.nvim_win_call(win, function()
      vim.cmd("lcd " .. vim.fn.fnameescape(dir))
    end)
  end

  local function file_dir_for_buf(bufnr)
    if not (bufnr and vim.api.nvim_buf_is_valid(bufnr)) then
      return nil
    end
    local bt = vim.api.nvim_get_option_value("buftype", { buf = bufnr })
    if bt ~= "" then
      return nil
    end
    local name = vim.api.nvim_buf_get_name(bufnr)
    if name == "" then
      return nil
    end
    if name:match("^[%a][%w+.-]*://") then
      return nil
    end
    local dir = vim.fn.fnamemodify(name, ":p:h")
    if dir == "" then
      return nil
    end
    return dir
  end

  -- Show dashboard when closing to empty buffer (Spacemacs-style).
  local group = vim.api.nvim_create_augroup("UserDashboard", { clear = true })
  vim.api.nvim_create_autocmd("BufDelete", {
    group = group,
    callback = function()
      vim.schedule(function()
        if util.get_buf_opt(0, "buftype", "") ~= "" then
          return
        end
        local bufs = vim.tbl_filter(function(b)
          return vim.api.nvim_buf_is_valid(b)
            and util.get_buf_opt(b, "buflisted", false)
            and vim.api.nvim_buf_get_name(b) ~= ""
        end, vim.api.nvim_list_bufs())
        if #bufs == 0 then
          require("snacks").dashboard()
        end
      end)
    end,
  })

  -- Keep window-local cwd in sync with file buffers (Spacemacs-style).
  local file_cwd_group = vim.api.nvim_create_augroup("UserFileCwd", { clear = true })
  vim.api.nvim_create_autocmd("BufEnter", {
    group = file_cwd_group,
    callback = function(args)
      local dir = file_dir_for_buf(args.buf)
      if not dir then
        return
      end
      local win = vim.fn.bufwinid(args.buf)
      if win == -1 then
        return
      end
      set_win_cwd(win, dir)
    end,
  })

  -- Keep window-local cwd in sync with Oil directory.
  local oil_group = vim.api.nvim_create_augroup("UserOilCwd", { clear = true })
  vim.api.nvim_create_autocmd("User", {
    group = oil_group,
    pattern = "OilEnter",
    callback = function(args)
      local ok, oil = pcall(require, "oil")
      if not ok then
        return
      end
      local bufnr = (args.data and args.data.buf) or args.buf
      local dir = oil.get_current_dir(bufnr)
      if not dir or dir == "" then
        return
      end
      local win = vim.fn.bufwinid(bufnr)
      if win == -1 then
        return
      end
      local map = util.get_var(nil, "oil_last_buf", {})
      if type(map) ~= "table" then
        map = {}
      end
      ---@cast map table<number, number>
      map[win] = bufnr
      vim.g.oil_last_buf = map
      set_win_cwd(win, dir)
    end,
  })

  local oil_map_group = vim.api.nvim_create_augroup("UserOilLastBuf", { clear = true })
  local function oil_last_buf_map()
    local map = util.get_var(nil, "oil_last_buf", {})
    if type(map) ~= "table" then
      return {}
    end
    ---@cast map table<number, number>
    return map
  end

  local function write_oil_last_buf(map)
    vim.g.oil_last_buf = map
  end

  local function clean_oil_last_buf()
    local map = oil_last_buf_map()
    local changed = false
    for win, buf in pairs(map) do
      if not vim.api.nvim_win_is_valid(win) or not vim.api.nvim_buf_is_valid(buf) then
        map[win] = nil
        changed = true
      end
    end
    if changed then
      write_oil_last_buf(map)
    end
  end

  vim.api.nvim_create_autocmd("WinClosed", {
    group = oil_map_group,
    callback = function(args)
      local win = tonumber(args.match)
      if not win then
        return
      end
      local map = oil_last_buf_map()
      if map[win] == nil then
        return
      end
      map[win] = nil
      write_oil_last_buf(map)
    end,
  })

  vim.api.nvim_create_autocmd("BufWipeout", {
    group = oil_map_group,
    callback = function(args)
      local buf = args.buf
      local map = oil_last_buf_map()
      local changed = false
      for win, mapped in pairs(map) do
        if mapped == buf then
          map[win] = nil
          changed = true
        end
      end
      if changed then
        write_oil_last_buf(map)
      end
    end,
  })

  vim.api.nvim_create_autocmd("VimResized", {
    group = oil_map_group,
    callback = clean_oil_last_buf,
  })

  local oil_rename_group = vim.api.nvim_create_augroup("UserOilRename", { clear = true })
  vim.api.nvim_create_autocmd("User", {
    group = oil_rename_group,
    pattern = "OilActionsPost",
    callback = function(event)
      local actions = event.data and event.data.actions
      local first = actions and actions[1]
      if not first or first.type ~= "move" then
        return
      end
      local ok, snacks = pcall(require, "snacks")
      if not ok then
        return
      end
      snacks.rename.on_rename_file(first.src_url, first.dest_url)
    end,
  })
end

return M
