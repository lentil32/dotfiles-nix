return {
  {
    "neogit",
    cmd = "Neogit",
    after = function()
      require("neogit").setup({
        integrations = { diffview = true },
        mappings = {
          popup = {
            ["O"] = "ResetPopup",
            ["X"] = false,
            ["F"] = "PullPopup",
            ["p"] = "PushPopup",
            ["P"] = false,
          },
          status = {
            ["gr"] = "RefreshBuffer",
          },
          commit_editor = {
            ["<localleader>c"] = "Submit",
            ["<localleader>k"] = "Abort",
            ["<localleader>q"] = "Close",
            ["<localleader>p"] = "PrevMessage",
            ["<localleader>n"] = "NextMessage",
            ["<localleader>r"] = "ResetMessage",
          },
          commit_editor_I = {
            ["<localleader>c"] = "Submit",
            ["<localleader>k"] = "Abort",
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
}
