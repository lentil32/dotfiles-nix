local buffers = require("config.buffer")
local oil = require("config.oil")
local org = require("config.org")
local project = require("config.project")
local util = require("config.util")
local window = require("config.window")

local M = {}

local function snacks()
  return _G.Snacks or require("snacks")
end

local delete_current_buffer = buffers.delete_current_buffer
local kill_window_and_buffer = buffers.kill_window_and_buffer

local function git_init()
  local out = vim.fn.system({ "git", "init" })
  if vim.v.shell_error == 0 then
    vim.notify("Git repository initialized", vim.log.levels.INFO)
  else
    vim.notify(out, vim.log.levels.ERROR)
  end
end

function M.setup()
  local Snacks = snacks()

  if vim.g.neovide then
    Snacks.keymap.set("n", "<D-s>", "<cmd>w<CR>", { desc = "Save" })
    Snacks.keymap.set("v", "<D-c>", '"+y', { desc = "Copy" })
    Snacks.keymap.set("n", "<D-v>", '"+P', { desc = "Paste" })
    Snacks.keymap.set("v", "<D-v>", '"+P', { desc = "Paste" })
    Snacks.keymap.set("c", "<D-v>", "<C-R>+", { desc = "Paste" })
    Snacks.keymap.set("i", "<D-v>", '<ESC>l"+Pli', { desc = "Paste" })
    Snacks.keymap.set("t", "<D-v>", function()
      local chan = vim.b.terminal_job_id
      if chan then
        vim.fn.chansend(chan, vim.fn.getreg("+"))
      end
    end, { desc = "Paste to terminal" })
  end

  -- Emacs-style line navigation in insert mode.
  -- Snacks.keymap.set("i", "<C-a>", "<C-o>^", { desc = "Line start (first non-blank)" })
  -- Snacks.keymap.set("i", "<C-e>", function()
  --   if vim.fn.pumvisible() == 1 then
  --     return "<C-e>"
  --   end
  --   return "<C-o>$"
  -- end, { expr = true, replace_keycodes = true, desc = "Line end" })

  -- Drag lines like Spacemacs drag-stuff.
  Snacks.keymap.set("n", "]e", "<cmd>m .+1<cr>==", { desc = "Move line down" })
  Snacks.keymap.set("n", "[e", "<cmd>m .-2<cr>==", { desc = "Move line up" })
  Snacks.keymap.set("x", "]e", ":m '>+1<cr>gv=gv", { desc = "Move selection down" })
  Snacks.keymap.set("x", "[e", ":m '<-2<cr>gv=gv", { desc = "Move selection up" })

  local term_group = vim.api.nvim_create_augroup("UserTermKeymaps", { clear = true })
  vim.api.nvim_create_autocmd("TermOpen", {
    group = term_group,
    callback = function(args)
      local win = vim.fn.bufwinid(args.buf)
      if win ~= -1 then
        util.set_win_opts(win, { number = false, relativenumber = false })
      else
        vim.opt_local.number = false
        vim.opt_local.relativenumber = false
      end
      Snacks.keymap.set("t", "<Esc>", [[<C-\><C-n>]], {
        buffer = args.buf,
        silent = true,
        desc = "Exit terminal mode",
      })
      Snacks.keymap.set("t", "<C-g>", "<Esc>", {
        buffer = args.buf,
        silent = true,
        desc = "Send Esc to terminal",
      })
      -- Neovide GUI doesn't pass Ctrl-V correctly to terminal; send it explicitly.
      -- Snacks.keymap.set("t", "<C-v>", function()
      --   local chan = vim.b.terminal_job_id
      --   if chan then
      --     vim.fn.chansend(chan, "\x16")
      --   end
      -- end, {
      --   buffer = args.buf,
      --   silent = true,
      --   desc = "Send Ctrl-V to terminal",
      -- })
    end,
  })
end

