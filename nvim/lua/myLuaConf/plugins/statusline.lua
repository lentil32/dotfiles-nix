local function sidekick_status()
  local ok, status = pcall(require, "sidekick.status")
  if ok then
    return status
  end
  return nil
end

local project = require("myLuaConf.project")

local function copilot_icon()
  return vim.fn.nr2char(0xF4B8) .. " "
end

local function project_icon()
  return vim.fn.nr2char(0xEA62) .. " "
end

local function project_label()
  local root = project.project_root()
  if not root or root == "" then
    return nil
  end
  local name = vim.fn.fnamemodify(root, ":t")
  if name == "" then
    name = vim.fn.fnamemodify(root, ":~")
  end
  return project_icon() .. name
end

local function overseer_status()
  local ok_constants, constants = pcall(require, "overseer.constants")
  local ok_tasks, task_list = pcall(require, "overseer.task_list")
  local ok_util, util = pcall(require, "overseer.util")
  if not (ok_constants and ok_tasks and ok_util) then
    return ""
  end

  local symbols = {
    [constants.STATUS.FAILURE] = "󰅚 ",
    [constants.STATUS.CANCELED] = " ",
    [constants.STATUS.SUCCESS] = "󰄴 ",
    [constants.STATUS.RUNNING] = "󰑮 ",
  }

  local tasks = task_list.list_tasks({ include_ephemeral = true })
  if type(tasks) ~= "table" then
    return ""
  end
  local tasks_by_status = util.tbl_group_by(tasks, "status")
  local pieces = {}
  for _, status in ipairs(constants.STATUS.values) do
    local status_tasks = tasks_by_status[status]
    if symbols[status] and status_tasks then
      table.insert(pieces, string.format("%s%s", symbols[status], #status_tasks))
    end
  end
  if #pieces > 0 then
    return table.concat(pieces, " ")
  end
  return ""
end

return {
  {
    "witch-line",
    for_cat = "general",
    event = "DeferredUIEnter",
    after = function()
      vim.o.laststatus = 2

      require("witch-line").setup({
        cache = { enabled = false },
        statusline = {
          global = {
            "mode",
            "git.branch",
            "git.diff.added",
            "git.diff.modified",
            "git.diff.removed",
            "diagnostic.error",
            "diagnostic.warn",
            "diagnostic.info",
            "diagnostic.hint",
            {
              id = "project.label",
              events = { "BufEnter", "DirChanged" },
              update = function()
                return project_label() or ""
              end,
              style = "Directory",
            },
            "file.name",
            {
              id = "sidekick.status",
              timing = true,
              update = function()
                return copilot_icon()
              end,
              style = function()
                local status = sidekick_status()
                if not status then
                  return nil
                end
                local info = status.get()
                if info then
                  if info.kind == "Error" then
                    return "DiagnosticError"
                  end
                  if info.busy then
                    return "DiagnosticWarn"
                  end
                  return "Special"
                end
                return nil
              end,
              hidden = function()
                local status = sidekick_status()
                return not status or status.get() == nil
              end,
            },
            "%=",
            {
              id = "overseer.status",
              timing = true,
              update = function()
                return overseer_status()
              end,
            },
            {
              id = "file.format",
              events = { "BufEnter", "BufWritePost" },
              update = function()
                local format = vim.bo.fileformat
                return format or ""
              end,
            },
            {
              id = "file.type",
              events = { "BufEnter", "FileType" },
              update = function()
                local filetype = vim.bo.filetype
                return filetype or ""
              end,
            },
            {
              id = "snacks.profiler",
              timing = true,
              update = function()
                local ok, Snacks = pcall(require, "snacks")
                if not ok or not Snacks.profiler or not Snacks.profiler.core then
                  return ""
                end
                if not Snacks.profiler.core.running then
                  return ""
                end
                local icon = "󰈸 "
                if Snacks.profiler.config and Snacks.profiler.config.icons then
                  icon = Snacks.profiler.config.icons.status or icon
                end
                local count = Snacks.profiler.core.events and #Snacks.profiler.core.events or 0
                return string.format("%s %d events", icon, count)
              end,
              style = "DiagnosticError",
            },
            "cursor.progress",
            "cursor.pos",
          },
        },
      })
    end,
  },
}
