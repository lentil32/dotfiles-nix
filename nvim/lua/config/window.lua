local M = {}

function M.ensure_other_window()
  local wins = vim.api.nvim_tabpage_list_wins(0)
  local cur = vim.api.nvim_get_current_win()
  if #wins > 1 then
    for i, win in ipairs(wins) do
      if win == cur then
        return wins[(i % #wins) + 1], false
      end
    end
  end
  vim.cmd("vsplit")
  local new_win = vim.api.nvim_get_current_win()
  vim.api.nvim_set_current_win(cur)
  return new_win, true
end

function M.focus_other_window()
  local target = M.ensure_other_window()
  if target and vim.api.nvim_win_is_valid(target) then
    vim.api.nvim_set_current_win(target)
  end
end

function M.goto_definition_other_window()
  local target, created = M.ensure_other_window()
  local cur = vim.api.nvim_get_current_win()
  vim.lsp.buf.definition({
    on_list = function(opts)
      local items = opts.items or {}
      if vim.tbl_isempty(items) then
        if created and target and vim.api.nvim_win_is_valid(target) then
          vim.api.nvim_win_close(target, true)
        end
        return
      end

      local item = items[1]
      if target and vim.api.nvim_win_is_valid(target) then
        vim.api.nvim_win_call(target, function()
          if item.bufnr and vim.api.nvim_buf_is_valid(item.bufnr) then
            vim.api.nvim_win_set_buf(0, item.bufnr)
          elseif item.filename then
            vim.cmd("edit " .. vim.fn.fnameescape(item.filename))
          end
          local lnum = item.lnum or 1
          local col = math.max((item.col or 1) - 1, 0)
          pcall(vim.api.nvim_win_set_cursor, 0, { lnum, col })
        end)
      end

      if #items > 1 then
        vim.fn.setqflist({}, " ", opts)
      end

      if vim.api.nvim_win_is_valid(cur) then
        vim.api.nvim_set_current_win(cur)
      end
    end,
  })
end

return M
