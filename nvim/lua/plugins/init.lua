local lze = require("lze")

--------------------------------------------------------------------------------
-- Helpers
--------------------------------------------------------------------------------

-- Lazy-load orgmode on demand
local function org_action(action)
  return function()
    vim.cmd.packadd("orgmode")
    require("orgmode").setup({
      org_agenda_files = { "~/org/**/*" },
      org_default_notes_file = "~/org/refile.org",
    })
    require("orgmode").action(action)
  end
end

local function open_yazi(path)
  if not path or path == "" then
    return
  end
  if not package.loaded["yazi"] then
    pcall(vim.cmd, "packadd yazi.nvim")
  end
  local ok, yazi = pcall(require, "yazi")
  if ok then
    yazi.yazi(nil, path)
  else
    vim.cmd("edit " .. vim.fn.fnameescape(path))
  end
end

local function dashboard_recent_files_with_yazi(opts)
  return function()
    local items = Snacks.dashboard.sections.recent_files(opts or {})()
    for _, item in ipairs(items) do
      local path = item.file
      item.action = function()
        if path and vim.fn.isdirectory(path) == 1 then
          open_yazi(path)
        else
          vim.cmd("edit " .. vim.fn.fnameescape(path))
        end
      end
    end
    local section = {}
    if opts and opts.padding then
      section.padding = opts.padding
    end
    for _, item in ipairs(items) do
      table.insert(section, item)
    end
    return section
  end
end

local function bat_preview(ctx)
  if vim.fn.executable("bat") ~= 1 then
    return Snacks.picker.preview.file(ctx)
  end

  local path = Snacks.picker.util.path(ctx.item)
  if not path or vim.fn.isdirectory(path) == 1 then
    return Snacks.picker.preview.file(ctx)
  end

  local uv = vim.uv or vim.loop
  local stat = uv.fs_stat(path)
  if not stat or stat.type == "directory" then
    return Snacks.picker.preview.file(ctx)
  end
  local max_size = ctx.picker.opts.previewers.file.max_size or (1024 * 1024)
  if stat.size > max_size then
    return Snacks.picker.preview.file(ctx)
  end

  local cmd = {
    "bat",
    "--style=numbers,changes",
    "--color=always",
    "--paging=never",
  }
  if ctx.item.pos and ctx.item.pos[1] then
    local line = ctx.item.pos[1]
    table.insert(cmd, "--highlight-line")
    table.insert(cmd, tostring(line))
    table.insert(cmd, "--line-range")
    table.insert(cmd, string.format("%d:%d", math.max(1, line - 5), line + 5))
  else
    table.insert(cmd, "--line-range")
    table.insert(cmd, "1:200")
  end
  table.insert(cmd, path)

  return Snacks.picker.preview.cmd(cmd, ctx, { term = true })
end

--------------------------------------------------------------------------------
-- Theme
--------------------------------------------------------------------------------

vim.cmd.colorscheme("modus_vivendi")

--------------------------------------------------------------------------------
-- Startup Plugins
--------------------------------------------------------------------------------

-- Completion (needed for LSP capabilities)
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

-- Search/Replace (needed by yazi.nvim)
require("grug-far").setup({})

-- Project management (projectile-like)
require("project").setup({
  use_lsp = true,
  patterns = { ".git", "package.json", "Cargo.toml", "flake.nix", "Makefile" },
  silent_chdir = true,
  show_hidden = true,
})

