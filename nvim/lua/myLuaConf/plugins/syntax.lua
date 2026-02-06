return {
  {
    "nvim-treesitter",
    for_cat = "treesitter",
    event = "BufReadPost",
    after = function()
      local function is_mise_filename(filename)
        return filename:match(".*mise.*%.toml$") ~= nil
      end

      local function is_mise_predicate(_, _, bufnr, _)
        local buf = tonumber(bufnr) or 0
        local filepath = vim.api.nvim_buf_get_name(buf)
        local filename = vim.fn.fnamemodify(filepath, ":t")
        return is_mise_filename(filename)
      end

      require("vim.treesitter.query").add_predicate("is-mise?", is_mise_predicate, {
        force = true,
        all = false,
      })

      require("nvim-treesitter.configs").setup({
        modules = {},
        ensure_installed = {},
        sync_install = false,
        auto_install = false,
        ignore_install = {},
        highlight = { enable = true },
        indent = { enable = true },
      })
    end,
  },
}
