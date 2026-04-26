use super::super::options::parse_optional_change_with;
use super::super::options::parse_optional_filetypes_disabled;
use super::super::options::validated_non_negative_f64;
use super::cterm_colors_object;
use super::options_dict;
use crate::config::BufferPerfMode;
use crate::config::LogLevel;
use crate::config::MAX_COLOR_LEVELS;
use crate::state::OptionalChange;
use crate::state::RuntimeOptionsPatch;
use crate::test_support::proptest::pure_config;
use nvim_oxi::Array;
use nvim_oxi::Object;
use proptest::collection::vec;
use proptest::prelude::*;
use proptest::string::string_regex;

fn filetype_name_strategy() -> BoxedStrategy<String> {
    string_regex("[a-z]{1,8}")
        .expect("valid filetype regex")
        .boxed()
}

proptest! {
    #![proptest_config(pure_config())]

    #[test]
    fn prop_parse_optional_change_with_distinguishes_clear_from_set(
        clear in any::<bool>(),
        value in 0.0_f64..256.0_f64,
    ) {
        let parsed = parse_optional_change_with(
            Some(if clear { Object::nil() } else { Object::from(value) }),
            "cursor_color",
            validated_non_negative_f64,
        )
        .expect("expected parse success");

        prop_assert_eq!(
            parsed,
            Some(if clear {
                OptionalChange::Clear
            } else {
                OptionalChange::Set(value)
            })
        );
    }

    #[test]
    fn prop_parse_optional_filetypes_disabled_maps_nil_and_string_arrays(
        clear in any::<bool>(),
        filetypes in vec(filetype_name_strategy(), 0..6),
    ) {
        let parsed = parse_optional_filetypes_disabled(
            Some(if clear {
                Object::nil()
            } else {
                Object::from(Array::from_iter(filetypes.iter().cloned().map(Object::from)))
            }),
            "filetypes_disabled",
        )
        .expect("expected parse success");

        prop_assert_eq!(parsed, Some(if clear { Vec::new() } else { filetypes }));
    }

    #[test]
    fn prop_runtime_options_patch_parse_particle_max_num_accepts_integral_floats_only(
        particle_max_num in 0_usize..512_usize,
        fractional_part in 0.1_f64..0.9_f64,
    ) {
        let integral =
            options_dict([("particle_max_num", Object::from(particle_max_num as f64))]);
        let patch = RuntimeOptionsPatch::parse(&integral).expect("expected parse success");
        prop_assert_eq!(patch.particles.particle_max_num, Some(particle_max_num));

        let fractional = options_dict([(
            "particle_max_num",
            Object::from(particle_max_num as f64 + fractional_part),
        )]);
        prop_assert!(RuntimeOptionsPatch::parse(&fractional).is_err());
    }
}

#[test]
fn runtime_options_patch_parse_rejects_negative_windows_zindex() {
    let opts = options_dict([("windows_zindex", Object::from(-1_i64))]);

    let err = RuntimeOptionsPatch::parse(&opts).expect_err("expected parse failure");
    assert!(
        err.to_string().contains("windows_zindex"),
        "unexpected error: {err}"
    );
    assert!(
        err.to_string().contains("non-negative integer"),
        "unexpected error: {err}"
    );
}

#[test]
fn runtime_options_patch_parse_cterm_cursor_colors_sets_color_levels() {
    let opts = options_dict([(
        "cterm_cursor_colors",
        cterm_colors_object(&[17_i64, 42_i64]),
    )]);

    let patch = RuntimeOptionsPatch::parse(&opts).expect("expected parse success");
    let Some(OptionalChange::Set(colors)) = patch.color.cterm_cursor_colors else {
        panic!("expected cterm cursor color patch to be set");
    };
    assert_eq!(colors.colors, vec![17_u16, 42_u16]);
    assert_eq!(colors.color_levels, 2_u32);
}

#[test]
fn runtime_options_patch_parse_rejects_color_levels_above_the_bounded_palette_cap() {
    let opts = options_dict([(
        "color_levels",
        Object::from(i64::from(MAX_COLOR_LEVELS) + 1),
    )]);

    let err = RuntimeOptionsPatch::parse(&opts).expect_err("expected parse failure");
    assert!(
        err.to_string().contains("color_levels"),
        "unexpected error: {err}"
    );
    assert!(
        err.to_string().contains("between 1 and 256"),
        "unexpected error: {err}"
    );
}

#[test]
fn runtime_options_patch_parse_rejects_cterm_cursor_colors_arrays_above_the_palette_cap() {
    let colors = Array::from_iter(
        (0..=MAX_COLOR_LEVELS).map(|index| Object::from(i64::from((index % 256) as u16))),
    );
    let opts = options_dict([("cterm_cursor_colors", Object::from(colors))]);

    let err = RuntimeOptionsPatch::parse(&opts).expect_err("expected parse failure");
    assert!(
        err.to_string().contains("cterm_cursor_colors"),
        "unexpected error: {err}"
    );
    assert!(
        err.to_string().contains("at most 256 entries"),
        "unexpected error: {err}"
    );
}

#[test]
fn runtime_options_patch_parse_accepts_buffer_perf_mode() {
    let opts = options_dict([("buffer_perf_mode", Object::from("fast"))]);

    let patch = RuntimeOptionsPatch::parse(&opts).expect("expected parse success");

    assert_eq!(patch.runtime.buffer_perf_mode, Some(BufferPerfMode::Fast));
}

#[test]
fn runtime_options_patch_parse_normalizes_logging_level_to_enum() {
    let opts = options_dict([("logging_level", Object::from(5_i64))]);

    let patch = RuntimeOptionsPatch::parse(&opts).expect("expected parse success");

    assert_eq!(patch.runtime.logging_level, Some(LogLevel::Off));
}

#[test]
fn runtime_options_patch_parse_rejects_non_positive_simulation_hz() {
    let opts = options_dict([("simulation_hz", Object::from(0.0_f64))]);

    let err = RuntimeOptionsPatch::parse(&opts).expect_err("expected parse failure");
    assert!(
        err.to_string().contains("simulation_hz"),
        "unexpected error: {err}"
    );
    assert!(
        err.to_string().contains("positive number"),
        "unexpected error: {err}"
    );
}

#[test]
fn runtime_options_patch_parse_rejects_top_k_per_cell_out_of_u8_range() {
    let opts = options_dict([("top_k_per_cell", Object::from(999_i64))]);

    let err = RuntimeOptionsPatch::parse(&opts).expect_err("expected parse failure");
    assert!(
        err.to_string().contains("top_k_per_cell"),
        "unexpected error: {err}"
    );
    assert!(
        err.to_string().contains("between 2 and 255"),
        "unexpected error: {err}"
    );
}

#[test]
fn runtime_options_patch_parse_rejects_unknown_buffer_perf_mode() {
    let opts = options_dict([("buffer_perf_mode", Object::from("minimal"))]);

    let err = RuntimeOptionsPatch::parse(&opts).expect_err("expected parse failure");
    assert!(
        err.to_string().contains("buffer_perf_mode"),
        "unexpected error: {err}"
    );
    assert!(
        err.to_string().contains("one of: auto, full, fast, off"),
        "unexpected error: {err}"
    );
}
