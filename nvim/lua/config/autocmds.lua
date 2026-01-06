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
end

return M
