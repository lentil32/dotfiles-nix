return {
  {
    "nvim-treesitter",
    for_cat = "treesitter",
    event = "BufReadPre",
    after = function()
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
