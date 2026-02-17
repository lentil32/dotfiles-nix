return {
  {
    "neogit",
    for_cat = "git",
    cmd = "Neogit",
    load = function(name)
      vim.cmd.packadd(name)
      vim.cmd.packadd("diffview.nvim")
    end,
    after = function()
      require("neogit").setup({
        kind = "auto",
        integrations = {
          diffview = false,
          fzf_lua = false,
          mini_pick = false,
          snacks = true,
          telescope = false,
        },
        graph_style = "unicode",
        disable_insert_on_commit = true,
        disable_signs = true,
        disable_context_highlighting = true,
        filewatcher = {
          enabled = false,
        },
        commit_editor = {
          kind = "tab",
          show_staged_diff = true,
          staged_diff_split_kind = "vsplit",
        },
        builders = {
          NeogitBranchPopup = function(builder)
            for _, group in ipairs(builder.state.actions) do
              for _, action in ipairs(group) do
                if action.description == "delete" then
                  for i, key in ipairs(action.keys) do
                    if key == "D" then
                      action.keys[i] = "x"
                    end
                  end
                  builder.state.keys["D"] = nil
                  builder.state.keys["x"] = true
                elseif action.description == "pull request" then
                  for i, key in ipairs(action.keys) do
                    if key == "o" then
                      action.keys[i] = "f"
                    end
                  end
                  builder.state.keys["o"] = nil
                  builder.state.keys["f"] = true
                end
              end
            end
          end,
        },
        mappings = {
          popup = {
            ["O"] = "ResetPopup",
            ["X"] = false,
            ["F"] = "PullPopup",
            ["p"] = "PushPopup",
            ["P"] = false,
          },
          status = {
            ["<c-r>"] = false,
            ["gr"] = "RefreshBuffer",
          },
          commit_editor = {
            ["<c-c><c-c>"] = false,
            ["<c-c><c-k>"] = false,
            ["<localleader>c"] = "Submit",
            ["<localleader>k"] = "Abort",
            ["<localleader>q"] = "Close",
            ["<localleader>p"] = "PrevMessage",
            ["<localleader>n"] = "NextMessage",
            ["<localleader>r"] = "ResetMessage",
          },
          rebase_editor = {
            ["<c-c><c-c>"] = false,
            ["<c-c><c-k>"] = false,
            ["gk"] = false,
            ["gj"] = false,
            ["<localleader>"] = "Submit",
            ["<localleader>a"] = "Abort",
            ["<localleader>c"] = "Submit",
            ["<localleader>k"] = "Abort",
            ["ZZ"] = "Submit",
            ["ZQ"] = "Abort",
            ["<m-k>"] = "MoveUp",
            ["<m-j>"] = "MoveDown",
          },
        },
      })
    end,
  },
  {
    "gitsigns.nvim",
    for_cat = "git",
    event = "BufReadPost",
    after = function()
      ---@class GitsignsModule
      ---@field setup fun(config?: table)
      local gitsigns = require("gitsigns") ---@type GitsignsModule
      gitsigns.setup({})
    end,
  },
  {
    "git-blame.nvim",
    for_cat = "git",
    cmd = { "GitBlameToggle", "GitBlameEnable", "GitBlameDisable" },
    after = function()
      require("gitblame").setup({
        enabled = false,
      })
    end,
  },
  {
    "vim-flog",
    for_cat = "git",
    cmd = { "Flog", "Flogsplit" },
  },
}
