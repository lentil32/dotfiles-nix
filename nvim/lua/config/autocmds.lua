local M = {}

function M.setup()
  -- Show dashboard when closing to empty buffer (Spacemacs-style).
  local group = vim.api.nvim_create_augroup("UserDashboard", { clear = true })
  vim.api.nvim_create_autocmd("BufDelete", {
    group = group,
    callback = function()
      vim.schedule(function()
        if vim.bo.buftype ~= "" then
          return
        end
        local bufs = vim.tbl_filter(function(b)
          return vim.api.nvim_buf_is_valid(b)
            and vim.bo[b].buflisted
            and vim.api.nvim_buf_get_name(b) ~= ""
        end, vim.api.nvim_list_bufs())
        if #bufs == 0 then
          require("snacks").dashboard()
        end
      end)
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
      vim.api.nvim_win_call(win, function()
        vim.cmd("lcd " .. vim.fn.fnameescape(dir))
      end)
    end,
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
