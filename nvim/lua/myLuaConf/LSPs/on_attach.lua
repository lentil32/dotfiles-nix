local plugin_util = require("rs_plugin_util")

return function(_, bufnr)
  local function format_buffer()
    ---@module "conform"
    local conform = require("conform")
    conform.format()
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
  nmap("gD", plugin_util.goto_definition_other_window, "[G]oto [D]efinition (other window)")
  nmap("gI", vim.lsp.buf.implementation, "[G]oto [I]mplementation")
  nmap("gr", vim.lsp.buf.references, "[G]oto [R]eferences")
  nmap("<leader>ds", vim.lsp.buf.document_symbol, "[D]ocument [S]ymbols")
  nmap("<leader>lws", vim.lsp.buf.workspace_symbol, "[L]SP [W]orkspace [S]ymbols")
  nmap("<leader>D", vim.lsp.buf.type_definition, "Type [D]efinition")
  nmap("K", vim.lsp.buf.hover, "Hover Documentation")
  nmap("<C-k>", vim.lsp.buf.signature_help, "Signature Documentation")
  nmap("<leader>lD", vim.lsp.buf.declaration, "[L]SP [D]eclaration")
  nmap("<leader>lwa", vim.lsp.buf.add_workspace_folder, "[L]SP [W]orkspace [A]dd Folder")
  nmap("<leader>lwr", vim.lsp.buf.remove_workspace_folder, "[L]SP [W]orkspace [R]emove Folder")
  nmap("<leader>lwl", function()
    print(vim.inspect(vim.lsp.buf.list_workspace_folders()))
  end, "[L]SP [W]orkspace [L]ist Folders")

  vim.keymap.set("n", "<localleader>a", vim.lsp.buf.code_action, { buffer = bufnr, desc = "Action" })
  vim.keymap.set("n", "<localleader>d", vim.lsp.buf.definition, { buffer = bufnr, desc = "Definition" })
  vim.keymap.set("n", "<localleader>f", format_buffer, { buffer = bufnr, desc = "Format" })
  vim.keymap.set("n", "<localleader>r", vim.lsp.buf.rename, { buffer = bufnr, desc = "Rename" })
  vim.keymap.set("n", "<localleader>h", vim.lsp.buf.hover, { buffer = bufnr, desc = "Hover" })
  vim.keymap.set("n", "<localleader>i", ts_cmd("TSToolsOrganizeImports"), { buffer = bufnr, desc = "Organize imports" })
  vim.keymap.set("n", "<localleader>u", ts_cmd("TSToolsRemoveUnused"), { buffer = bufnr, desc = "Remove unused" })
  vim.keymap.set(
    "n",
    "<localleader>m",
    ts_cmd("TSToolsAddMissingImports"),
    { buffer = bufnr, desc = "Add missing imports" }
  )

  vim.api.nvim_buf_create_user_command(bufnr, "Format", function()
    vim.lsp.buf.format()
  end, { desc = "Format current buffer with LSP" })
end
