return {
  {
    "nvim-treesitter",
    for_cat = "treesitter",
    lazy = false,
    after = function()
      local function is_mise_filename(filename)
        return filename:match(".*mise.*%.toml$") ~= nil
      end

      local function is_mise_predicate(_, _, bufnr, _)
        local buf = tonumber(bufnr) or 0
        local filepath = vim.api.nvim_buf_get_name(buf)
        local filename = vim.fn.fnamemodify(filepath, ":t")
        return is_mise_filename(filename)
      end

      require("vim.treesitter.query").add_predicate("is-mise?", is_mise_predicate, {
        force = true,
        all = false,
      })

      require("nvim-treesitter").setup()

      local function attach_treesitter(bufnr, language)
        if not vim.treesitter.language.add(language) then
          return
        end
        vim.treesitter.start(bufnr, language)
        vim.bo[bufnr].indentexpr = "v:lua.require'nvim-treesitter'.indentexpr()"
        vim.wo.foldexpr = "v:lua.vim.treesitter.foldexpr()"
        vim.wo.foldmethod = "expr"
        vim.o.foldlevel = 99
      end

      vim.api.nvim_create_autocmd("FileType", {
        group = vim.api.nvim_create_augroup("myLuaConf_treesitter", { clear = true }),
        callback = function(event)
          local language = vim.treesitter.language.get_lang(event.match)
          if not language then
            return
          end
          attach_treesitter(event.buf, language)
        end,
      })

      if vim.bo.filetype ~= "" then
        local language = vim.treesitter.language.get_lang(vim.bo.filetype)
        if language then
          attach_treesitter(vim.api.nvim_get_current_buf(), language)
        end
      end
    end,
  },
}
