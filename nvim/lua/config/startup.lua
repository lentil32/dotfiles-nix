local M = {}

function M.setup()
  vim.cmd.colorscheme("modus_vivendi")

  require("blink.cmp").setup({
    keymap = {
      preset = "default",
      ["<C-space>"] = { "show", "show_documentation", "hide_documentation" },
      ["<C-e>"] = { "hide" },
      ["<CR>"] = { "accept", "fallback" },
      ["<Tab>"] = { "select_next", "snippet_forward", "fallback" },
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

  require("grug-far").setup({})

  require("project").setup({
    use_lsp = true,
    patterns = { ".git", "package.json", "Cargo.toml", "flake.nix", "Makefile" },
    silent_chdir = true,
    show_hidden = true,
  })

  require("config.snacks").setup()
end

return M
