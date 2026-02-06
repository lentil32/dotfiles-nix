local wezterm = require("wezterm")
local act = wezterm.action

local function parse_workspace_pane(value)
  if type(value) ~= "string" then
    return nil
  end

  local sep = value:find("|", 1, true)
  if sep == nil then
    return nil
  end

  local workspace = value:sub(1, sep - 1)
  local pane_id = tonumber(value:sub(sep + 1))
  if workspace == "" or pane_id == nil then
    return nil
  end

  return {
    workspace = workspace,
    pane_id = pane_id,
  }
end

local function activate_pane_by_id(target_pane_id)
  local mux = wezterm.mux

  for _, mux_window in ipairs(mux.all_windows()) do
    for _, tab in ipairs(mux_window:tabs()) do
      for _, tab_pane in ipairs(tab:panes()) do
        if tab_pane:pane_id() == target_pane_id then
          tab:activate()
          tab_pane:activate()
          return true
        end
      end
    end
  end

  return false
end

local function switch_to_workspace_and_pane(window, pane, target)
  window:perform_action(act.SwitchToWorkspace({ name = target.workspace }), pane)
  return activate_pane_by_id(target.pane_id)
end

local function shell_quote(value)
  return "'" .. value:gsub("'", "'\\''") .. "'"
end

local function resolve_lemonaid_bin()
  local home = os.getenv("HOME")
  if home == nil then
    return "lemonaid"
  end

  local local_bin = home .. "/.local/bin/lemonaid"
  local handle = io.open(local_bin, "r")
  if handle ~= nil then
    handle:close()
    return local_bin
  end

  return "lemonaid"
end

local lemonaid_bin = resolve_lemonaid_bin()

local function run_lemonaid_swap(current_workspace, current_pane_id)
  local command =
    string.format("%s wezterm swap %s %d", shell_quote(lemonaid_bin), shell_quote(current_workspace), current_pane_id)

  local handle = io.popen(command)
  if handle == nil then
    wezterm.log_error("failed to spawn lemonaid wezterm swap")
    return nil
  end

  local output = handle:read("*a") or ""
  handle:close()
  return parse_workspace_pane((output:gsub("%s+$", "")))
end

wezterm.on("user-var-changed", function(window, pane, name, value)
  if name ~= "switch_workspace_and_pane" then
    return
  end

  local target = parse_workspace_pane(value)
  if target == nil then
    wezterm.log_error("invalid switch_workspace_and_pane payload: " .. tostring(value))
    return
  end

  window:perform_action(
    wezterm.action_callback(function(win, p)
      switch_to_workspace_and_pane(win, p, target)
    end),
    pane
  )
end)

return {
  color_scheme = "Tokyo Night",
  use_fancy_tab_bar = true,
  window_frame = {
    font = wezterm.font({ family = "Iosevka Comfy", weight = "Bold" }),
    font_size = 12.0,
    active_titlebar_bg = "#1a1b26",
    inactive_titlebar_bg = "#16161e",
    active_titlebar_fg = "#c0caf5",
    inactive_titlebar_fg = "#545c7e",
    active_titlebar_border_bottom = "#1a1b26",
    inactive_titlebar_border_bottom = "#16161e",
    button_bg = "#1a1b26",
    button_fg = "#c0caf5",
    button_hover_bg = "#292e42",
    button_hover_fg = "#c0caf5",
  },
  colors = {
    tab_bar = {
      inactive_tab_edge = "#1f2335",
      background = "#16161e",
      active_tab = {
        bg_color = "#1a1b26",
        fg_color = "#7aa2f7",
        intensity = "Bold",
        underline = "None",
        italic = false,
        strikethrough = false,
      },
      inactive_tab = {
        bg_color = "#16161e",
        fg_color = "#545c7e",
      },
      inactive_tab_hover = {
        bg_color = "#292e42",
        fg_color = "#7aa2f7",
        italic = true,
      },
      new_tab = {
        bg_color = "#16161e",
        fg_color = "#7aa2f7",
      },
      new_tab_hover = {
        bg_color = "#292e42",
        fg_color = "#7aa2f7",
        italic = true,
      },
    },
  },
  font = wezterm.font("Iosevka Comfy"),
  font_size = 14.0,
  front_end = "WebGpu",
  webgpu_power_preference = "HighPerformance",
  max_fps = 120,
  animation_fps = 120,
  scrollback_lines = 32768,
  audible_bell = "SystemBeep",
  visual_bell = {
    fade_in_duration_ms = 100,
    fade_out_duration_ms = 200,
    target = "BackgroundColor",
  },
  keys = {
    { key = "Enter", mods = "SHIFT", action = wezterm.action({ SendString = "\x1b\r" }) },
    {
      key = "p",
      mods = "SUPER|CTRL",
      action = wezterm.action_callback(function(window, pane)
        local current_workspace = wezterm.mux.get_active_workspace()
        local target = run_lemonaid_swap(current_workspace, pane:pane_id())
        if target == nil then
          wezterm.log_error("lemonaid wezterm swap returned invalid output")
          return
        end
        switch_to_workspace_and_pane(window, pane, target)
      end),
    },
  },
}
