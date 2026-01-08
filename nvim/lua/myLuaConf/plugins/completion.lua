return {
  {
    "blink.cmp",
    for_cat = "completion",
    event = "DeferredUIEnter",
    after = function()
      require("blink.cmp").setup({
        keymap = {
          preset = "default",
          ["<C-space>"] = { "show", "show_documentation", "hide_documentation" },
          ["<CR>"] = { "accept", "fallback" },
          ["<Tab>"] = {
            "accept",
            "snippet_forward",
            function()
              local ok, sidekick = pcall(require, "sidekick")
              if ok and sidekick.nes_jump_or_apply then
                return sidekick.nes_jump_or_apply()
              end
              return false
            end,
            "fallback",
          },
          ["<S-Tab>"] = { "select_prev", "snippet_backward", "fallback" },
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
