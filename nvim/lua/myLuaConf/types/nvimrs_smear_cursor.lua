---@meta

---@class nvimrs_smear_cursor.SetupOpts
---@field enabled? boolean
---@field time_interval? number
---@field fps? number
---@field simulation_hz? number
---@field max_simulation_steps_per_frame? integer
---@field delay_event_to_smear? number
---@field delay_after_key? number
---@field smear_to_cmd? boolean
---@field smear_insert_mode? boolean
---@field smear_replace_mode? boolean
---@field smear_terminal_mode? boolean
---@field animate_in_insert_mode? boolean
---@field animate_command_line? boolean
---@field vertical_bar_cursor? boolean
---@field vertical_bar_cursor_insert_mode? boolean
---@field horizontal_bar_cursor_replace_mode? boolean
---@field hide_target_hack? boolean
---@field max_kept_windows? integer
---@field windows_zindex? integer
---@field filetypes_disabled? string[]
---@field logging_level? integer
---@field cursor_color? string|nil
---@field cursor_color_insert_mode? string|nil
---@field normal_bg? string|nil
---@field transparent_bg_fallback_color? string
---@field cterm_bg? integer|nil
---@field cterm_cursor_colors? integer[]|nil
---@field smear_between_windows? boolean
---@field smear_between_buffers? boolean
---@field smear_between_neighbor_lines? boolean
---@field min_horizontal_distance_smear? number
---@field min_vertical_distance_smear? number
---@field smear_horizontally? boolean
---@field smear_vertically? boolean
---@field smear_diagonally? boolean
---@field scroll_buffer_space? boolean
---@field anticipation? number
---@field head_response_ms? number
---@field tail_response_ms? number
---@field damping_ratio? number
---@field stop_distance_enter? number
---@field stop_distance_exit? number
---@field stop_velocity_enter? number
---@field stop_hold_frames? integer
---@field max_length? number
---@field max_length_insert_mode? number
---@field trail_duration_ms? number
---@field trail_short_duration_ms? number
---@field trail_size? number
---@field trail_min_distance? number
---@field trail_thickness? number
---@field trail_thickness_x? number
---@field particles_enabled? boolean
---@field particle_max_num? integer
---@field particle_spread? number
---@field particles_per_second? number
---@field particles_per_length? number
---@field particle_max_lifetime? number
---@field particle_lifetime_distribution_exponent? number
---@field particle_max_initial_velocity? number
---@field particle_velocity_from_cursor? number
---@field particle_random_velocity? number
---@field particle_damping? number
---@field particle_gravity? number
---@field min_distance_emit_particles? number
---@field particle_switch_octant_braille? number
---@field particles_over_text? boolean
---@field never_draw_over_target? boolean
---@field color_levels? integer
---@field gamma? number
---@field aa_band_min? number
---@field aa_band_max? number
---@field edge_gate_low? number
---@field edge_gate_high? number
---@field temporal_hysteresis_enter? number
---@field temporal_hysteresis_exit? number

local M = {}

---@return integer
function M.ping() end

---@generic T: table
---@param args T
---@return T
function M.echo(args) end

---@param args table
---@return table
function M.step(args) end

---@param opts? nvimrs_smear_cursor.SetupOpts
function M.setup(opts) end

function M.on_key() end

---@param event string
function M.on_autocmd(event) end

---@param opts? nvimrs_smear_cursor.SetupOpts
function M.toggle(opts) end

---@return string
function M.diagnostics() end

return M
