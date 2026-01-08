return {
  {
    "orgmode",
    for_cat = "org",
    ft = "org",
    after = function()
      require("config.org").setup()
    end,
  },
}