function M.list()
  local Snacks = snacks()
  local picker = Snacks.picker
  local terminal = Snacks.terminal
  local bufdelete = Snacks.bufdelete
  local keymaps = {}
  local function add(list)
    vim.list_extend(keymaps, list)
  end

  add({
    -- Top-level
    { "<leader>/",     function() picker.grep() end,     desc = "Search project" },
    { "<leader>*",     function() picker.grep({ search = vim.fn.expand("<cword>") }) end, desc = "Search project (word)" },
    { "<leader><Tab>", "<cmd>b#<cr>",                             desc = "Last buffer" },
    { "<leader>'",     function() terminal.toggle() end, desc = "Terminal" },
  })

  add({
    -- File
    { "<leader>f",  group = "file" },
    { "<leader>ff", function() picker.files() end, desc = "Find file" },
    {
      "<leader>fF",
      function()
        local dir = vim.fn.input({
          prompt = "Find files in: ",
          default = vim.fn.getcwd() .. "/",
          completion = "dir",
        })
        if dir == nil or dir == "" then
          return
        end
        picker.files({ cwd = vim.fn.expand(dir) })
      end,
      desc = "Find file (Snacks in dir)"
    },
    {
      "<leader>fj",
      function()
        local path = vim.fn.expand("%:p:h")
        if path == "" then
          path = vim.fn.getcwd()
        end
        oil.open_oil(path)
      end,
      desc = "Jump to directory (Oil)"
    },
    { "<leader>fr",  function() picker.recent() end,                   desc = "Recent files" },
    { "<leader>fs",  "<cmd>w<cr>",                                              desc = "Save" },
    { "<leader>fy",  group = "yank" },
    { "<leader>fyy", function() vim.fn.setreg("+", vim.fn.expand("%:t")) end,   desc = "Filename" },
    { "<leader>fyY", function() vim.fn.setreg("+", vim.fn.expand("%:p")) end,   desc = "Full path" },
    { "<leader>fyd", function() vim.fn.setreg("+", vim.fn.expand("%:p:h")) end, desc = "Directory" },
    { "<leader>fyr", function() vim.fn.setreg("+", vim.fn.expand("%")) end,     desc = "Relative path" },
  })

  add({
    -- Project
    { "<leader>p",  group = "project" },
    { "<leader>pp", function() picker.projects() end, desc = "Switch project" },
    { "<leader>pf", function() picker.files() end,    desc = "Find file" },
    {
      "<leader>pd",
      function()
        picker.files({ cmd = "fd", args = { "--type", "d" } })
      end,
      desc = "Find directory"
    },
    { "<leader>pD", function() oil.open_oil(project.project_root()) end,                                 desc = "Dired (Oil)" },
    { "<leader>pr", function() picker.recent({ filter = { cwd = true } }) end,                  desc = "Recent files" },
    { "<leader>pb", function() picker.buffers({ filter = { cwd = true } }) end,                 desc = "Project buffers" },
    { "<leader>ps", function() picker.grep() end,                                               desc = "Search in project" },
    { "<leader>pR", function() require("grug-far").open({ prefills = { paths = vim.fn.getcwd() } }) end, desc = "Replace in project" },
    { "<leader>p'", function() terminal.toggle() end,                                           desc = "Terminal" },
    {
      "<leader>pk",
      function()
        local cwd = vim.fn.getcwd()
        bufdelete.delete({
          filter = function(buf)
            if not vim.api.nvim_buf_is_loaded(buf) then
              return false
            end
            local name = vim.api.nvim_buf_get_name(buf)
            return name ~= "" and name:find(cwd, 1, true) ~= nil
          end,
        })
      end,
      desc = "Kill project buffers"
    },
    {
      "<leader>pI",
      function()
        require("project.project").set_pwd(vim.fn.getcwd(), "manual")
        vim.notify("Project cache invalidated", vim.log.levels.INFO)
      end,
      desc = "Invalidate cache"
    },
    { "<leader>pv", "<cmd>Neogit<cr>", desc = "Version control" },
  })

  add({
    -- Buffer
    { "<leader>b",  group = "buffer" },
    { "<leader>bb", function() picker.buffers() end,       desc = "Buffers" },
    { "<leader>bj", project.show_project_root,                      desc = "Project root" },
    { "<leader>bd", delete_current_buffer,                          desc = "Delete" },
    { "<leader>bn", "<cmd>bnext<cr>",                               desc = "Next" },
    { "<leader>bp", "<cmd>bprev<cr>",                               desc = "Prev" },
    { "<leader>bs", "<cmd>edit ~/.local/share/nvim/scratch.md<cr>", desc = "Scratch buffer" },
    { "<leader>bt", "<cmd>enew | terminal<cr>",                     desc = "Terminal" },
  })

  add({
    -- Search
    { "<leader>s",  group = "search" },
    { "<leader>sp", function() picker.grep() end,  desc = "Project" },
    { "<leader>ss", function() picker.lines() end, desc = "Buffer" },
  })

  add({
    -- Goto
    { "g",  group = "goto" },
    { "gd", vim.lsp.buf.definition,               desc = "Definition" },
    { "gD", window.goto_definition_other_window, desc = "Definition (other window)" },
    { "gr", vim.lsp.buf.references,               desc = "References" },
  })

  add({
    -- Git
    { "<leader>g",  group = "git" },
    { "<leader>gs", "<cmd>Neogit<cr>",                             desc = "Status" },
    { "<leader>gb", "<cmd>GitBlameToggle<cr>",                     desc = "Blame line" },
    { "<leader>gt", function() picker.git_log_file() end, desc = "Log file" },
    { "<leader>gi", git_init,                                      desc = "Git init" },
    { "<leader>gp", function() picker.gh_pr() end,        desc = "GitHub PRs" },
  })

  add({
    -- Errors/Diagnostics
    { "<leader>e",  group = "errors" },
    { "<leader>el", function() picker.diagnostics_buffer() end,        desc = "List (buffer)" },
    { "<leader>eL", function() picker.diagnostics() end,               desc = "List (project)" },
    { "<leader>en", function() vim.diagnostic.goto_next() end,                  desc = "Next" },
    { "<leader>ep", function() vim.diagnostic.goto_prev() end,                  desc = "Previous" },
    { "<leader>ex", function() vim.diagnostic.open_float() end,                 desc = "Explain" },
    { "<leader>ec", function() vim.diagnostic.reset(0) end,                     desc = "Clear" },
    { "<leader>ed", function() vim.diagnostic.enable(false, { bufnr = 0 }) end, desc = "Disable" },
    { "<leader>ee", function() vim.diagnostic.enable(true, { bufnr = 0 }) end,  desc = "Enable" },
    {
      "<leader>ey",
      function()
        local diag = vim.diagnostic.get(0, { lnum = vim.fn.line(".") - 1 })[1]
        if diag then
          vim.fn.setreg("+", diag.message)
          vim.notify("Copied: " .. diag.message, vim.log.levels.INFO)
        end
      end,
      desc = "Yank message"
    },
  })

  add({
    -- Applications
    { "<leader>a",   group = "applications" },
    { "<leader>ao",  group = "org" },
    { "<leader>aoa", org.org_action("agenda.prompt"),  desc = "Agenda" },
    { "<leader>aoc", org.org_action("capture.prompt"), desc = "Capture" },
  })

  add({
    -- LSP
    { "<leader>l",  group = "lsp" },
    { "<leader>la", vim.lsp.buf.code_action,                    desc = "Action" },
    { "<leader>ld", vim.lsp.buf.definition,                     desc = "Definition" },
    { "<leader>lf", "<cmd>lua require('conform').format()<cr>", desc = "Format" },
    { "<leader>lr", vim.lsp.buf.rename,                         desc = "Rename" },
    { "<leader>lh", vim.lsp.buf.hover,                          desc = "Hover" },
    { "<leader>li", "<cmd>TSToolsOrganizeImports<cr>",          desc = "Organize imports" },
    { "<leader>lu", "<cmd>TSToolsRemoveUnused<cr>",             desc = "Remove unused" },
    { "<leader>lm", "<cmd>TSToolsAddMissingImports<cr>",        desc = "Add missing imports" },
  })

  add({
    -- Major mode leader (Spacemacs ",")
    { ",",  group = "major mode" },
    { ",a", vim.lsp.buf.code_action,                    desc = "Action" },
    { ",d", vim.lsp.buf.definition,                     desc = "Definition" },
    { ",f", "<cmd>lua require('conform').format()<cr>", desc = "Format" },
    { ",r", vim.lsp.buf.rename,                         desc = "Rename" },
    { ",h", vim.lsp.buf.hover,                          desc = "Hover" },
    { ",i", "<cmd>TSToolsOrganizeImports<cr>",          desc = "Organize imports" },
    { ",u", "<cmd>TSToolsRemoveUnused<cr>",             desc = "Remove unused" },
    { ",m", "<cmd>TSToolsAddMissingImports<cr>",        desc = "Add missing imports" },
  })

  add({
    -- Window
    { "<leader>w",  group = "window" },
    { "<leader>wh", "<C-w>h",               desc = "Focus left" },
    { "<leader>wj", "<C-w>j",               desc = "Focus down" },
    { "<leader>wk", "<C-w>k",               desc = "Focus up" },
    { "<leader>wl", "<C-w>l",               desc = "Focus right" },
    { "<leader>ww", "<C-w>w",               desc = "Other window" },
    { "<leader>wH", "<C-w>H",               desc = "Move far left" },
    { "<leader>wJ", "<C-w>J",               desc = "Move far down" },
    { "<leader>wK", "<C-w>K",               desc = "Move far up" },
    { "<leader>wL", "<C-w>L",               desc = "Move far right" },
    { "<leader>wr", "<C-w>r",               desc = "Rotate forward" },
    { "<leader>wR", "<C-w>R",               desc = "Rotate backward" },
    { "<leader>wX", "<C-w>x",               desc = "Exchange" },
    { "<leader>wx", kill_window_and_buffer, desc = "Kill buffer & window" },
    { "<leader>wv", "<cmd>vsplit<cr>",      desc = "Vsplit" },
    { "<leader>w-", "<cmd>split<cr>",       desc = "Split" },
    { "<leader>w=", "<C-w>=",               desc = "Balance" },
    { "<leader>wm", "<C-w>|<C-w>_",         desc = "Maximize" },
    { "<leader>w_", "<C-w>|",               desc = "Maximize horizontally" },
    { "<leader>wd", "<cmd>close<cr>",       desc = "Close" },
    { "<leader>wD", "<cmd>only<cr>",        desc = "Close others" },
  })

  return keymaps
end

return M
