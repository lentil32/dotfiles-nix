local function ensure_other_window()
  local wins = vim.api.nvim_tabpage_list_wins(0)
  local cur = vim.api.nvim_get_current_win()
  if #wins > 1 then
    for i, win in ipairs(wins) do
      if win == cur then
        return wins[(i % #wins) + 1], false
      end
    end
  end
  vim.cmd("vsplit")
  local new_win = vim.api.nvim_get_current_win()
  vim.api.nvim_set_current_win(cur)
  return new_win, true
end

local function goto_definition_other_window()
  local target, created = ensure_other_window()
  local cur = vim.api.nvim_get_current_win()
  vim.lsp.buf.definition({
    on_list = function(opts)
      local items = opts.items or {}
      if vim.tbl_isempty(items) then
        if created and target and vim.api.nvim_win_is_valid(target) then
          vim.api.nvim_win_close(target, true)
        end
        return
      end

      local item = items[1]
      if target and vim.api.nvim_win_is_valid(target) then
        vim.api.nvim_win_call(target, function()
          if item.bufnr and vim.api.nvim_buf_is_valid(item.bufnr) then
            vim.api.nvim_win_set_buf(0, item.bufnr)
          elseif item.filename then
            vim.cmd("edit " .. vim.fn.fnameescape(item.filename))
          end
          local lnum = item.lnum or 1
          local col = math.max((item.col or 1) - 1, 0)
          pcall(vim.api.nvim_win_set_cursor, 0, { lnum, col })
        end)
      end

      if #items > 1 then
        vim.fn.setqflist({}, " ", { title = opts.title, items = items })
      end

      if vim.api.nvim_win_is_valid(cur) then
        vim.api.nvim_set_current_win(cur)
      end
    end,
  })
end

return function(_, bufnr)
  local function format_buffer()
    local ok, conform = pcall(require, "conform")
    if ok and conform.format then
      conform.format()
    else
      vim.lsp.buf.format()
    end
  end

  local function ts_cmd(cmd)
    return function()
      if vim.fn.exists(":" .. cmd) == 2 then
        vim.cmd(cmd)
      else
        vim.notify(cmd .. " not available", vim.log.levels.WARN)
      end
    end
  end

  local nmap = function(keys, func, desc)
    if desc then
      desc = "LSP: " .. desc
    end
    vim.keymap.set("n", keys, func, { buffer = bufnr, desc = desc })
  end

  nmap("<leader>rn", vim.lsp.buf.rename, "[R]e[n]ame")
  nmap("<leader>la", vim.lsp.buf.code_action, "[L]SP [A]ction")
  nmap("<leader>ld", vim.lsp.buf.definition, "[L]SP [D]efinition")
  nmap("<leader>lr", vim.lsp.buf.rename, "[L]SP [R]ename")
  nmap("<leader>lh", vim.lsp.buf.hover, "[L]SP [H]over")
  nmap("<leader>lf", format_buffer, "[L]SP [F]ormat")
  nmap("<leader>li", ts_cmd("TSToolsOrganizeImports"), "[L]SP Organize [I]mports")
  nmap("<leader>lu", ts_cmd("TSToolsRemoveUnused"), "[L]SP Remove [U]nused")
  nmap("<leader>lm", ts_cmd("TSToolsAddMissingImports"), "[L]SP Add [M]issing Imports")
  nmap("gd", vim.lsp.buf.definition, "[G]oto [D]efinition")
  nmap("gD", goto_definition_other_window, "[G]oto [D]efinition (other window)")
  nmap("gI", vim.lsp.buf.implementation, "[G]oto [I]mplementation")
  nmap("gr", vim.lsp.buf.references, "[G]oto [R]eferences")
  nmap("<leader>ds", vim.lsp.buf.document_symbol, "[D]ocument [S]ymbols")
  nmap("<leader>ws", vim.lsp.buf.workspace_symbol, "[W]orkspace [S]ymbols")
  nmap("<leader>D", vim.lsp.buf.type_definition, "Type [D]efinition")
  nmap("K", vim.lsp.buf.hover, "Hover Documentation")
  nmap("<C-k>", vim.lsp.buf.signature_help, "Signature Documentation")
  nmap("<leader>lD", vim.lsp.buf.declaration, "[L]SP [D]eclaration")
  nmap("<leader>wa", vim.lsp.buf.add_workspace_folder, "[W]orkspace [A]dd Folder")
  nmap("<leader>wr", vim.lsp.buf.remove_workspace_folder, "[W]orkspace [R]emove Folder")
  nmap("<leader>wl", function()
    print(vim.inspect(vim.lsp.buf.list_workspace_folders()))
  end, "[W]orkspace [L]ist Folders")

  vim.keymap.set("n", "<localleader>a", vim.lsp.buf.code_action, { buffer = bufnr, desc = "Action" })
  vim.keymap.set("n", "<localleader>d", vim.lsp.buf.definition, { buffer = bufnr, desc = "Definition" })
  vim.keymap.set("n", "<localleader>f", format_buffer, { buffer = bufnr, desc = "Format" })
  vim.keymap.set("n", "<localleader>r", vim.lsp.buf.rename, { buffer = bufnr, desc = "Rename" })
  vim.keymap.set("n", "<localleader>h", vim.lsp.buf.hover, { buffer = bufnr, desc = "Hover" })
  vim.keymap.set("n", "<localleader>i", ts_cmd("TSToolsOrganizeImports"), { buffer = bufnr, desc = "Organize imports" })
  vim.keymap.set("n", "<localleader>u", ts_cmd("TSToolsRemoveUnused"), { buffer = bufnr, desc = "Remove unused" })
  vim.keymap.set("n", "<localleader>m", ts_cmd("TSToolsAddMissingImports"), { buffer = bufnr, desc = "Add missing imports" })

  vim.api.nvim_buf_create_user_command(bufnr, "Format", function()
    vim.lsp.buf.format()
  end, { desc = "Format current buffer with LSP" })
end
