return {
  {
    "oil.nvim",
    after = function()
      if not _G.get_oil_winbar then
        function _G.get_oil_winbar()
          local ok, oil = pcall(require, "oil")
          if not ok then
            return ""
          end
          local bufnr = vim.api.nvim_win_get_buf(vim.g.statusline_winid)
          local dir = oil.get_current_dir(bufnr)
          if dir then
            return vim.fn.fnamemodify(dir, ":~")
          end
          return vim.api.nvim_buf_get_name(bufnr)
        end
      end

      require("oil").setup({
        default_file_explorer = true,
        columns = { "icon", "permissions", "size", "mtime" },
        win_options = {
          winbar = "%!v:lua.get_oil_winbar()",
          number = false,
          relativenumber = false,
        },
        keymaps = {
          ["q"] = "actions.close",
          ["<BS>"] = "actions.parent",
          ["."] = "actions.toggle_hidden",
          ["<S-CR>"] = { "actions.select", opts = { vertical = true } },
          ["<localleader>c"] = function()
            require("oil").save()
          end,
          ["<localleader>k"] = function()
            require("oil").discard_all_changes()
          end,
          ["<localleader>p"] = function()
            require("oil").open_preview()
          end,
        },
      })
    end,
  },
}
