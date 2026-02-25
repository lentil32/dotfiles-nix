use crate::lua::{
    bool_from_object, f64_from_object, i64_from_object, invalid_key, parse_indexed_objects,
    string_from_object,
};
use crate::state::{
    ColorOptionsPatch, CtermCursorColorsPatch, MotionOptionsPatch, OptionalChange,
    ParticleOptionsPatch, RenderingOptionsPatch, RuntimeOptionsPatch, RuntimeState,
    RuntimeSwitchesPatch, SmearBehaviorPatch,
};
use nvim_oxi::{Dictionary, Object, Result, String as NvimString};

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
    Ok(parsed as u16)
}

fn parse_optional_with<T, F, V>(value: V, key: &'static str, parse: F) -> Result<Option<T>>
where
    F: Fn(&str, Object) -> Result<T>,
    V: Into<Option<Object>>,
{
    value.into().map(|value| parse(key, value)).transpose()
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
    let Some(value) = value.into() else {
        return Ok(None);
    };
    if value.is_nil() {
        return Ok(Some(OptionalChange::Clear));
    }
    parse(key, value).map(|parsed| Some(OptionalChange::Set(parsed)))
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

fn parse_runtime_options_patch(opts: &Dictionary) -> Result<RuntimeOptionsPatch> {
    macro_rules! raw {
        ($field:ident) => {
            raw_option(opts, stringify!($field))
        };
    }
    macro_rules! optional_with {
        ($field:ident, $parser:expr) => {
            parse_optional_with(raw!($field), stringify!($field), $parser)?
        };
    }
    macro_rules! optional_change {
        ($field:ident, $parser:expr) => {
            parse_optional_change_with(raw!($field), stringify!($field), $parser)?
        };
    }
    macro_rules! optional_non_negative_i64 {
        ($field:ident) => {
            parse_optional_non_negative_i64(raw!($field), stringify!($field))?
        };
    }
    macro_rules! optional_non_negative_u32 {
        ($field:ident) => {
            parse_optional_non_negative_u32(raw!($field), stringify!($field))?
        };
    }
    macro_rules! optional_non_negative_usize {
        ($field:ident) => {
            parse_optional_non_negative_usize(raw!($field), stringify!($field))?
        };
    }
    macro_rules! optional_positive_u32 {
        ($field:ident) => {
            parse_optional_positive_u32(raw!($field), stringify!($field))?
        };
    }
    macro_rules! optional_filetypes_disabled {
        ($field:ident) => {
            parse_optional_filetypes_disabled(raw!($field), stringify!($field))?
        };
    }
    macro_rules! optional_cterm_cursor_colors {
        ($field:ident) => {
            parse_optional_cterm_cursor_colors(raw!($field), stringify!($field))?
        };
    }

    Ok(RuntimeOptionsPatch {
        runtime: RuntimeSwitchesPatch {
            enabled: optional_with!(enabled, bool_from_object),
            time_interval: optional_with!(time_interval, validated_f64)
                .map(|parsed| parsed.max(1.0)),
            delay_disable: optional_change!(delay_disable, validated_non_negative_f64),
            delay_event_to_smear: optional_with!(delay_event_to_smear, validated_non_negative_f64),
            delay_after_key: optional_with!(delay_after_key, validated_non_negative_f64),
            smear_to_cmd: optional_with!(smear_to_cmd, bool_from_object),
            smear_insert_mode: optional_with!(smear_insert_mode, bool_from_object),
            smear_replace_mode: optional_with!(smear_replace_mode, bool_from_object),
            smear_terminal_mode: optional_with!(smear_terminal_mode, bool_from_object),
            vertical_bar_cursor: optional_with!(vertical_bar_cursor, bool_from_object),
            vertical_bar_cursor_insert_mode: optional_with!(
                vertical_bar_cursor_insert_mode,
                bool_from_object
            ),
            horizontal_bar_cursor_replace_mode: optional_with!(
                horizontal_bar_cursor_replace_mode,
                bool_from_object
            ),
            hide_target_hack: optional_with!(hide_target_hack, bool_from_object),
            max_kept_windows: optional_non_negative_usize!(max_kept_windows),
            windows_zindex: optional_non_negative_u32!(windows_zindex),
            filetypes_disabled: optional_filetypes_disabled!(filetypes_disabled),
            logging_level: optional_non_negative_i64!(logging_level),
        },
        color: ColorOptionsPatch {
            cursor_color: optional_change!(cursor_color, string_from_object),
            cursor_color_insert_mode: optional_change!(
                cursor_color_insert_mode,
                string_from_object
            ),
            normal_bg: optional_change!(normal_bg, string_from_object),
            transparent_bg_fallback_color: optional_with!(
                transparent_bg_fallback_color,
                string_from_object
            ),
            cterm_bg: optional_change!(cterm_bg, validated_cterm_color_index),
            cterm_cursor_colors: optional_cterm_cursor_colors!(cterm_cursor_colors),
        },
        smear: SmearBehaviorPatch {
            smear_between_buffers: optional_with!(smear_between_buffers, bool_from_object),
            smear_between_neighbor_lines: optional_with!(
                smear_between_neighbor_lines,
                bool_from_object
            ),
            min_horizontal_distance_smear: optional_with!(
                min_horizontal_distance_smear,
                validated_non_negative_f64
            ),
            min_vertical_distance_smear: optional_with!(
                min_vertical_distance_smear,
                validated_non_negative_f64
            ),
            smear_horizontally: optional_with!(smear_horizontally, bool_from_object),
            smear_vertically: optional_with!(smear_vertically, bool_from_object),
            smear_diagonally: optional_with!(smear_diagonally, bool_from_object),
            scroll_buffer_space: optional_with!(scroll_buffer_space, bool_from_object),
        },
        motion: MotionOptionsPatch {
            stiffness: optional_with!(stiffness, validated_non_negative_f64),
            trailing_stiffness: optional_with!(trailing_stiffness, validated_non_negative_f64),
            trailing_exponent: optional_with!(trailing_exponent, validated_non_negative_f64),
            stiffness_insert_mode: optional_with!(
                stiffness_insert_mode,
                validated_non_negative_f64
            ),
            trailing_stiffness_insert_mode: optional_with!(
                trailing_stiffness_insert_mode,
                validated_non_negative_f64
            ),
            trailing_exponent_insert_mode: optional_with!(
                trailing_exponent_insert_mode,
                validated_non_negative_f64
            ),
            anticipation: optional_with!(anticipation, validated_non_negative_f64),
            damping: optional_with!(damping, validated_non_negative_f64),
            damping_insert_mode: optional_with!(damping_insert_mode, validated_non_negative_f64),
            distance_stop_animating: optional_with!(
                distance_stop_animating,
                validated_non_negative_f64
            ),
            distance_stop_animating_vertical_bar: optional_with!(
                distance_stop_animating_vertical_bar,
                validated_non_negative_f64
            ),
            max_length: optional_with!(max_length, validated_non_negative_f64),
            max_length_insert_mode: optional_with!(
                max_length_insert_mode,
                validated_non_negative_f64
            ),
        },
        particles: ParticleOptionsPatch {
            particles_enabled: optional_with!(particles_enabled, bool_from_object),
            particle_max_num: optional_non_negative_usize!(particle_max_num),
            particle_spread: optional_with!(particle_spread, validated_non_negative_f64),
            particles_per_second: optional_with!(particles_per_second, validated_non_negative_f64),
            particles_per_length: optional_with!(particles_per_length, validated_non_negative_f64),
            particle_max_lifetime: optional_with!(
                particle_max_lifetime,
                validated_non_negative_f64
            ),
            particle_lifetime_distribution_exponent: optional_with!(
                particle_lifetime_distribution_exponent,
                validated_non_negative_f64
            ),
            particle_max_initial_velocity: optional_with!(
                particle_max_initial_velocity,
                validated_non_negative_f64
            ),
            particle_velocity_from_cursor: optional_with!(
                particle_velocity_from_cursor,
                validated_non_negative_f64
            ),
            particle_random_velocity: optional_with!(
                particle_random_velocity,
                validated_non_negative_f64
            ),
            particle_damping: optional_with!(particle_damping, validated_non_negative_f64),
            particle_gravity: optional_with!(particle_gravity, validated_non_negative_f64),
            min_distance_emit_particles: optional_with!(
                min_distance_emit_particles,
                validated_non_negative_f64
            ),
            particle_switch_octant_braille: optional_with!(
                particle_switch_octant_braille,
                validated_non_negative_f64
            ),
            particles_over_text: optional_with!(particles_over_text, bool_from_object),
            volume_reduction_exponent: optional_with!(
                volume_reduction_exponent,
                validated_non_negative_f64
            ),
            minimum_volume_factor: optional_with!(
                minimum_volume_factor,
                validated_non_negative_f64
            ),
        },
        rendering: RenderingOptionsPatch {
            never_draw_over_target: optional_with!(never_draw_over_target, bool_from_object),
            use_diagonal_blocks: optional_with!(use_diagonal_blocks, bool_from_object),
            max_slope_horizontal: optional_with!(max_slope_horizontal, validated_non_negative_f64),
            min_slope_vertical: optional_with!(min_slope_vertical, validated_non_negative_f64),
            max_angle_difference_diagonal: optional_with!(
                max_angle_difference_diagonal,
                validated_non_negative_f64
            ),
            max_offset_diagonal: optional_with!(max_offset_diagonal, validated_non_negative_f64),
            min_shade_no_diagonal: optional_with!(
                min_shade_no_diagonal,
                validated_non_negative_f64
            ),
            min_shade_no_diagonal_vertical_bar: optional_with!(
                min_shade_no_diagonal_vertical_bar,
                validated_non_negative_f64
            ),
            max_shade_no_matrix: optional_with!(max_shade_no_matrix, validated_non_negative_f64),
            color_levels: optional_positive_u32!(color_levels),
            gamma: optional_with!(gamma, validated_positive_f64),
            gradient_exponent: optional_with!(gradient_exponent, validated_non_negative_f64),
            matrix_pixel_threshold: optional_with!(
                matrix_pixel_threshold,
                validated_non_negative_f64
            ),
            matrix_pixel_threshold_vertical_bar: optional_with!(
                matrix_pixel_threshold_vertical_bar,
                validated_non_negative_f64
            ),
            matrix_pixel_min_factor: optional_with!(
                matrix_pixel_min_factor,
                validated_non_negative_f64
            ),
        },
    })
}

impl RuntimeOptionsPatch {
    pub(super) fn parse(opts: &Dictionary) -> Result<Self> {
        parse_runtime_options_patch(opts)
    }
}

pub(super) fn apply_runtime_options(state: &mut RuntimeState, opts: &Dictionary) -> Result<()> {
    let patch = RuntimeOptionsPatch::parse(opts)?;
    let effects = state.apply_runtime_options_patch(patch);
    if let Some(logging_level) = effects.logging_level {
        super::logging::set_log_level(logging_level);
    }
    Ok(())
}
