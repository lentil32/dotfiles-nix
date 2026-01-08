return {
  {
    "grug-far.nvim",
    for_cat = "general",
    after = function()
      require("grug-far").setup({})
    end,
  },
}
