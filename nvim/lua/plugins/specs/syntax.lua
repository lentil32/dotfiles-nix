return {
  {
    "nvim-treesitter",
    event = "BufReadPre",
    after = function()
      require("nvim-treesitter.configs").setup({
        highlight = { enable = true },
        indent = { enable = true },
      })
    end,
  },
}
