return {
  {
    "oil.nvim",
    after = function()
      require("config.oil").setup()
    end,
  },
}