-- Dashboard & utilities
require("snacks").setup({
  styles = {
    dashboard = {
      -- Avoid double BufDelete/BufWipeout callbacks in snacks.nvim.
      bo = { bufhidden = "delete" },
    },
    terminal = {
      keys = {
        term_normal = false,
      },
    },
  },
  dashboard = {
    enabled = true,
    preset = {
      keys = {
        { icon = " ", key = "f", desc = "Find File", action = ":lua Snacks.picker.files()" },
        { icon = " ", key = "n", desc = "New File", action = ":ene | startinsert" },
        { icon = " ", key = "g", desc = "Find Text", action = ":lua Snacks.picker.grep()" },
        { icon = " ", key = "r", desc = "Recent Files", action = ":lua Snacks.picker.recent()" },
        { icon = " ", key = "s", desc = "Git Status", action = ":Neogit" },
        { icon = " ", key = "o", desc = "Org Agenda", action = org_action("agenda.prompt") },
        { icon = " ", key = "q", desc = "Quit", action = ":qa" },
      },
      header = [[
 ███╗   ██╗ ███████╗ ██████╗  ██╗   ██╗ ██╗ ███╗   ███╗
 ████╗  ██║ ██╔════╝██╔═══██╗ ██║   ██║ ██║ ████╗ ████║
 ██╔██╗ ██║ █████╗  ██║   ██║ ██║   ██║ ██║ ██╔████╔██║
 ██║╚██╗██║ ██╔══╝  ██║   ██║ ╚██╗ ██╔╝ ██║ ██║╚██╔╝██║
 ██║ ╚████║ ███████╗╚██████╔╝  ╚████╔╝  ██║ ██║ ╚═╝ ██║
 ╚═╝  ╚═══╝ ╚══════╝ ╚═════╝    ╚═══╝   ╚═╝ ╚═╝     ╚═╝]],
    },
    sections = {
      { section = "header" },
      { section = "keys", gap = 1, padding = 1 },
      dashboard_recent_files_with_yazi({ limit = 5, padding = 1 }),
    },
  },
  terminal = {
    enabled = true,
    win = { style = "terminal", position = "float", border = "rounded" },
  },
  picker = {
    enabled = true,
    win = {
      input = {
        keys = {
          ["<Esc>"] = { "cancel", mode = { "i", "n" } },
        },
      },
    },
    sources = {
      files = {
        cmd = "rg",
        preview = bat_preview,
      },
      grep = { preview = bat_preview },
      grep_buffers = { preview = bat_preview },
      recent = { preview = bat_preview },
      projects = {
        patterns = { ".git", "package.json", "Cargo.toml", "flake.nix", "Makefile" },
      },
    },
  },
  gh = { enabled = true },
})

do
  local dashboard = require("snacks.dashboard")
  local dashboard_cls = dashboard.Dashboard
  local orig_size = dashboard_cls.size
  local orig_update = dashboard_cls.update

  function dashboard_cls:size()
    if not self.win or not vim.api.nvim_win_is_valid(self.win) then
      return self._size or { width = 0, height = 0 }
    end
    return orig_size(self)
  end

  function dashboard_cls:update(...)
    if not self.win or not vim.api.nvim_win_is_valid(self.win) then
      return
    end
    if vim.api.nvim_win_get_buf(self.win) ~= self.buf then
      local win = vim.fn.bufwinid(self.buf)
      if win == -1 then
        return
      end
      self.win = win
    end
    return orig_update(self, ...)
  end
end

--------------------------------------------------------------------------------
-- Global Keymaps
--------------------------------------------------------------------------------

-- Terminal
vim.api.nvim_create_autocmd("TermOpen", {
  callback = function(args)
    vim.keymap.set("t", "<Esc>", [[<C-\><C-n>]], {
      buffer = args.buf,
      silent = true,
      desc = "Exit terminal mode",
    })
    vim.keymap.set("t", "<C-g>", "<Esc>", {
      buffer = args.buf,
      silent = true,
      desc = "Send Esc to terminal",
    })
  end,
})

--------------------------------------------------------------------------------
-- Autocmds
--------------------------------------------------------------------------------

-- Show dashboard when closing to empty buffer (Spacemacs-style)
vim.api.nvim_create_autocmd("BufDelete", {
  callback = function()
    vim.schedule(function()
      -- Skip if current buffer is special (floating, nofile, etc.)
      if vim.bo.buftype ~= "" then
        return
      end
      local bufs = vim.tbl_filter(function(b)
        return vim.api.nvim_buf_is_valid(b)
          and vim.bo[b].buflisted
          and vim.api.nvim_buf_get_name(b) ~= ""
      end, vim.api.nvim_list_bufs())
      if #bufs == 0 then
        Snacks.dashboard()
      end
    end)
  end,
})

--------------------------------------------------------------------------------
-- Keybindings (Spacemacs-style via which-key)
--------------------------------------------------------------------------------

