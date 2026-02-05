return {
  {
    "blink.cmp",
    for_cat = "completion",
    event = "DeferredUIEnter",
    after = function()
      require("blink.cmp").setup({
        keymap = {
          preset = "default",
          ["<Tab>"] = { "show_documentation", "hide_documentation", "fallback" },
          ["<C-Space>"] = { "show", "fallback" },
          ["<CR>"] = { "accept", "fallback" },
          ["<C-n>"] = {
            "select_next",
            "snippet_forward",
            function() -- sidekick next edit suggestion
              return require("sidekick").nes_jump_or_apply()
            end,
            "fallback",
          },
          ["<C-p>"] = { "select_prev", "snippet_backward", "fallback" },
          ["<C-b>"] = { "scroll_documentation_up", "fallback" },
          ["<C-f>"] = { "scroll_documentation_down", "fallback" },
        },
        sources = {
          default = { "lsp", "path", "buffer" },
        },
        completion = {
          documentation = { auto_show = true },
          trigger = { show_on_insert_on_trigger_character = true },
          list = { selection = { preselect = true, auto_insert = false } },
          menu = { auto_show = true },
        },
      })
    end,
  },
}
