return {
  {
    "orgmode",
    ft = "org",
    after = function()
      require("config.org").setup()
    end,
  },
}
