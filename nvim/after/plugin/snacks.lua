local util = require("snacks.util")

if util.var(nil, "snacks_patch_applied", false) then
  return
end
vim.g.snacks_patch_applied = true

do
  if not util.var(nil, "snacks_image_unhide_patch", false) then
    vim.g.snacks_image_unhide_patch = true
    local placement = require("snacks.image.placement")
    local orig_update = placement.update
    function placement:update()
      if self.hidden and self:ready() and #self:wins() > 0 then
        self.hidden = false
      end
      return orig_update(self)
    end
  end
end

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

  function dashboard_cls:update()
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
    return orig_update(self)
  end
end
