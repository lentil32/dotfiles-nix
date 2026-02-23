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

#[derive(Debug, Default)]
struct RawOptionObject(Option<Object>);

impl From<RawOptionObject> for Option<Object> {
    fn from(value: RawOptionObject) -> Self {
        value.0
    }
}

#[derive(Debug)]
struct RawRuntimeOptions {
    enabled: RawOptionObject,
    time_interval: RawOptionObject,
    delay_disable: RawOptionObject,
    delay_event_to_smear: RawOptionObject,
    delay_after_key: RawOptionObject,
    smear_to_cmd: RawOptionObject,
    smear_insert_mode: RawOptionObject,
    smear_replace_mode: RawOptionObject,
    smear_terminal_mode: RawOptionObject,
    vertical_bar_cursor: RawOptionObject,
    vertical_bar_cursor_insert_mode: RawOptionObject,
    horizontal_bar_cursor_replace_mode: RawOptionObject,
    hide_target_hack: RawOptionObject,
    max_kept_windows: RawOptionObject,
    windows_zindex: RawOptionObject,
    filetypes_disabled: RawOptionObject,
    logging_level: RawOptionObject,
    cursor_color: RawOptionObject,
    cursor_color_insert_mode: RawOptionObject,
    normal_bg: RawOptionObject,
    transparent_bg_fallback_color: RawOptionObject,
    cterm_bg: RawOptionObject,
    cterm_cursor_colors: RawOptionObject,
    smear_between_buffers: RawOptionObject,
    smear_between_neighbor_lines: RawOptionObject,
    min_horizontal_distance_smear: RawOptionObject,
    min_vertical_distance_smear: RawOptionObject,
    smear_horizontally: RawOptionObject,
    smear_vertically: RawOptionObject,
    smear_diagonally: RawOptionObject,
    scroll_buffer_space: RawOptionObject,
    stiffness: RawOptionObject,
    trailing_stiffness: RawOptionObject,
    trailing_exponent: RawOptionObject,
    stiffness_insert_mode: RawOptionObject,
    trailing_stiffness_insert_mode: RawOptionObject,
    trailing_exponent_insert_mode: RawOptionObject,
    anticipation: RawOptionObject,
    damping: RawOptionObject,
    damping_insert_mode: RawOptionObject,
    distance_stop_animating: RawOptionObject,
    distance_stop_animating_vertical_bar: RawOptionObject,
    max_length: RawOptionObject,
    max_length_insert_mode: RawOptionObject,
    particles_enabled: RawOptionObject,
    particle_max_num: RawOptionObject,
    particle_spread: RawOptionObject,
    particles_per_second: RawOptionObject,
    particles_per_length: RawOptionObject,
    particle_max_lifetime: RawOptionObject,
    particle_lifetime_distribution_exponent: RawOptionObject,
    particle_max_initial_velocity: RawOptionObject,
    particle_velocity_from_cursor: RawOptionObject,
    particle_random_velocity: RawOptionObject,
    particle_damping: RawOptionObject,
    particle_gravity: RawOptionObject,
    min_distance_emit_particles: RawOptionObject,
    particle_switch_octant_braille: RawOptionObject,
    particles_over_text: RawOptionObject,
    volume_reduction_exponent: RawOptionObject,
    minimum_volume_factor: RawOptionObject,
    never_draw_over_target: RawOptionObject,
    use_diagonal_blocks: RawOptionObject,
    max_slope_horizontal: RawOptionObject,
    min_slope_vertical: RawOptionObject,
    max_angle_difference_diagonal: RawOptionObject,
    max_offset_diagonal: RawOptionObject,
    min_shade_no_diagonal: RawOptionObject,
    min_shade_no_diagonal_vertical_bar: RawOptionObject,
    max_shade_no_matrix: RawOptionObject,
    color_levels: RawOptionObject,
    gamma: RawOptionObject,
    gradient_exponent: RawOptionObject,
    matrix_pixel_threshold: RawOptionObject,
    matrix_pixel_threshold_vertical_bar: RawOptionObject,
    matrix_pixel_min_factor: RawOptionObject,
}

