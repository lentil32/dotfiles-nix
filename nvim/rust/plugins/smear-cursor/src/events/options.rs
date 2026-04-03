use crate::config::BufferPerfMode;
use crate::lua::ParsedOptionalChange;
use crate::lua::bool_from_object;
use crate::lua::f64_from_object;
use crate::lua::i64_from_object;
use crate::lua::invalid_key;
use crate::lua::parse_indexed_objects;
use crate::lua::parse_optional_change_with as parse_optional_change_value_with;
use crate::lua::parse_optional_with as parse_optional_value_with;
use crate::lua::string_from_object;
use crate::state::ColorOptionsPatch;
use crate::state::CtermCursorColorsPatch;
use crate::state::MotionOptionsPatch;
use crate::state::OptionalChange;
use crate::state::ParticleOptionsPatch;
use crate::state::RenderingOptionsPatch;
use crate::state::RuntimeOptionsPatch;
use crate::state::RuntimeState;
use crate::state::RuntimeSwitchesPatch;
use crate::state::SmearBehaviorPatch;
use nvim_oxi::Dictionary;
use nvim_oxi::Object;
use nvim_oxi::Result;
use nvim_oxi::String as NvimString;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OptionKey {
    Enabled,
    TimeInterval,
    Fps,
    SimulationHz,
    MaxSimulationStepsPerFrame,
    DelayEventToSmear,
    SmearToCmd,
    SmearInsertMode,
    SmearReplaceMode,
    SmearTerminalMode,
    AnimateInInsertMode,
    AnimateCommandLine,
    VerticalBarCursor,
    VerticalBarCursorInsertMode,
    HorizontalBarCursorReplaceMode,
    HideTargetHack,
    MaxKeptWindows,
    WindowsZindex,
    BufferPerfMode,
    FiletypesDisabled,
    LoggingLevel,
    CursorColor,
    CursorColorInsertMode,
    NormalBg,
    TransparentBgFallbackColor,
    CtermBg,
    CtermCursorColors,
    SmearBetweenWindows,
    SmearBetweenBuffers,
    SmearBetweenNeighborLines,
    MinHorizontalDistanceSmear,
    MinVerticalDistanceSmear,
    SmearHorizontally,
    SmearVertically,
    SmearDiagonally,
    ScrollBufferSpace,
    Anticipation,
    HeadResponseMs,
    DampingRatio,
    TailResponseMs,
    StopDistanceEnter,
    StopDistanceExit,
    StopVelocityEnter,
    StopHoldFrames,
    MaxLength,
    MaxLengthInsertMode,
    TrailDurationMs,
    TrailMinDistance,
    TrailThickness,
    TrailThicknessX,
    ParticlesEnabled,
    ParticleMaxNum,
    ParticleSpread,
    ParticlesPerSecond,
    ParticlesPerLength,
    ParticleMaxLifetime,
    ParticleLifetimeDistributionExponent,
    ParticleMaxInitialVelocity,
    ParticleVelocityFromCursor,
    ParticleRandomVelocity,
    ParticleDamping,
    ParticleGravity,
    MinDistanceEmitParticles,
    ParticleSwitchOctantBraille,
    ParticlesOverText,
    NeverDrawOverTarget,
    ColorLevels,
    Gamma,
    TailDurationMs,
    SpatialCoherenceWeight,
    TemporalStabilityWeight,
    TopKPerCell,
}

