return {
  {
    "sidekick.nvim",
    event = "BufReadPre",
    after = function()
      require("config.sidekick").setup()
    end,
  },
}
