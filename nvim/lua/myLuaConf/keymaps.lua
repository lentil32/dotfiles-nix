local buffers = require("myLuaConf.buffer")
local oil = require("myLuaConf.oil")
local project = require("myLuaConf.project")
local readline = require("myLuaConf.readline")
local util = require("myLuaConf.util")

local M = {}

local function snacks()
  return require("snacks")
end

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

  if util.get_var(nil, "neovide") then
    Snacks.keymap.set("n", "<D-s>", "<cmd>w<CR>", { desc = "Save" })
    Snacks.keymap.set("v", "<D-c>", '"+y', { desc = "Copy" })
    Snacks.keymap.set("n", "<D-v>", '"+P', { desc = "Paste" })
    Snacks.keymap.set("v", "<D-v>", '"+P', { desc = "Paste" })
    Snacks.keymap.set("c", "<D-v>", "<C-R>+", { desc = "Paste" })
    Snacks.keymap.set("i", "<D-v>", '<ESC>l"+Pli', { desc = "Paste" })
    Snacks.keymap.set("t", "<D-v>", function()
      local chan = tonumber(util.get_var(nil, "terminal_job_id"))
      if chan and chan > 0 then
        vim.fn.chansend(chan, vim.fn.getreg("+"))
      end
    end, { desc = "Paste to terminal" })
  end

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
        util.set_buf_opts(args.buf, { number = false, relativenumber = false })
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
    end,
  })

  -- Emacs/readline-style bindings (insert).
  Snacks.keymap.set("i", "<C-a>", readline.beginning_of_line, { desc = "Beginning of line" })
  Snacks.keymap.set("i", "<C-e>", readline.end_of_line, { desc = "End of line" })
  Snacks.keymap.set("i", "<C-t>", readline.transpose_chars, { desc = "Transpose chars" })
  Snacks.keymap.set("i", "<M-f>", readline.forward_word, { desc = "Forward word" })
  Snacks.keymap.set("i", "<M-b>", readline.backward_word, { desc = "Backward word" })
  Snacks.keymap.set("i", "<M-d>", readline.kill_word, { desc = "Kill word" })
end