impl OptionKey {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Enabled => "enabled",
            Self::TimeInterval => "time_interval",
            Self::Fps => "fps",
            Self::SimulationHz => "simulation_hz",
            Self::MaxSimulationStepsPerFrame => "max_simulation_steps_per_frame",
            Self::DelayEventToSmear => "delay_event_to_smear",
            Self::SmearToCmd => "smear_to_cmd",
            Self::SmearInsertMode => "smear_insert_mode",
            Self::SmearReplaceMode => "smear_replace_mode",
            Self::SmearTerminalMode => "smear_terminal_mode",
            Self::AnimateInInsertMode => "animate_in_insert_mode",
            Self::AnimateCommandLine => "animate_command_line",
            Self::VerticalBarCursor => "vertical_bar_cursor",
            Self::VerticalBarCursorInsertMode => "vertical_bar_cursor_insert_mode",
            Self::HorizontalBarCursorReplaceMode => "horizontal_bar_cursor_replace_mode",
            Self::HideTargetHack => "hide_target_hack",
            Self::MaxKeptWindows => "max_kept_windows",
            Self::WindowsZindex => "windows_zindex",
            Self::BufferPerfMode => "buffer_perf_mode",
            Self::FiletypesDisabled => "filetypes_disabled",
            Self::LoggingLevel => "logging_level",
            Self::CursorColor => "cursor_color",
            Self::CursorColorInsertMode => "cursor_color_insert_mode",
            Self::NormalBg => "normal_bg",
            Self::TransparentBgFallbackColor => "transparent_bg_fallback_color",
            Self::CtermBg => "cterm_bg",
            Self::CtermCursorColors => "cterm_cursor_colors",
            Self::SmearBetweenWindows => "smear_between_windows",
            Self::SmearBetweenBuffers => "smear_between_buffers",
            Self::SmearBetweenNeighborLines => "smear_between_neighbor_lines",
            Self::MinHorizontalDistanceSmear => "min_horizontal_distance_smear",
            Self::MinVerticalDistanceSmear => "min_vertical_distance_smear",
            Self::SmearHorizontally => "smear_horizontally",
            Self::SmearVertically => "smear_vertically",
            Self::SmearDiagonally => "smear_diagonally",
            Self::ScrollBufferSpace => "scroll_buffer_space",
            Self::Anticipation => "anticipation",
            Self::HeadResponseMs => "head_response_ms",
            Self::DampingRatio => "damping_ratio",
            Self::TailResponseMs => "tail_response_ms",
            Self::StopDistanceEnter => "stop_distance_enter",
            Self::StopDistanceExit => "stop_distance_exit",
            Self::StopVelocityEnter => "stop_velocity_enter",
            Self::StopHoldFrames => "stop_hold_frames",
            Self::MaxLength => "max_length",
            Self::MaxLengthInsertMode => "max_length_insert_mode",
            Self::TrailDurationMs => "trail_duration_ms",
            Self::TrailMinDistance => "trail_min_distance",
            Self::TrailThickness => "trail_thickness",
            Self::TrailThicknessX => "trail_thickness_x",
            Self::ParticlesEnabled => "particles_enabled",
            Self::ParticleMaxNum => "particle_max_num",
            Self::ParticleSpread => "particle_spread",
            Self::ParticlesPerSecond => "particles_per_second",
            Self::ParticlesPerLength => "particles_per_length",
            Self::ParticleMaxLifetime => "particle_max_lifetime",
            Self::ParticleLifetimeDistributionExponent => "particle_lifetime_distribution_exponent",
            Self::ParticleMaxInitialVelocity => "particle_max_initial_velocity",
            Self::ParticleVelocityFromCursor => "particle_velocity_from_cursor",
            Self::ParticleRandomVelocity => "particle_random_velocity",
            Self::ParticleDamping => "particle_damping",
            Self::ParticleGravity => "particle_gravity",
            Self::MinDistanceEmitParticles => "min_distance_emit_particles",
            Self::ParticleSwitchOctantBraille => "particle_switch_octant_braille",
            Self::ParticlesOverText => "particles_over_text",
            Self::NeverDrawOverTarget => "never_draw_over_target",
            Self::ColorLevels => "color_levels",
            Self::Gamma => "gamma",
            Self::TailDurationMs => "tail_duration_ms",
            Self::SpatialCoherenceWeight => "spatial_coherence_weight",
            Self::TemporalStabilityWeight => "temporal_stability_weight",
            Self::TopKPerCell => "top_k_per_cell",
        }
    }
}

#[derive(Clone, Copy)]
struct OptionSpec {
    key: OptionKey,
    parse_and_apply: fn(&Dictionary, &mut RuntimeOptionsPatch, OptionKey) -> Result<()>,
}

impl OptionSpec {
    const fn new(
        key: OptionKey,
        parse_and_apply: fn(&Dictionary, &mut RuntimeOptionsPatch, OptionKey) -> Result<()>,
    ) -> Self {
        Self {
            key,
            parse_and_apply,
        }
    }

    fn apply(self, opts: &Dictionary, patch: &mut RuntimeOptionsPatch) -> Result<()> {
        (self.parse_and_apply)(opts, patch, self.key)
    }
}

fn validated_f64(key: &str, value: Object) -> Result<f64> {
    let parsed = f64_from_object(key, value)?;
    if !parsed.is_finite() {
        return Err(invalid_key(key, "finite number"));
    }
    Ok(parsed)
}

pub(super) fn validated_non_negative_f64(key: &str, value: Object) -> Result<f64> {
    let parsed = validated_f64(key, value)?;
    if parsed < 0.0 {
        return Err(invalid_key(key, "non-negative number"));
    }
    Ok(parsed)
}

fn validated_positive_f64(key: &str, value: Object) -> Result<f64> {
    let parsed = validated_f64(key, value)?;
    if parsed <= 0.0 {
        return Err(invalid_key(key, "positive number"));
    }
    Ok(parsed)
}

fn validated_cterm_color_index(key: &str, value: Object) -> Result<u16> {
    let parsed = i64_from_object(key, value)?;
    if !(0..=255).contains(&parsed) {
        return Err(invalid_key(key, "integer between 0 and 255"));
    }
    u16::try_from(parsed).map_err(|_| invalid_key(key, "integer between 0 and 255"))
}

