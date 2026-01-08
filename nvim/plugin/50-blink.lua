local ok, blink = pcall(require, "blink.cmp")
if not ok then
  return
end

blink.setup({
  keymap = {
    preset = "default",
    ["<C-space>"] = { "show", "show_documentation", "hide_documentation" },
    ["<C-e>"] = { "hide" },
    ["<CR>"] = { "accept", "fallback" },
    ["<Tab>"] = {
      "accept",
      "snippet_forward",
      function()
        return require("config.sidekick").nes_jump_or_apply()
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
