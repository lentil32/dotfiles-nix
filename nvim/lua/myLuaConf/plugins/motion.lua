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
    beforeAll = function()
      vim.g.nvim_surround_no_normal_mappings = true
      vim.g.nvim_surround_no_visual_mappings = true
    end,
    after = function()
      require("nvim-surround").setup()
    end,
    keys = {
      {
        "s",
        "<Plug>(nvim-surround-normal)",
        mode = "n",
        desc = "Add a surrounding pair around a motion",
      },
      {
        "ss",
        "<Plug>(nvim-surround-normal-cur)",
        mode = "n",
        desc = "Add a surrounding pair around the current line",
      },
      {
        "sS",
        "<Plug>(nvim-surround-normal-line)",
        mode = "n",
        desc = "Add a surrounding pair around a motion, on new lines",
      },
      {
        "sSS",
        "<Plug>(nvim-surround-normal-cur-line)",
        mode = "n",
        desc = "Add a surrounding pair around the current line, on new lines",
      },
      {
        "s",
        "<Plug>(nvim-surround-visual)",
        mode = "x",
        desc = "Add a surrounding pair around a visual selection",
      },
      {
        "ds",
        "<Plug>(nvim-surround-delete)",
        mode = "n",
        desc = "Delete a surrounding pair",
      },
      {
        "cs",
        "<Plug>(nvim-surround-change)",
        mode = "n",
        desc = "Change a surrounding pair",
      },
      {
        "cS",
        "<Plug>(nvim-surround-change-line)",
        mode = "n",
        desc = "Change a surrounding pair, putting replacements on new lines",
      },
    },
  },
}
