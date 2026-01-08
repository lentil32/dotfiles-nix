local util = require("myLuaConf.util")

local M = {}

local function snacks()
  return _G.Snacks or require("snacks")
end

function M.delete_current_buffer()
  local Snacks = snacks()
  local buf = vim.api.nvim_get_current_buf()
  local win = vim.api.nvim_get_current_win()
  local map = util.get_var(nil, "oil_last_buf", {})
  if type(map) ~= "table" then
    map = {}
  end
  ---@cast map table<number, number>
  local oil_buf = map[win]
  if oil_buf and oil_buf ~= buf and vim.api.nvim_buf_is_valid(oil_buf) then
    local oil_util = util.try_require("oil.util")
    if oil_util and oil_util.is_oil_bufnr(oil_buf) then
      vim.api.nvim_win_set_buf(0, oil_buf)
      Snacks.bufdelete.delete({ buf = buf })
      return
    end
  end
  Snacks.bufdelete()
end

function M.kill_window_and_buffer()
  local Snacks = snacks()
  local buf = vim.api.nvim_get_current_buf()
  if util.get_var(buf, "snacks_terminal") then
    for _, term in ipairs(Snacks.terminal.list()) do
      if term.buf == buf then
        if term.augroup then
          pcall(vim.api.nvim_clear_autocmds, { group = term.augroup, event = "TermClose", buffer = buf })
        else
          pcall(vim.api.nvim_clear_autocmds, { event = "TermClose", buffer = buf })
        end
        if term.win and vim.api.nvim_win_is_valid(term.win) then
          pcall(vim.api.nvim_win_close, term.win, true)
        end
        pcall(function()
          vim.cmd("bwipeout! " .. buf)
        end)
        return
      end
    end
  end
  if #vim.api.nvim_list_wins() > 1 then
    vim.cmd("close")
  end
  local is_terminal = util.get_buf_opt(buf, "buftype", "") == "terminal"
    or util.get_buf_opt(buf, "filetype", "") == "snacks_terminal"
  Snacks.bufdelete.delete({ buf = buf, force = is_terminal, wipe = is_terminal })
end

return M
