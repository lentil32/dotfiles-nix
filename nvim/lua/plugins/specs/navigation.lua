return {
  {
    "yazi.nvim",
    cmd = "Yazi",
    after = function()
      require("yazi").setup({
        open_for_directories = true,
        integrations = {
          grep_in_directory = "snacks.picker",
          grep_in_selected_files = "snacks.picker",
        },
      })
    end,
  },
}