fn raw_option(opts: &Dictionary, key: &str) -> RawOptionObject {
    RawOptionObject(opts.get(&NvimString::from(key)).cloned())
}

impl RawRuntimeOptions {
    fn parse(opts: &Dictionary) -> Result<Self> {
        Ok(Self {
            enabled: raw_option(opts, "enabled"),
            time_interval: raw_option(opts, "time_interval"),
            delay_disable: raw_option(opts, "delay_disable"),
            delay_event_to_smear: raw_option(opts, "delay_event_to_smear"),
            delay_after_key: raw_option(opts, "delay_after_key"),
            smear_to_cmd: raw_option(opts, "smear_to_cmd"),
            smear_insert_mode: raw_option(opts, "smear_insert_mode"),
            smear_replace_mode: raw_option(opts, "smear_replace_mode"),
            smear_terminal_mode: raw_option(opts, "smear_terminal_mode"),
            vertical_bar_cursor: raw_option(opts, "vertical_bar_cursor"),
            vertical_bar_cursor_insert_mode: raw_option(opts, "vertical_bar_cursor_insert_mode"),
            horizontal_bar_cursor_replace_mode: raw_option(
                opts,
                "horizontal_bar_cursor_replace_mode",
            ),
            hide_target_hack: raw_option(opts, "hide_target_hack"),
            max_kept_windows: raw_option(opts, "max_kept_windows"),
            windows_zindex: raw_option(opts, "windows_zindex"),
            filetypes_disabled: raw_option(opts, "filetypes_disabled"),
            logging_level: raw_option(opts, "logging_level"),
            cursor_color: raw_option(opts, "cursor_color"),
            cursor_color_insert_mode: raw_option(opts, "cursor_color_insert_mode"),
            normal_bg: raw_option(opts, "normal_bg"),
            transparent_bg_fallback_color: raw_option(opts, "transparent_bg_fallback_color"),
            cterm_bg: raw_option(opts, "cterm_bg"),
            cterm_cursor_colors: raw_option(opts, "cterm_cursor_colors"),
            smear_between_buffers: raw_option(opts, "smear_between_buffers"),
            smear_between_neighbor_lines: raw_option(opts, "smear_between_neighbor_lines"),
            min_horizontal_distance_smear: raw_option(opts, "min_horizontal_distance_smear"),
            min_vertical_distance_smear: raw_option(opts, "min_vertical_distance_smear"),
            smear_horizontally: raw_option(opts, "smear_horizontally"),
            smear_vertically: raw_option(opts, "smear_vertically"),
            smear_diagonally: raw_option(opts, "smear_diagonally"),
            scroll_buffer_space: raw_option(opts, "scroll_buffer_space"),
            stiffness: raw_option(opts, "stiffness"),
            trailing_stiffness: raw_option(opts, "trailing_stiffness"),
            trailing_exponent: raw_option(opts, "trailing_exponent"),
            stiffness_insert_mode: raw_option(opts, "stiffness_insert_mode"),
            trailing_stiffness_insert_mode: raw_option(opts, "trailing_stiffness_insert_mode"),
            trailing_exponent_insert_mode: raw_option(opts, "trailing_exponent_insert_mode"),
            anticipation: raw_option(opts, "anticipation"),
            damping: raw_option(opts, "damping"),
            damping_insert_mode: raw_option(opts, "damping_insert_mode"),
            distance_stop_animating: raw_option(opts, "distance_stop_animating"),
            distance_stop_animating_vertical_bar: raw_option(
                opts,
                "distance_stop_animating_vertical_bar",
            ),
            max_length: raw_option(opts, "max_length"),
            max_length_insert_mode: raw_option(opts, "max_length_insert_mode"),
            particles_enabled: raw_option(opts, "particles_enabled"),
            particle_max_num: raw_option(opts, "particle_max_num"),
            particle_spread: raw_option(opts, "particle_spread"),
            particles_per_second: raw_option(opts, "particles_per_second"),
            particles_per_length: raw_option(opts, "particles_per_length"),
            particle_max_lifetime: raw_option(opts, "particle_max_lifetime"),
            particle_lifetime_distribution_exponent: raw_option(
                opts,
                "particle_lifetime_distribution_exponent",
            ),
            particle_max_initial_velocity: raw_option(opts, "particle_max_initial_velocity"),
            particle_velocity_from_cursor: raw_option(opts, "particle_velocity_from_cursor"),
            particle_random_velocity: raw_option(opts, "particle_random_velocity"),
            particle_damping: raw_option(opts, "particle_damping"),
            particle_gravity: raw_option(opts, "particle_gravity"),
            min_distance_emit_particles: raw_option(opts, "min_distance_emit_particles"),
            particle_switch_octant_braille: raw_option(opts, "particle_switch_octant_braille"),
            particles_over_text: raw_option(opts, "particles_over_text"),
            volume_reduction_exponent: raw_option(opts, "volume_reduction_exponent"),
            minimum_volume_factor: raw_option(opts, "minimum_volume_factor"),
            never_draw_over_target: raw_option(opts, "never_draw_over_target"),
            use_diagonal_blocks: raw_option(opts, "use_diagonal_blocks"),
            max_slope_horizontal: raw_option(opts, "max_slope_horizontal"),
            min_slope_vertical: raw_option(opts, "min_slope_vertical"),
            max_angle_difference_diagonal: raw_option(opts, "max_angle_difference_diagonal"),
            max_offset_diagonal: raw_option(opts, "max_offset_diagonal"),
            min_shade_no_diagonal: raw_option(opts, "min_shade_no_diagonal"),
            min_shade_no_diagonal_vertical_bar: raw_option(
                opts,
                "min_shade_no_diagonal_vertical_bar",
            ),
            max_shade_no_matrix: raw_option(opts, "max_shade_no_matrix"),
            color_levels: raw_option(opts, "color_levels"),
            gamma: raw_option(opts, "gamma"),
            gradient_exponent: raw_option(opts, "gradient_exponent"),
            matrix_pixel_threshold: raw_option(opts, "matrix_pixel_threshold"),
            matrix_pixel_threshold_vertical_bar: raw_option(
                opts,
                "matrix_pixel_threshold_vertical_bar",
            ),
            matrix_pixel_min_factor: raw_option(opts, "matrix_pixel_min_factor"),
        })
    }

