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
          return ""
        end
      end

      ---@type oil.SetupOpts
      local oil_opts = {
        default_file_explorer = true,
        watch_for_changes = true,
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
          ["<S-CR>"] = { "actions.select", opts = { vertical = true, split = "belowright" } },
          ["gs"] = function()
            require("hop").hint_words()
          end,
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
      }

      require("oil").setup(oil_opts)
    end,
  },
}