function M.list()
  local Snacks = snacks()
  local picker = Snacks.picker
  local bufdelete = Snacks.bufdelete
  local function run_shell(cwd)
    local dir = cwd or vim.fn.getcwd()
    Snacks.input.input({ prompt = "Shell command" }, function(value)
      local cmd = vim.trim(value or "")
      if cmd == "" then
        return
      end
      Snacks.terminal.open(cmd, {
        cwd = dir,
        win = {
          position = "bottom",
          keys = {
            term_normal = { "<esc>", "close", mode = "t", desc = "Close terminal" },
          },
        },
        interactive = false,
      })
    end)
  end
  local function project_root_or_warn()
    local root = project.project_root()
    if not root or root == "" then
      vim.notify("No project root found", vim.log.levels.WARN)
      return nil
    end
    return root
  end
  local keymaps = {}
  local function add(list)
    vim.list_extend(keymaps, list)
  end

  add({
    -- Top-level
    {
      "<leader>/",
      function()
        local root = project_root_or_warn()
        if not root then
          return
        end
        picker.grep({ cwd = root })
      end,
      desc = "Search project",
    },
    {
      "<leader>*",
      function()
        local root = project_root_or_warn()
        if not root then
          return
        end
        picker.grep({ cwd = root, search = vim.fn.expand("<cword>") })
      end,
      desc = "Search project (word)",
    },
    { "<leader><Tab>", "<cmd>b#<cr>", desc = "Last buffer" },
    {
      "<leader>'",
      function()
        Snacks.terminal.toggle()
      end,
      desc = "Terminal",
    },
    {
      "&",
      function()
        run_shell(vim.fn.getcwd())
      end,
      desc = "Shell command",
    },
  })

  add({
    -- File
    { "<leader>f", group = "file" },
    {
      "<leader>ff",
      function()
        picker.files()
      end,
      desc = "Find file",
    },
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
      desc = "Find file (Snacks in dir)",
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
      desc = "Jump to directory (Oil)",
    },
    {
      "<leader>fr",
      function()
        picker.recent()
      end,
      desc = "Recent files",
    },
    { "<leader>fs", "<cmd>w<cr>", desc = "Save" },
    { "<leader>fy", group = "yank" },
    {
      "<leader>fyy",
      function()
        vim.fn.setreg("+", vim.fn.expand("%:t"))
      end,
      desc = "Filename",
    },
    {
      "<leader>fyY",
      function()
        vim.fn.setreg("+", vim.fn.expand("%:p"))
      end,
      desc = "Full path",
    },
    {
      "<leader>fyd",
      function()
        vim.fn.setreg("+", vim.fn.expand("%:p:h"))
      end,
      desc = "Directory",
    },
    {
      "<leader>fyr",
      function()
        vim.fn.setreg("+", vim.fn.expand("%"))
      end,
      desc = "Relative path",
    },
  })

  add({
    -- Project
    { "<leader>p", group = "project" },
    {
      "<leader>pp",
      function()
        picker.projects()
      end,
      desc = "Switch project",
    },
    {
      "<leader>pf",
      function()
        local root = project_root_or_warn()
        if not root then
          return
        end
        picker.files({ cwd = root })
      end,
      desc = "Find file",
    },
    {
      "<leader>p&",
      function()
        local root = project_root_or_warn()
        if not root then
          return
        end
        run_shell(root)
      end,
      desc = "Shell command (project)",
    },
    {
      "<leader>pd",
      function()
        local root = project_root_or_warn()
        if not root then
          return
        end
        picker.files({ cwd = root, cmd = "fd", args = { "--type", "d" } })
      end,
      desc = "Find directory",
    },
    {
      "<leader>pD",
      function()
        local root = project_root_or_warn()
        if not root then
          return
        end
        oil.open_oil(root)
      end,
      desc = "Dired (Oil)",
    },
    {
      "<leader>pr",
      function()
        local root = project_root_or_warn()
        if not root then
          return
        end
        picker.recent({ filter = { cwd = root } })
      end,
      desc = "Recent files",
    },
    {
      "<leader>pb",
      function()
        local root = project_root_or_warn()
        if not root then
          return
        end
        picker.buffers({ filter = { cwd = root } })
      end,
      desc = "Project buffers",
    },
    {
      "<leader>ps",
      function()
        local root = project_root_or_warn()
        if not root then
          return
        end
        picker.grep({ cwd = root })
      end,
      desc = "Search in project",
    },
    {
      "<leader>pR",
      function()
        local root = project_root_or_warn()
        if not root then
          return
        end
        require("grug-far").open({ prefills = { paths = root } })
      end,
      desc = "Replace in project",
    },
    {
      "<leader>p'",
      function()
        local root = project_root_or_warn()
        if not root then
          return
        end
        Snacks.terminal.toggle(nil, { cwd = root })
      end,
      desc = "Terminal",
    },
    {
      "<leader>pk",
      function()
        local cwd = project_root_or_warn()
        if not cwd then
          return
        end
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
      desc = "Kill project buffers",
    },
    {
      "<leader>pv",
      function()
        local root = project_root_or_warn()
        if not root then
          return
        end
        vim.cmd("Neogit cwd=" .. vim.fn.fnameescape(root))
      end,
      desc = "Version control",
    },
  })

  add({
    -- Buffer
    { "<leader>b", group = "buffer" },
    {
      "<leader>bb",
      function()
        picker.buffers()
      end,
      desc = "Buffers",
    },
    { "<leader>bj", project.show_project_root, desc = "Project root" },
    { "<leader>bd", buffers.delete_current_buffer, desc = "Delete" },
    { "<leader>bn", "<cmd>bnext<cr>", desc = "Next" },
    { "<leader>bp", "<cmd>bprev<cr>", desc = "Prev" },
    { "<leader>bs", "<cmd>edit ~/.local/share/nvim/scratch.md<cr>", desc = "Scratch buffer" },
    {
      "<leader>bt",
      function()
        Snacks.terminal.toggle()
      end,
      desc = "Terminal",
    },
  })

  add({
    -- Search
    { "<leader>s", group = "search" },
    {
      "<leader>sp",
      function()
        local root = project_root_or_warn()
        if not root then
          return
        end
        picker.grep({ cwd = root })
      end,
      desc = "Project",
    },
    {
      "<leader>ss",
      function()
        picker.lines()
      end,
      desc = "Buffer",
    },
  })

  add({
    -- Errors/Diagnostics
    { "<leader>e", group = "errors" },
    {
      "<leader>el",
      function()
        picker.diagnostics_buffer()
      end,
      desc = "List (buffer)",
    },
    {
      "<leader>eL",
      function()
        picker.diagnostics()
      end,
      desc = "List (project)",
    },
    {
      "<leader>en",
      function()
        vim.diagnostic.goto_next()
      end,
      desc = "Next",
    },
    {
      "<leader>ep",
      function()
        vim.diagnostic.goto_prev()
      end,
      desc = "Previous",
    },
    {
      "<leader>ex",
      function()
        vim.diagnostic.open_float()
      end,
      desc = "Explain",
    },
    {
      "<leader>ec",
      function()
        vim.diagnostic.reset(0)
      end,
      desc = "Clear",
    },
    {
      "<leader>ed",
      function()
        vim.diagnostic.enable(false, { bufnr = 0 })
      end,
      desc = "Disable",
    },
    {
      "<leader>ee",
      function()
        vim.diagnostic.enable(true, { bufnr = 0 })
      end,
      desc = "Enable",
    },
    {
      "<leader>ey",
      function()
        local diag = vim.diagnostic.get(0, { lnum = vim.fn.line(".") - 1 })[1]
        if diag then
          vim.fn.setreg("+", diag.message)
          vim.notify("Copied: " .. diag.message, vim.log.levels.INFO)
        end
      end,
      desc = "Yank message",
    },
  })

  add({
    -- Window
    { "<leader>w", group = "window" },
    { "<leader>wx", buffers.kill_window_and_buffer, desc = "Close window" },
    { "<leader>wo", "<cmd>only<cr>", desc = "Only window" },
    { "<leader>wD", "<cmd>only<cr>", desc = "Close others" },
    { "<leader>wd", "<cmd>close<cr>", desc = "Close" },
    { "<leader>ws", "<cmd>split<cr>", desc = "Split below" },
    { "<leader>w-", "<cmd>split<cr>", desc = "Split below" },
    { "<leader>wv", "<cmd>vsplit<cr>", desc = "Split right" },
    { "<leader>w=", "<cmd>wincmd =<cr>", desc = "Balance windows" },
    { "<leader>wm", "<C-w>|<C-w>_", desc = "Maximize" },
    { "<leader>w_", "<C-w>|", desc = "Maximize horizontally" },
    { "<leader>wX", "<C-w>x", desc = "Exchange" },
    { "<leader>wr", "<C-w>r", desc = "Rotate forward" },
    { "<leader>wR", "<C-w>R", desc = "Rotate backward" },
    { "<leader>ww", "<cmd>wincmd w<cr>", desc = "Next window" },
    { "<leader>wh", "<cmd>wincmd h<cr>", desc = "Left window" },
    { "<leader>wj", "<cmd>wincmd j<cr>", desc = "Lower window" },
    { "<leader>wk", "<cmd>wincmd k<cr>", desc = "Upper window" },
    { "<leader>wl", "<cmd>wincmd l<cr>", desc = "Right window" },
    { "<leader>wH", "<C-w>H", desc = "Move far left" },
    { "<leader>wJ", "<C-w>J", desc = "Move far down" },
    { "<leader>wK", "<C-w>K", desc = "Move far up" },
    { "<leader>wL", "<C-w>L", desc = "Move far right" },
    { "<leader>w0", "<cmd>wincmd =<cr>", desc = "Reset layout" },
  })

  add({
    -- Applications
    { "<leader>a", group = "applications" },
    { "<leader>ao", group = "org" },
  })

  add({
    -- Org
    { "<leader>o", group = "org" },
  })

  add({
    -- LSP (buffer-local mappings in on_attach)
    { "<leader>l", group = "lsp" },
  })

  add({
    -- Major mode leader
    { "<localleader>", group = "major mode" },
  })

  add({
    -- Git
    { "<leader>g", group = "git" },
    { "<leader>gs", "<cmd>Neogit<cr>", desc = "Status" },
    { "<leader>gb", "<cmd>GitBlameToggle<cr>", desc = "Blame line" },
    {
      "<leader>gt",
      function()
        picker.git_log_file()
      end,
      desc = "Log file",
    },
    { "<leader>gi", git_init, desc = "Init repo" },
    {
      "<leader>gI",
      function()
        picker.gh_issue()
      end,
      desc = "GitHub issues",
    },
    {
      "<leader>gp",
      function()
        picker.gh_pr()
      end,
      desc = "GitHub PRs",
    },
  })

  return keymaps
end

return M
