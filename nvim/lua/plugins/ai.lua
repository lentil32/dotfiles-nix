return {
  {
    "sidekick.nvim",
    for_cat = "general",
    event = "BufReadPre",
    after = function()
      require("config.sidekick").setup()
    end,
  },
}
