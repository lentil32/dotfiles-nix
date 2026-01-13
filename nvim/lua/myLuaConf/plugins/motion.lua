return {
  {
    "flash.nvim",
    for_cat = "general",
    event = "DeferredUIEnter",
    after = function()
      require("flash").setup({
        search = {
          mode = "search",
        },
        jump = {
          autojump = true,
        },
      })
    end,
    keys = {
      {
        "gs",
        function()
          require("flash").jump()
        end,
        mode = { "n", "x", "o" },
        desc = "Flash",
      },
      {
        "S",
        function()
          require("flash").treesitter()
        end,
        mode = { "n", "x", "o" },
        desc = "Flash Treesitter",
      },
      {
        "r",
        function()
          require("flash").remote()
        end,
        mode = "o",
        desc = "Remote Flash",
      },
      {
        "R",
        function()
          require("flash").treesitter_search()
        end,
        mode = { "o", "x" },
        desc = "Treesitter Search",
      },
      {
        "<c-s>",
        function()
          require("flash").toggle()
        end,
        mode = { "c" },
        desc = "Toggle Flash Search",
      },
    },
  },
  {
    "hop.nvim",
    for_cat = "general",
    event = "DeferredUIEnter",
    after = function()
      require("hop").setup({})
    end,
    keys = {
      {
        "gS",
        function()
          require("hop").hint_words()
        end,
        mode = { "n", "x", "o" },
        desc = "Hop word",
      },
    },
  },
  {
    "nvim-surround",
    for_cat = "general",
    event = "DeferredUIEnter",
    after = function()
      require("nvim-surround").setup({
        keymaps = {
          normal = "s",
          normal_cur = "ss",
          normal_line = "sS",
          normal_cur_line = "sSS",
          visual = "s",
          visual_line = false,
        },
      })
    end,
  },
}