fn parse_optional_with<T, F, V>(value: V, key: &'static str, parse: F) -> Result<Option<T>>
where
    F: Fn(&str, Object) -> Result<T>,
    V: Into<Option<Object>>,
{
    parse_optional_value_with(value.into(), key, parse)
}

pub(super) fn parse_optional_change_with<T, F, V>(
    value: V,
    key: &'static str,
    parse: F,
) -> Result<Option<OptionalChange<T>>>
where
    F: Fn(&str, Object) -> Result<T>,
    V: Into<Option<Object>>,
{
    let parsed = parse_optional_change_value_with(value.into(), key, parse)?;
    Ok(parsed.map(|value| match value {
        ParsedOptionalChange::Set(value) => OptionalChange::Set(value),
        ParsedOptionalChange::Clear => OptionalChange::Clear,
    }))
}

fn parse_optional_non_negative_i64<V>(value: V, key: &'static str) -> Result<Option<i64>>
where
    V: Into<Option<Object>>,
{
    parse_optional_with(value, key, i64_from_object)?
        .map(|parsed| {
            if parsed < 0 {
                return Err(invalid_key(key, "non-negative integer"));
            }
            Ok(parsed)
        })
        .transpose()
}

fn parse_optional_non_negative_u32<V>(value: V, key: &'static str) -> Result<Option<u32>>
where
    V: Into<Option<Object>>,
{
    parse_optional_non_negative_i64(value, key)?
        .map(|parsed| u32::try_from(parsed).map_err(|_| invalid_key(key, "non-negative integer")))
        .transpose()
}

fn parse_optional_non_negative_usize<V>(value: V, key: &'static str) -> Result<Option<usize>>
where
    V: Into<Option<Object>>,
{
    parse_optional_non_negative_i64(value, key)?
        .map(|parsed| usize::try_from(parsed).map_err(|_| invalid_key(key, "non-negative integer")))
        .transpose()
}

fn parse_optional_positive_u32<V>(value: V, key: &'static str) -> Result<Option<u32>>
where
    V: Into<Option<Object>>,
{
    parse_optional_with(value, key, i64_from_object)?
        .map(|parsed| {
            if parsed < 1 {
                return Err(invalid_key(key, "positive integer"));
            }
            u32::try_from(parsed).map_err(|_| invalid_key(key, "positive integer"))
        })
        .transpose()
}

pub(super) fn parse_optional_filetypes_disabled<V>(
    value: V,
    key: &'static str,
) -> Result<Option<Vec<String>>>
where
    V: Into<Option<Object>>,
{
    let Some(value) = value.into() else {
        return Ok(None);
    };
    if value.is_nil() {
        return Ok(Some(Vec::new()));
    }

    let values =
        parse_indexed_objects(key, value, None).map_err(|_| invalid_key(key, "array[string]"))?;
    let mut filetypes = Vec::with_capacity(values.len());
    for (index, entry) in values.into_iter().enumerate() {
        let entry_key = format!("{key}[{}]", index + 1);
        filetypes.push(string_from_object(&entry_key, entry)?);
    }
    Ok(Some(filetypes))
}

fn parse_optional_cterm_cursor_colors<V>(
    value: V,
    key: &'static str,
) -> Result<Option<OptionalChange<CtermCursorColorsPatch>>>
where
    V: Into<Option<Object>>,
{
    let Some(value) = value.into() else {
        return Ok(None);
    };
    if value.is_nil() {
        return Ok(Some(OptionalChange::Clear));
    }

    let values =
        parse_indexed_objects(key, value, None).map_err(|_| invalid_key(key, "array[integer]"))?;
    let mut colors = Vec::with_capacity(values.len());
    for (index, entry) in values.into_iter().enumerate() {
        let entry_key = format!("{key}[{}]", index + 1);
        colors.push(validated_cterm_color_index(&entry_key, entry)?);
    }
    let color_levels =
        u32::try_from(colors.len()).map_err(|_| invalid_key(key, "array length too large"))?;
    Ok(Some(OptionalChange::Set(CtermCursorColorsPatch {
        colors,
        color_levels,
    })))
}

fn raw_option(opts: &Dictionary, key: &str) -> Option<Object> {
    opts.get(&NvimString::from(key)).cloned()
}

fn parse_optional_bool(raw: Option<Object>, key: &'static str) -> Result<Option<bool>> {
    parse_optional_with(raw, key, bool_from_object)
}

fn parse_optional_non_negative_f64(raw: Option<Object>, key: &'static str) -> Result<Option<f64>> {
    parse_optional_with(raw, key, validated_non_negative_f64)
}

fn parse_optional_positive_f64(raw: Option<Object>, key: &'static str) -> Result<Option<f64>> {
    parse_optional_with(raw, key, validated_positive_f64)
}

fn parse_optional_top_k_per_cell_value(
    raw: Option<Object>,
    key: &'static str,
) -> Result<Option<u8>> {
    parse_optional_positive_u32(raw, key)?
        .map(|parsed| {
            if parsed < 2 {
                return Err(invalid_key(key, "integer greater than or equal to 2"));
            }
            u8::try_from(parsed).map_err(|_| invalid_key(key, "integer between 2 and 255"))
        })
        .transpose()
}

fn parse_optional_string(raw: Option<Object>, key: &'static str) -> Result<Option<String>> {
    parse_optional_with(raw, key, string_from_object)
}

fn parse_optional_buffer_perf_mode(
    raw: Option<Object>,
    key: &'static str,
) -> Result<Option<BufferPerfMode>> {
    parse_optional_with(raw, key, string_from_object)?
        .map(|mode| match mode.as_str() {
            "auto" => Ok(BufferPerfMode::Auto),
            "full" => Ok(BufferPerfMode::Full),
            "fast" => Ok(BufferPerfMode::Fast),
            "off" => Ok(BufferPerfMode::Off),
            _ => Err(invalid_key(key, "one of: auto, full, fast, off")),
        })
        .transpose()
}

fn parse_optional_change_string(
    raw: Option<Object>,
    key: &'static str,
) -> Result<Option<OptionalChange<String>>> {
    parse_optional_change_with(raw, key, string_from_object)
}

fn parse_optional_change_u16(
    raw: Option<Object>,
    key: &'static str,
) -> Result<Option<OptionalChange<u16>>> {
    parse_optional_change_with(raw, key, validated_cterm_color_index)
}

fn parse_optional_time_interval(raw: Option<Object>, key: &'static str) -> Result<Option<f64>> {
    Ok(parse_optional_with(raw, key, validated_f64)?.map(|parsed| parsed.max(1.0)))
}

macro_rules! define_option_spec {
    ($fn_name:ident, $const_name:ident, $variant:ident, $parser:path, $section:ident.$field:ident) => {
        fn $fn_name(
            opts: &Dictionary,
            patch: &mut RuntimeOptionsPatch,
            key: OptionKey,
        ) -> Result<()> {
            patch.$section.$field = $parser(raw_option(opts, key.as_str()), key.as_str())?;
            Ok(())
        }

        const $const_name: OptionSpec = OptionSpec::new(OptionKey::$variant, $fn_name);
    };
}

define_option_spec!(
    spec_enabled_apply,
    SPEC_ENABLED,
    Enabled,
    parse_optional_bool,
    runtime.enabled
);
define_option_spec!(
    spec_time_interval_apply,
    SPEC_TIME_INTERVAL,
    TimeInterval,
    parse_optional_time_interval,
    runtime.time_interval
);
define_option_spec!(
    spec_fps_apply,
    SPEC_FPS,
    Fps,
    parse_optional_positive_f64,
    runtime.fps
);
define_option_spec!(
    spec_simulation_hz_apply,
    SPEC_SIMULATION_HZ,
    SimulationHz,
    parse_optional_positive_f64,
    runtime.simulation_hz
);
define_option_spec!(
    spec_max_simulation_steps_per_frame_apply,
    SPEC_MAX_SIMULATION_STEPS_PER_FRAME,
    MaxSimulationStepsPerFrame,
    parse_optional_positive_u32,
    runtime.max_simulation_steps_per_frame
);
define_option_spec!(
    spec_delay_event_to_smear_apply,
    SPEC_DELAY_EVENT_TO_SMEAR,
    DelayEventToSmear,
    parse_optional_non_negative_f64,
    runtime.delay_event_to_smear
);
define_option_spec!(
    spec_smear_to_cmd_apply,
    SPEC_SMEAR_TO_CMD,
    SmearToCmd,
    parse_optional_bool,
    runtime.smear_to_cmd
);
define_option_spec!(
    spec_smear_insert_mode_apply,
    SPEC_SMEAR_INSERT_MODE,
    SmearInsertMode,
    parse_optional_bool,
    runtime.smear_insert_mode
);
define_option_spec!(
    spec_smear_replace_mode_apply,
    SPEC_SMEAR_REPLACE_MODE,
    SmearReplaceMode,
    parse_optional_bool,
    runtime.smear_replace_mode
);
define_option_spec!(
    spec_smear_terminal_mode_apply,
    SPEC_SMEAR_TERMINAL_MODE,
    SmearTerminalMode,
    parse_optional_bool,
    runtime.smear_terminal_mode
);
define_option_spec!(
    spec_animate_in_insert_mode_apply,
    SPEC_ANIMATE_IN_INSERT_MODE,
    AnimateInInsertMode,
    parse_optional_bool,
    runtime.animate_in_insert_mode
);
define_option_spec!(
    spec_animate_command_line_apply,
    SPEC_ANIMATE_COMMAND_LINE,
    AnimateCommandLine,
    parse_optional_bool,
    runtime.animate_command_line
);
define_option_spec!(
    spec_vertical_bar_cursor_apply,
    SPEC_VERTICAL_BAR_CURSOR,
    VerticalBarCursor,
    parse_optional_bool,
    runtime.vertical_bar_cursor
);
define_option_spec!(
    spec_vertical_bar_cursor_insert_mode_apply,
    SPEC_VERTICAL_BAR_CURSOR_INSERT_MODE,
    VerticalBarCursorInsertMode,
    parse_optional_bool,
    runtime.vertical_bar_cursor_insert_mode
);
define_option_spec!(
    spec_horizontal_bar_cursor_replace_mode_apply,
    SPEC_HORIZONTAL_BAR_CURSOR_REPLACE_MODE,
    HorizontalBarCursorReplaceMode,
    parse_optional_bool,
    runtime.horizontal_bar_cursor_replace_mode
);
define_option_spec!(
    spec_hide_target_hack_apply,
    SPEC_HIDE_TARGET_HACK,
    HideTargetHack,
    parse_optional_bool,
    runtime.hide_target_hack
);
define_option_spec!(
    spec_max_kept_windows_apply,
    SPEC_MAX_KEPT_WINDOWS,
    MaxKeptWindows,
    parse_optional_non_negative_usize,
    runtime.max_kept_windows
);
define_option_spec!(
    spec_windows_zindex_apply,
    SPEC_WINDOWS_ZINDEX,
    WindowsZindex,
    parse_optional_non_negative_u32,
    runtime.windows_zindex
);
define_option_spec!(
    spec_buffer_perf_mode_apply,
    SPEC_BUFFER_PERF_MODE,
    BufferPerfMode,
    parse_optional_buffer_perf_mode,
    runtime.buffer_perf_mode
);
define_option_spec!(
    spec_filetypes_disabled_apply,
    SPEC_FILETYPES_DISABLED,
    FiletypesDisabled,
    parse_optional_filetypes_disabled,
    runtime.filetypes_disabled
);
define_option_spec!(
    spec_logging_level_apply,
    SPEC_LOGGING_LEVEL,
    LoggingLevel,
    parse_optional_non_negative_i64,
    runtime.logging_level
);
define_option_spec!(
    spec_cursor_color_apply,
    SPEC_CURSOR_COLOR,
    CursorColor,
    parse_optional_change_string,
    color.cursor_color
);
define_option_spec!(
    spec_cursor_color_insert_mode_apply,
    SPEC_CURSOR_COLOR_INSERT_MODE,
    CursorColorInsertMode,
    parse_optional_change_string,
    color.cursor_color_insert_mode
);
define_option_spec!(
    spec_normal_bg_apply,
    SPEC_NORMAL_BG,
    NormalBg,
    parse_optional_change_string,
    color.normal_bg
);
define_option_spec!(
    spec_transparent_bg_fallback_color_apply,
    SPEC_TRANSPARENT_BG_FALLBACK_COLOR,
    TransparentBgFallbackColor,
    parse_optional_string,
    color.transparent_bg_fallback_color
);
define_option_spec!(
    spec_cterm_bg_apply,
    SPEC_CTERM_BG,
    CtermBg,
    parse_optional_change_u16,
    color.cterm_bg
);
define_option_spec!(
    spec_cterm_cursor_colors_apply,
    SPEC_CTERM_CURSOR_COLORS,
    CtermCursorColors,
    parse_optional_cterm_cursor_colors,
    color.cterm_cursor_colors
);
define_option_spec!(
    spec_smear_between_windows_apply,
    SPEC_SMEAR_BETWEEN_WINDOWS,
    SmearBetweenWindows,
    parse_optional_bool,
    smear.smear_between_windows
);
define_option_spec!(
    spec_smear_between_buffers_apply,
    SPEC_SMEAR_BETWEEN_BUFFERS,
    SmearBetweenBuffers,
    parse_optional_bool,
    smear.smear_between_buffers
);
define_option_spec!(
    spec_smear_between_neighbor_lines_apply,
    SPEC_SMEAR_BETWEEN_NEIGHBOR_LINES,
    SmearBetweenNeighborLines,
    parse_optional_bool,
    smear.smear_between_neighbor_lines
);
define_option_spec!(
    spec_min_horizontal_distance_smear_apply,
    SPEC_MIN_HORIZONTAL_DISTANCE_SMEAR,
    MinHorizontalDistanceSmear,
    parse_optional_non_negative_f64,
    smear.min_horizontal_distance_smear
);
define_option_spec!(
    spec_min_vertical_distance_smear_apply,
    SPEC_MIN_VERTICAL_DISTANCE_SMEAR,
    MinVerticalDistanceSmear,
    parse_optional_non_negative_f64,
    smear.min_vertical_distance_smear
);
define_option_spec!(
    spec_smear_horizontally_apply,
    SPEC_SMEAR_HORIZONTALLY,
    SmearHorizontally,
    parse_optional_bool,
    smear.smear_horizontally
);
define_option_spec!(
    spec_smear_vertically_apply,
    SPEC_SMEAR_VERTICALLY,
    SmearVertically,
    parse_optional_bool,
    smear.smear_vertically
);
define_option_spec!(
    spec_smear_diagonally_apply,
    SPEC_SMEAR_DIAGONALLY,
    SmearDiagonally,
    parse_optional_bool,
    smear.smear_diagonally
);
define_option_spec!(
    spec_scroll_buffer_space_apply,
    SPEC_SCROLL_BUFFER_SPACE,
    ScrollBufferSpace,
    parse_optional_bool,
    smear.scroll_buffer_space
);
define_option_spec!(
    spec_anticipation_apply,
    SPEC_ANTICIPATION,
    Anticipation,
    parse_optional_non_negative_f64,
    motion.anticipation
);
define_option_spec!(
    spec_head_response_ms_apply,
    SPEC_HEAD_RESPONSE_MS,
    HeadResponseMs,
    parse_optional_positive_f64,
    motion.head_response_ms
);
define_option_spec!(
    spec_damping_ratio_apply,
    SPEC_DAMPING_RATIO,
    DampingRatio,
    parse_optional_positive_f64,
    motion.damping_ratio
);
define_option_spec!(
    spec_tail_response_ms_apply,
    SPEC_TAIL_RESPONSE_MS,
    TailResponseMs,
    parse_optional_positive_f64,
    motion.tail_response_ms
);
define_option_spec!(
    spec_stop_distance_enter_apply,
    SPEC_STOP_DISTANCE_ENTER,
    StopDistanceEnter,
    parse_optional_non_negative_f64,
    motion.stop_distance_enter
);
define_option_spec!(
    spec_stop_distance_exit_apply,
    SPEC_STOP_DISTANCE_EXIT,
    StopDistanceExit,
    parse_optional_non_negative_f64,
    motion.stop_distance_exit
);
define_option_spec!(
    spec_stop_velocity_enter_apply,
    SPEC_STOP_VELOCITY_ENTER,
    StopVelocityEnter,
    parse_optional_non_negative_f64,
    motion.stop_velocity_enter
);
define_option_spec!(
    spec_stop_hold_frames_apply,
    SPEC_STOP_HOLD_FRAMES,
    StopHoldFrames,
    parse_optional_positive_u32,
    motion.stop_hold_frames
);
define_option_spec!(
    spec_max_length_apply,
    SPEC_MAX_LENGTH,
    MaxLength,
    parse_optional_non_negative_f64,
    motion.max_length
);
define_option_spec!(
    spec_max_length_insert_mode_apply,
    SPEC_MAX_LENGTH_INSERT_MODE,
    MaxLengthInsertMode,
    parse_optional_non_negative_f64,
    motion.max_length_insert_mode
);
define_option_spec!(
    spec_trail_duration_ms_apply,
    SPEC_TRAIL_DURATION_MS,
    TrailDurationMs,
    parse_optional_positive_f64,
    motion.trail_duration_ms
);
define_option_spec!(
    spec_trail_min_distance_apply,
    SPEC_TRAIL_MIN_DISTANCE,
    TrailMinDistance,
    parse_optional_non_negative_f64,
    motion.trail_min_distance
);
define_option_spec!(
    spec_trail_thickness_apply,
    SPEC_TRAIL_THICKNESS,
    TrailThickness,
    parse_optional_non_negative_f64,
    motion.trail_thickness
);
define_option_spec!(
    spec_trail_thickness_x_apply,
    SPEC_TRAIL_THICKNESS_X,
    TrailThicknessX,
    parse_optional_non_negative_f64,
    motion.trail_thickness_x
);
define_option_spec!(
    spec_particles_enabled_apply,
    SPEC_PARTICLES_ENABLED,
    ParticlesEnabled,
    parse_optional_bool,
    particles.particles_enabled
);
define_option_spec!(
    spec_particle_max_num_apply,
    SPEC_PARTICLE_MAX_NUM,
    ParticleMaxNum,
    parse_optional_non_negative_usize,
    particles.particle_max_num
);
define_option_spec!(
    spec_particle_spread_apply,
    SPEC_PARTICLE_SPREAD,
    ParticleSpread,
    parse_optional_non_negative_f64,
    particles.particle_spread
);
define_option_spec!(
    spec_particles_per_second_apply,
    SPEC_PARTICLES_PER_SECOND,
    ParticlesPerSecond,
    parse_optional_non_negative_f64,
    particles.particles_per_second
);
define_option_spec!(
    spec_particles_per_length_apply,
    SPEC_PARTICLES_PER_LENGTH,
    ParticlesPerLength,
    parse_optional_non_negative_f64,
    particles.particles_per_length
);
define_option_spec!(
    spec_particle_max_lifetime_apply,
    SPEC_PARTICLE_MAX_LIFETIME,
    ParticleMaxLifetime,
    parse_optional_non_negative_f64,
    particles.particle_max_lifetime
);
define_option_spec!(
    spec_particle_lifetime_distribution_exponent_apply,
    SPEC_PARTICLE_LIFETIME_DISTRIBUTION_EXPONENT,
    ParticleLifetimeDistributionExponent,
    parse_optional_non_negative_f64,
    particles.particle_lifetime_distribution_exponent
);
define_option_spec!(
    spec_particle_max_initial_velocity_apply,
    SPEC_PARTICLE_MAX_INITIAL_VELOCITY,
    ParticleMaxInitialVelocity,
    parse_optional_non_negative_f64,
    particles.particle_max_initial_velocity
);
define_option_spec!(
    spec_particle_velocity_from_cursor_apply,
    SPEC_PARTICLE_VELOCITY_FROM_CURSOR,
    ParticleVelocityFromCursor,
    parse_optional_non_negative_f64,
    particles.particle_velocity_from_cursor
);
define_option_spec!(
    spec_particle_random_velocity_apply,
    SPEC_PARTICLE_RANDOM_VELOCITY,
    ParticleRandomVelocity,
    parse_optional_non_negative_f64,
    particles.particle_random_velocity
);
define_option_spec!(
    spec_particle_damping_apply,
    SPEC_PARTICLE_DAMPING,
    ParticleDamping,
    parse_optional_non_negative_f64,
    particles.particle_damping
);
define_option_spec!(
    spec_particle_gravity_apply,
    SPEC_PARTICLE_GRAVITY,
    ParticleGravity,
    parse_optional_non_negative_f64,
    particles.particle_gravity
);
define_option_spec!(
    spec_min_distance_emit_particles_apply,
    SPEC_MIN_DISTANCE_EMIT_PARTICLES,
    MinDistanceEmitParticles,
    parse_optional_non_negative_f64,
    particles.min_distance_emit_particles
);
define_option_spec!(
    spec_particle_switch_octant_braille_apply,
    SPEC_PARTICLE_SWITCH_OCTANT_BRAILLE,
    ParticleSwitchOctantBraille,
    parse_optional_non_negative_f64,
    particles.particle_switch_octant_braille
);
define_option_spec!(
    spec_particles_over_text_apply,
    SPEC_PARTICLES_OVER_TEXT,
    ParticlesOverText,
    parse_optional_bool,
    particles.particles_over_text
);
define_option_spec!(
    spec_never_draw_over_target_apply,
    SPEC_NEVER_DRAW_OVER_TARGET,
    NeverDrawOverTarget,
    parse_optional_bool,
    rendering.never_draw_over_target
);
define_option_spec!(
    spec_color_levels_apply,
    SPEC_COLOR_LEVELS,
    ColorLevels,
    parse_optional_positive_u32,
    rendering.color_levels
);
define_option_spec!(
    spec_gamma_apply,
    SPEC_GAMMA,
    Gamma,
    parse_optional_positive_f64,
    rendering.gamma
);
define_option_spec!(
    spec_tail_duration_ms_apply,
    SPEC_TAIL_DURATION_MS,
    TailDurationMs,
    parse_optional_positive_f64,
    rendering.tail_duration_ms
);
define_option_spec!(
    spec_spatial_coherence_weight_apply,
    SPEC_SPATIAL_COHERENCE_WEIGHT,
    SpatialCoherenceWeight,
    parse_optional_non_negative_f64,
    rendering.spatial_coherence_weight
);
define_option_spec!(
    spec_temporal_stability_weight_apply,
    SPEC_TEMPORAL_STABILITY_WEIGHT,
    TemporalStabilityWeight,
    parse_optional_non_negative_f64,
    rendering.temporal_stability_weight
);
define_option_spec!(
    spec_top_k_per_cell_apply,
    SPEC_TOP_K_PER_CELL,
    TopKPerCell,
    parse_optional_top_k_per_cell_value,
    rendering.top_k_per_cell
);

const OPTION_SPECS: &[OptionSpec] = &[
    SPEC_ENABLED,
    SPEC_TIME_INTERVAL,
    SPEC_FPS,
    SPEC_SIMULATION_HZ,
    SPEC_MAX_SIMULATION_STEPS_PER_FRAME,
    SPEC_DELAY_EVENT_TO_SMEAR,
    SPEC_SMEAR_TO_CMD,
    SPEC_SMEAR_INSERT_MODE,
    SPEC_SMEAR_REPLACE_MODE,
    SPEC_SMEAR_TERMINAL_MODE,
    SPEC_ANIMATE_IN_INSERT_MODE,
    SPEC_ANIMATE_COMMAND_LINE,
    SPEC_VERTICAL_BAR_CURSOR,
    SPEC_VERTICAL_BAR_CURSOR_INSERT_MODE,
    SPEC_HORIZONTAL_BAR_CURSOR_REPLACE_MODE,
    SPEC_HIDE_TARGET_HACK,
    SPEC_MAX_KEPT_WINDOWS,
    SPEC_WINDOWS_ZINDEX,
    SPEC_BUFFER_PERF_MODE,
    SPEC_FILETYPES_DISABLED,
    SPEC_LOGGING_LEVEL,
    SPEC_CURSOR_COLOR,
    SPEC_CURSOR_COLOR_INSERT_MODE,
    SPEC_NORMAL_BG,
    SPEC_TRANSPARENT_BG_FALLBACK_COLOR,
    SPEC_CTERM_BG,
    SPEC_CTERM_CURSOR_COLORS,
    SPEC_SMEAR_BETWEEN_WINDOWS,
    SPEC_SMEAR_BETWEEN_BUFFERS,
    SPEC_SMEAR_BETWEEN_NEIGHBOR_LINES,
    SPEC_MIN_HORIZONTAL_DISTANCE_SMEAR,
    SPEC_MIN_VERTICAL_DISTANCE_SMEAR,
    SPEC_SMEAR_HORIZONTALLY,
    SPEC_SMEAR_VERTICALLY,
    SPEC_SMEAR_DIAGONALLY,
    SPEC_SCROLL_BUFFER_SPACE,
    SPEC_ANTICIPATION,
    SPEC_HEAD_RESPONSE_MS,
    SPEC_DAMPING_RATIO,
    SPEC_TAIL_RESPONSE_MS,
    SPEC_STOP_DISTANCE_ENTER,
    SPEC_STOP_DISTANCE_EXIT,
    SPEC_STOP_VELOCITY_ENTER,
    SPEC_STOP_HOLD_FRAMES,
    SPEC_MAX_LENGTH,
    SPEC_MAX_LENGTH_INSERT_MODE,
    SPEC_TRAIL_DURATION_MS,
    SPEC_TRAIL_MIN_DISTANCE,
    SPEC_TRAIL_THICKNESS,
    SPEC_TRAIL_THICKNESS_X,
    SPEC_PARTICLES_ENABLED,
    SPEC_PARTICLE_MAX_NUM,
    SPEC_PARTICLE_SPREAD,
    SPEC_PARTICLES_PER_SECOND,
    SPEC_PARTICLES_PER_LENGTH,
    SPEC_PARTICLE_MAX_LIFETIME,
    SPEC_PARTICLE_LIFETIME_DISTRIBUTION_EXPONENT,
    SPEC_PARTICLE_MAX_INITIAL_VELOCITY,
    SPEC_PARTICLE_VELOCITY_FROM_CURSOR,
    SPEC_PARTICLE_RANDOM_VELOCITY,
    SPEC_PARTICLE_DAMPING,
    SPEC_PARTICLE_GRAVITY,
    SPEC_MIN_DISTANCE_EMIT_PARTICLES,
    SPEC_PARTICLE_SWITCH_OCTANT_BRAILLE,
    SPEC_PARTICLES_OVER_TEXT,
    SPEC_NEVER_DRAW_OVER_TARGET,
    SPEC_COLOR_LEVELS,
    SPEC_GAMMA,
    SPEC_TAIL_DURATION_MS,
    SPEC_SPATIAL_COHERENCE_WEIGHT,
    SPEC_TEMPORAL_STABILITY_WEIGHT,
    SPEC_TOP_K_PER_CELL,
];

fn validate_known_option_keys(opts: &Dictionary) -> Result<()> {
    for key in opts.keys() {
        let key_str = key.to_string_lossy();
        let known = OPTION_SPECS
            .iter()
            .any(|spec| spec.key.as_str() == key_str.as_ref());
        if !known {
            return Err(invalid_key(
                key_str.as_ref(),
                "supported option key for nvimrs_smear_cursor",
            ));
        }
    }
    Ok(())
}

fn parse_runtime_options_patch(opts: &Dictionary) -> Result<RuntimeOptionsPatch> {
    validate_known_option_keys(opts)?;

    let mut patch = RuntimeOptionsPatch {
        runtime: RuntimeSwitchesPatch::default(),
        color: ColorOptionsPatch::default(),
        smear: SmearBehaviorPatch::default(),
        motion: MotionOptionsPatch::default(),
        particles: ParticleOptionsPatch::default(),
        rendering: RenderingOptionsPatch::default(),
    };

    for spec in OPTION_SPECS {
        spec.apply(opts, &mut patch)?;
    }

    Ok(patch)
}

impl RuntimeOptionsPatch {
    pub(super) fn parse(opts: &Dictionary) -> Result<Self> {
        parse_runtime_options_patch(opts)
    }
}

pub(super) fn apply_runtime_options(state: &mut RuntimeState, opts: &Dictionary) -> Result<()> {
    let patch = RuntimeOptionsPatch::parse(opts)?;
    patch.validate_against(&state.config)?;
    let effects = state.apply_runtime_options_patch(patch);
    if let Some(logging_level) = effects.logging_level {
        super::logging::set_log_level(logging_level);
    }
    Ok(())
}