local keymaps = {
  -- Top-level
  { "<leader>/", function() Snacks.picker.grep() end, desc = "Search project" },
  { "<leader><Tab>", "<cmd>b#<cr>", desc = "Last buffer" },
  { "<leader>'", function() Snacks.terminal.toggle() end, desc = "Terminal" },

  -- File
  { "<leader>f", group = "file" },
  { "<leader>ff", function() Snacks.picker.files() end, desc = "Find file" },
  { "<leader>fF", function()
      local dir = vim.fn.input({
        prompt = "Find files in: ",
        default = vim.fn.getcwd() .. "/",
        completion = "dir",
      })
      if dir == nil or dir == "" then
        return
      end
      Snacks.picker.files({ cwd = vim.fn.expand(dir) })
    end, desc = "Find file (Snacks in dir)" },
  { "<leader>fj", "<cmd>Yazi<cr>", desc = "Jump to directory" },
  { "<leader>fr", function() Snacks.picker.recent() end, desc = "Recent files" },
  { "<leader>fs", "<cmd>w<cr>", desc = "Save" },
  { "<leader>fy", group = "yank" },
  { "<leader>fyy", function() vim.fn.setreg("+", vim.fn.expand("%:t")) end, desc = "Filename" },
  { "<leader>fyY", function() vim.fn.setreg("+", vim.fn.expand("%:p")) end, desc = "Full path" },
  { "<leader>fyd", function() vim.fn.setreg("+", vim.fn.expand("%:p:h")) end, desc = "Directory" },
  { "<leader>fyr", function() vim.fn.setreg("+", vim.fn.expand("%")) end, desc = "Relative path" },

  -- Project
  { "<leader>p", group = "project" },
  { "<leader>pp", function() Snacks.picker.projects() end, desc = "Switch project" },
  { "<leader>pf", function() Snacks.picker.files() end, desc = "Find file" },
  { "<leader>pd", function()
      Snacks.picker.files({ cmd = "fd", args = { "--type", "d" } })
    end, desc = "Find directory" },
  { "<leader>pD", "<cmd>Yazi cwd<cr>", desc = "Dired (Yazi)" },
  { "<leader>pr", function() Snacks.picker.recent({ filter = { cwd = true } }) end, desc = "Recent files" },
  { "<leader>pb", function() Snacks.picker.buffers({ filter = { cwd = true } }) end, desc = "Project buffers" },
  { "<leader>ps", function() Snacks.picker.grep() end, desc = "Search in project" },
  { "<leader>pR", function() require("grug-far").open({ prefills = { paths = vim.fn.getcwd() } }) end, desc = "Replace in project" },
  { "<leader>p'", function() Snacks.terminal.toggle() end, desc = "Terminal" },
  { "<leader>pk", function()
      local cwd = vim.fn.getcwd()
      for _, buf in ipairs(vim.api.nvim_list_bufs()) do
        if vim.api.nvim_buf_is_loaded(buf) then
          local name = vim.api.nvim_buf_get_name(buf)
          if name:find(cwd, 1, true) then
            vim.api.nvim_buf_delete(buf, { force = false })
          end
        end
      end
    end, desc = "Kill project buffers" },
  { "<leader>pI", function()
      require("project.project").set_pwd(vim.fn.getcwd(), "manual")
      vim.notify("Project cache invalidated", vim.log.levels.INFO)
    end, desc = "Invalidate cache" },
  { "<leader>pv", "<cmd>Neogit<cr>", desc = "Version control" },

  -- Buffer
  { "<leader>b", group = "buffer" },
  { "<leader>bb", function() Snacks.picker.buffers() end, desc = "Buffers" },
  { "<leader>bd", function()
      local buf = vim.api.nvim_get_current_buf()
      local force = vim.bo[buf].buftype == "terminal"
      vim.api.nvim_buf_delete(buf, { force = force })
    end, desc = "Delete" },
  { "<leader>bn", "<cmd>bnext<cr>", desc = "Next" },
  { "<leader>bp", "<cmd>bprev<cr>", desc = "Prev" },
  { "<leader>bs", "<cmd>edit ~/.local/share/nvim/scratch.md<cr>", desc = "Scratch buffer" },
  { "<leader>bt", "<cmd>enew | terminal<cr>", desc = "Terminal" },

  -- Search
  { "<leader>s", group = "search" },
  { "<leader>sp", function() Snacks.picker.grep() end, desc = "Project" },
  { "<leader>ss", function() Snacks.picker.lines() end, desc = "Buffer" },

  -- Git
  { "<leader>g", group = "git" },
  { "<leader>gs", "<cmd>Neogit<cr>", desc = "Status" },
  { "<leader>gi", function() Snacks.picker.gh_issue() end, desc = "GitHub issues" },
  { "<leader>gp", function() Snacks.picker.gh_pr() end, desc = "GitHub PRs" },

  -- Errors/Diagnostics
  { "<leader>e", group = "errors" },
  { "<leader>el", function() Snacks.picker.diagnostics_buffer() end, desc = "List (buffer)" },
  { "<leader>eL", function() Snacks.picker.diagnostics() end, desc = "List (project)" },
  { "<leader>en", function() vim.diagnostic.goto_next() end, desc = "Next" },
  { "<leader>ep", function() vim.diagnostic.goto_prev() end, desc = "Previous" },
  { "<leader>ex", function() vim.diagnostic.open_float() end, desc = "Explain" },
  { "<leader>ec", function() vim.diagnostic.reset(0) end, desc = "Clear" },
  { "<leader>ed", function() vim.diagnostic.enable(false, { bufnr = 0 }) end, desc = "Disable" },
  { "<leader>ee", function() vim.diagnostic.enable(true, { bufnr = 0 }) end, desc = "Enable" },
  { "<leader>ey", function()
      local diag = vim.diagnostic.get(0, { lnum = vim.fn.line(".") - 1 })[1]
      if diag then
        vim.fn.setreg("+", diag.message)
        vim.notify("Copied: " .. diag.message, vim.log.levels.INFO)
      end
    end, desc = "Yank message" },

  -- Applications
  { "<leader>a", group = "applications" },
  { "<leader>ao", group = "org" },
  { "<leader>aoa", org_action("agenda.prompt"), desc = "Agenda" },
  { "<leader>aoc", org_action("capture.prompt"), desc = "Capture" },

  -- LSP
  { "<leader>l", group = "lsp" },
  { "<leader>la", vim.lsp.buf.code_action, desc = "Action" },
  { "<leader>ld", vim.lsp.buf.definition, desc = "Definition" },
  { "<leader>lf", "<cmd>lua require('conform').format()<cr>", desc = "Format" },
  { "<leader>lr", vim.lsp.buf.rename, desc = "Rename" },
  { "<leader>lh", vim.lsp.buf.hover, desc = "Hover" },
  { "<leader>li", "<cmd>TSToolsOrganizeImports<cr>", desc = "Organize imports" },
  { "<leader>lu", "<cmd>TSToolsRemoveUnused<cr>", desc = "Remove unused" },
  { "<leader>lm", "<cmd>TSToolsAddMissingImports<cr>", desc = "Add missing imports" },

  -- Major mode leader (Spacemacs ",")
  { ",", group = "major mode" },
  { ",a", vim.lsp.buf.code_action, desc = "Action" },
  { ",d", vim.lsp.buf.definition, desc = "Definition" },
  { ",f", "<cmd>lua require('conform').format()<cr>", desc = "Format" },
  { ",r", vim.lsp.buf.rename, desc = "Rename" },
  { ",h", vim.lsp.buf.hover, desc = "Hover" },
  { ",i", "<cmd>TSToolsOrganizeImports<cr>", desc = "Organize imports" },
  { ",u", "<cmd>TSToolsRemoveUnused<cr>", desc = "Remove unused" },
  { ",m", "<cmd>TSToolsAddMissingImports<cr>", desc = "Add missing imports" },

  -- Window
  { "<leader>w", group = "window" },
  { "<leader>wh", "<C-w>h", desc = "Focus left" },
  { "<leader>wj", "<C-w>j", desc = "Focus down" },
  { "<leader>wk", "<C-w>k", desc = "Focus up" },
  { "<leader>wl", "<C-w>l", desc = "Focus right" },
  { "<leader>ww", "<C-w>w", desc = "Other window" },
  { "<leader>wH", "<C-w>H", desc = "Move far left" },
  { "<leader>wJ", "<C-w>J", desc = "Move far down" },
  { "<leader>wK", "<C-w>K", desc = "Move far up" },
  { "<leader>wL", "<C-w>L", desc = "Move far right" },
  { "<leader>wr", "<C-w>r", desc = "Rotate forward" },
  { "<leader>wR", "<C-w>R", desc = "Rotate backward" },
  { "<leader>wX", "<C-w>x", desc = "Exchange" },
  { "<leader>wx", function()
      local buf = vim.api.nvim_get_current_buf()
      if #vim.api.nvim_list_wins() > 1 then
        vim.cmd("close")
      end
      local force = vim.bo[buf].buftype == "terminal"
      vim.api.nvim_buf_delete(buf, { force = force })
    end, desc = "Kill buffer & window" },
  { "<leader>wv", "<cmd>vsplit<cr>", desc = "Vsplit" },
  { "<leader>w-", "<cmd>split<cr>", desc = "Split" },
  { "<leader>w=", "<C-w>=", desc = "Balance" },
  { "<leader>wm", "<C-w>|<C-w>_", desc = "Maximize" },
  { "<leader>w_", "<C-w>|", desc = "Maximize horizontally" },
  { "<leader>wd", "<cmd>close<cr>", desc = "Close" },
  { "<leader>wD", "<cmd>only<cr>", desc = "Close others" },
}

