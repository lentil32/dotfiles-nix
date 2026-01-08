return {
  {
    "neogit",
    cmd = "Neogit",
    after = function()
      require("neogit").setup({
        integrations = { diffview = false, snacks = false },
        commit_editor = {
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
    event = "BufReadPre",
    after = function()
      require("gitsigns").setup({})
    end,
  },
  {
    "git-blame.nvim",
    cmd = { "GitBlameToggle", "GitBlameEnable", "GitBlameDisable" },
    after = function()
      require("gitblame").setup({
        enabled = false,
      })
    end,
  },
}