    fn into_patch(self) -> Result<RuntimeOptionsPatch> {
        Ok(RuntimeOptionsPatch {
            runtime: RuntimeSwitchesPatch {
                enabled: parse_optional_with(self.enabled, "enabled", bool_from_object)?,
                time_interval: parse_optional_with(
                    self.time_interval,
                    "time_interval",
                    validated_f64,
                )?
                .map(|parsed| parsed.max(1.0)),
                delay_disable: parse_optional_change_with(
                    self.delay_disable,
                    "delay_disable",
                    validated_non_negative_f64,
                )?,
                delay_event_to_smear: parse_optional_with(
                    self.delay_event_to_smear,
                    "delay_event_to_smear",
                    validated_non_negative_f64,
                )?,
                delay_after_key: parse_optional_with(
                    self.delay_after_key,
                    "delay_after_key",
                    validated_non_negative_f64,
                )?,
                smear_to_cmd: parse_optional_with(
                    self.smear_to_cmd,
                    "smear_to_cmd",
                    bool_from_object,
                )?,
                smear_insert_mode: parse_optional_with(
                    self.smear_insert_mode,
                    "smear_insert_mode",
                    bool_from_object,
                )?,
                smear_replace_mode: parse_optional_with(
                    self.smear_replace_mode,
                    "smear_replace_mode",
                    bool_from_object,
                )?,
                smear_terminal_mode: parse_optional_with(
                    self.smear_terminal_mode,
                    "smear_terminal_mode",
                    bool_from_object,
                )?,
                vertical_bar_cursor: parse_optional_with(
                    self.vertical_bar_cursor,
                    "vertical_bar_cursor",
                    bool_from_object,
                )?,
                vertical_bar_cursor_insert_mode: parse_optional_with(
                    self.vertical_bar_cursor_insert_mode,
                    "vertical_bar_cursor_insert_mode",
                    bool_from_object,
                )?,
                horizontal_bar_cursor_replace_mode: parse_optional_with(
                    self.horizontal_bar_cursor_replace_mode,
                    "horizontal_bar_cursor_replace_mode",
                    bool_from_object,
                )?,
                hide_target_hack: parse_optional_with(
                    self.hide_target_hack,
                    "hide_target_hack",
                    bool_from_object,
                )?,
                max_kept_windows: parse_optional_non_negative_usize(
                    self.max_kept_windows,
                    "max_kept_windows",
                )?,
                windows_zindex: parse_optional_non_negative_u32(
                    self.windows_zindex,
                    "windows_zindex",
                )?,
                filetypes_disabled: parse_optional_filetypes_disabled(
                    self.filetypes_disabled,
                    "filetypes_disabled",
                )?,
                logging_level: parse_optional_non_negative_i64(
                    self.logging_level,
                    "logging_level",
                )?,
            },
            color: ColorOptionsPatch {
                cursor_color: parse_optional_change_with(
                    self.cursor_color,
                    "cursor_color",
                    string_from_object,
                )?,
                cursor_color_insert_mode: parse_optional_change_with(
                    self.cursor_color_insert_mode,
                    "cursor_color_insert_mode",
                    string_from_object,
                )?,
                normal_bg: parse_optional_change_with(
                    self.normal_bg,
                    "normal_bg",
                    string_from_object,
                )?,
                transparent_bg_fallback_color: parse_optional_with(
                    self.transparent_bg_fallback_color,
                    "transparent_bg_fallback_color",
                    string_from_object,
                )?,
                cterm_bg: parse_optional_change_with(
                    self.cterm_bg,
                    "cterm_bg",
                    validated_cterm_color_index,
                )?,
                cterm_cursor_colors: parse_optional_cterm_cursor_colors(
                    self.cterm_cursor_colors,
                    "cterm_cursor_colors",
                )?,
            },
            smear: SmearBehaviorPatch {
                smear_between_buffers: parse_optional_with(
                    self.smear_between_buffers,
                    "smear_between_buffers",
                    bool_from_object,
                )?,
                smear_between_neighbor_lines: parse_optional_with(
                    self.smear_between_neighbor_lines,
                    "smear_between_neighbor_lines",
                    bool_from_object,
                )?,
                min_horizontal_distance_smear: parse_optional_with(
                    self.min_horizontal_distance_smear,
                    "min_horizontal_distance_smear",
                    validated_non_negative_f64,
                )?,
                min_vertical_distance_smear: parse_optional_with(
                    self.min_vertical_distance_smear,
                    "min_vertical_distance_smear",
                    validated_non_negative_f64,
                )?,
                smear_horizontally: parse_optional_with(
                    self.smear_horizontally,
                    "smear_horizontally",
                    bool_from_object,
                )?,
                smear_vertically: parse_optional_with(
                    self.smear_vertically,
                    "smear_vertically",
                    bool_from_object,
                )?,
                smear_diagonally: parse_optional_with(
                    self.smear_diagonally,
                    "smear_diagonally",
                    bool_from_object,
                )?,
                scroll_buffer_space: parse_optional_with(
                    self.scroll_buffer_space,
                    "scroll_buffer_space",
                    bool_from_object,
                )?,
            },
            motion: MotionOptionsPatch {
                stiffness: parse_optional_with(
                    self.stiffness,
                    "stiffness",
                    validated_non_negative_f64,
                )?,
                trailing_stiffness: parse_optional_with(
                    self.trailing_stiffness,
                    "trailing_stiffness",
                    validated_non_negative_f64,
                )?,
                trailing_exponent: parse_optional_with(
                    self.trailing_exponent,
                    "trailing_exponent",
                    validated_non_negative_f64,
                )?,
                stiffness_insert_mode: parse_optional_with(
                    self.stiffness_insert_mode,
                    "stiffness_insert_mode",
                    validated_non_negative_f64,
                )?,
                trailing_stiffness_insert_mode: parse_optional_with(
                    self.trailing_stiffness_insert_mode,
                    "trailing_stiffness_insert_mode",
                    validated_non_negative_f64,
                )?,
                trailing_exponent_insert_mode: parse_optional_with(
                    self.trailing_exponent_insert_mode,
                    "trailing_exponent_insert_mode",
                    validated_non_negative_f64,
                )?,
                anticipation: parse_optional_with(
                    self.anticipation,
                    "anticipation",
                    validated_non_negative_f64,
                )?,
                damping: parse_optional_with(self.damping, "damping", validated_non_negative_f64)?,
                damping_insert_mode: parse_optional_with(
                    self.damping_insert_mode,
                    "damping_insert_mode",
                    validated_non_negative_f64,
                )?,
                distance_stop_animating: parse_optional_with(
                    self.distance_stop_animating,
                    "distance_stop_animating",
                    validated_non_negative_f64,
                )?,
                distance_stop_animating_vertical_bar: parse_optional_with(
                    self.distance_stop_animating_vertical_bar,
                    "distance_stop_animating_vertical_bar",
                    validated_non_negative_f64,
                )?,
                max_length: parse_optional_with(
                    self.max_length,
                    "max_length",
                    validated_non_negative_f64,
                )?,
                max_length_insert_mode: parse_optional_with(
                    self.max_length_insert_mode,
                    "max_length_insert_mode",
                    validated_non_negative_f64,
                )?,
            },
            particles: ParticleOptionsPatch {
                particles_enabled: parse_optional_with(
                    self.particles_enabled,
                    "particles_enabled",
                    bool_from_object,
                )?,
                particle_max_num: parse_optional_non_negative_usize(
                    self.particle_max_num,
                    "particle_max_num",
                )?,
                particle_spread: parse_optional_with(
                    self.particle_spread,
                    "particle_spread",
                    validated_non_negative_f64,
                )?,
                particles_per_second: parse_optional_with(
                    self.particles_per_second,
                    "particles_per_second",
                    validated_non_negative_f64,
                )?,
                particles_per_length: parse_optional_with(
                    self.particles_per_length,
                    "particles_per_length",
                    validated_non_negative_f64,
                )?,
                particle_max_lifetime: parse_optional_with(
                    self.particle_max_lifetime,
                    "particle_max_lifetime",
                    validated_non_negative_f64,
                )?,
                particle_lifetime_distribution_exponent: parse_optional_with(
                    self.particle_lifetime_distribution_exponent,
                    "particle_lifetime_distribution_exponent",
                    validated_non_negative_f64,
                )?,
                particle_max_initial_velocity: parse_optional_with(
                    self.particle_max_initial_velocity,
                    "particle_max_initial_velocity",
                    validated_non_negative_f64,
                )?,
                particle_velocity_from_cursor: parse_optional_with(
                    self.particle_velocity_from_cursor,
                    "particle_velocity_from_cursor",
                    validated_non_negative_f64,
                )?,
                particle_random_velocity: parse_optional_with(
                    self.particle_random_velocity,
                    "particle_random_velocity",
                    validated_non_negative_f64,
                )?,
                particle_damping: parse_optional_with(
                    self.particle_damping,
                    "particle_damping",
                    validated_non_negative_f64,
                )?,
                particle_gravity: parse_optional_with(
                    self.particle_gravity,
                    "particle_gravity",
                    validated_non_negative_f64,
                )?,
                min_distance_emit_particles: parse_optional_with(
                    self.min_distance_emit_particles,
                    "min_distance_emit_particles",
                    validated_non_negative_f64,
                )?,
                particle_switch_octant_braille: parse_optional_with(
                    self.particle_switch_octant_braille,
                    "particle_switch_octant_braille",
                    validated_non_negative_f64,
                )?,
                particles_over_text: parse_optional_with(
                    self.particles_over_text,
                    "particles_over_text",
                    bool_from_object,
                )?,
                volume_reduction_exponent: parse_optional_with(
                    self.volume_reduction_exponent,
                    "volume_reduction_exponent",
                    validated_non_negative_f64,
                )?,
                minimum_volume_factor: parse_optional_with(
                    self.minimum_volume_factor,
                    "minimum_volume_factor",
                    validated_non_negative_f64,
                )?,
            },
            rendering: RenderingOptionsPatch {
                never_draw_over_target: parse_optional_with(
                    self.never_draw_over_target,
                    "never_draw_over_target",
                    bool_from_object,
                )?,
                use_diagonal_blocks: parse_optional_with(
                    self.use_diagonal_blocks,
                    "use_diagonal_blocks",
                    bool_from_object,
                )?,
                max_slope_horizontal: parse_optional_with(
                    self.max_slope_horizontal,
                    "max_slope_horizontal",
                    validated_non_negative_f64,
                )?,
                min_slope_vertical: parse_optional_with(
                    self.min_slope_vertical,
                    "min_slope_vertical",
                    validated_non_negative_f64,
                )?,
                max_angle_difference_diagonal: parse_optional_with(
                    self.max_angle_difference_diagonal,
                    "max_angle_difference_diagonal",
                    validated_non_negative_f64,
                )?,
                max_offset_diagonal: parse_optional_with(
                    self.max_offset_diagonal,
                    "max_offset_diagonal",
                    validated_non_negative_f64,
                )?,
                min_shade_no_diagonal: parse_optional_with(
                    self.min_shade_no_diagonal,
                    "min_shade_no_diagonal",
                    validated_non_negative_f64,
                )?,
                min_shade_no_diagonal_vertical_bar: parse_optional_with(
                    self.min_shade_no_diagonal_vertical_bar,
                    "min_shade_no_diagonal_vertical_bar",
                    validated_non_negative_f64,
                )?,
                max_shade_no_matrix: parse_optional_with(
                    self.max_shade_no_matrix,
                    "max_shade_no_matrix",
                    validated_non_negative_f64,
                )?,
                color_levels: parse_optional_positive_u32(self.color_levels, "color_levels")?,
                gamma: parse_optional_with(self.gamma, "gamma", validated_positive_f64)?,
                gradient_exponent: parse_optional_with(
                    self.gradient_exponent,
                    "gradient_exponent",
                    validated_non_negative_f64,
                )?,
                matrix_pixel_threshold: parse_optional_with(
                    self.matrix_pixel_threshold,
                    "matrix_pixel_threshold",
                    validated_non_negative_f64,
                )?,
                matrix_pixel_threshold_vertical_bar: parse_optional_with(
                    self.matrix_pixel_threshold_vertical_bar,
                    "matrix_pixel_threshold_vertical_bar",
                    validated_non_negative_f64,
                )?,
                matrix_pixel_min_factor: parse_optional_with(
                    self.matrix_pixel_min_factor,
                    "matrix_pixel_min_factor",
                    validated_non_negative_f64,
                )?,
            },
        })
    }
}

impl RuntimeOptionsPatch {
    pub(super) fn parse(opts: &Dictionary) -> Result<Self> {
        RawRuntimeOptions::parse(opts)?.into_patch()
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