--------------------------------------------------------------------------------
-- Lazy Plugins (via lze)
--------------------------------------------------------------------------------

lze.load({
  ------------------------------------------------------------------------------
  -- UI
  ------------------------------------------------------------------------------
  {
    "which-key.nvim",
    event = "DeferredUIEnter",
    after = function()
      local wk = require("which-key")
      wk.setup({ delay = 300 })
      wk.add(keymaps)
    end,
  },

  ------------------------------------------------------------------------------
  -- Navigation
  ------------------------------------------------------------------------------
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

  ------------------------------------------------------------------------------
  -- Git
  ------------------------------------------------------------------------------
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

  ------------------------------------------------------------------------------
  -- Org
  ------------------------------------------------------------------------------
  {
    "orgmode",
    ft = "org",
    after = function()
      require("orgmode").setup({
        org_agenda_files = { "~/org/**/*" },
        org_default_notes_file = "~/org/refile.org",
      })
    end,
  },

  ------------------------------------------------------------------------------
  -- Syntax
  ------------------------------------------------------------------------------
  {
    "nvim-treesitter",
    event = "BufReadPre",
    after = function()
      require("nvim-treesitter.configs").setup({
        highlight = { enable = true },
        indent = { enable = true },
      })
    end,
  },

  ------------------------------------------------------------------------------
  -- LSP & Formatting
  ------------------------------------------------------------------------------
  {
    "nvim-lspconfig",
    event = "BufReadPre",
    after = function()
      local capabilities = require("blink.cmp").get_lsp_capabilities()

      vim.lsp.config("nil_ls", { capabilities = capabilities })
      vim.lsp.config("rust_analyzer", { capabilities = capabilities })
      vim.lsp.config("lua_ls", {
        capabilities = capabilities,
        settings = { Lua = { diagnostics = { globals = { "vim" } } } },
      })
      vim.lsp.config("ruff", { capabilities = capabilities })
      vim.lsp.config("biome", { capabilities = capabilities })

      vim.lsp.enable({ "nil_ls", "rust_analyzer", "lua_ls", "ruff", "biome" })
    end,
  },

  {
    "typescript-tools.nvim",
    ft = { "typescript", "typescriptreact", "javascript", "javascriptreact" },
    after = function()
      local capabilities = require("blink.cmp").get_lsp_capabilities()
      require("typescript-tools").setup({
        capabilities = capabilities,
        settings = {
          tsserver_file_preferences = {
            includeInlayParameterNameHints = "all",
            includeInlayFunctionParameterTypeHints = true,
            includeInlayVariableTypeHints = true,
          },
        },
      })
    end,
  },

  {
    "conform.nvim",
    event = "BufWritePre",
    after = function()
      require("conform").setup({
        formatters_by_ft = {
          javascript = { "biome" },
          typescript = { "biome" },
          javascriptreact = { "biome" },
          typescriptreact = { "biome" },
          json = { "biome" },
        },
        format_on_save = {
          timeout_ms = 500,
          lsp_fallback = true,
        },
      })
    end,
  },
})
