use std::io::Cursor;

use pretty_assertions::assert_eq;

use super::Schema;
use super::load_summary_value_from_reader;

#[test]
fn window_switch_summary_merges_phase_and_diagnostics_events() {
    let log = concat!(
        "ignored\n",
        "PERF_JSON {\"schema\":\"window-switch\",\"version\":1,\"kind\":\"scenario\",\"payload\":{\"name\":\"planner_heavy\",\"preset\":\"planner_heavy\"}}\n",
        "PERF_JSON {\"schema\":\"window-switch\",\"version\":1,\"kind\":\"phase\",\"payload\":{\"name\":\"baseline\",\"iterations\":600,\"elapsed_ms\":6.0,\"avg_us\":10.0,\"floating_windows\":2,\"visible_floating_windows\":1,\"smear_floating_windows\":1,\"visible_smear_floating_windows\":1,\"lua_memory_kib\":128.0}}\n",
        "PERF_JSON {\"schema\":\"window-switch\",\"version\":1,\"kind\":\"diagnostics\",\"payload\":{\"phase\":\"post_recovery\",\"raw\":\"smear_cursor perf_class=full\",\"fields\":{\"perf_class\":\"full\",\"buffer_line_count\":12000}}}\n",
        "PERF_JSON {\"schema\":\"window-switch\",\"version\":1,\"kind\":\"summary\",\"payload\":{\"baseline_avg_us\":10.0,\"recovery_avg_us\":11.0,\"recovery_ratio\":1.1,\"recovery_wait_mode\":\"fixed\",\"recovery_wait_elapsed_ms\":250.0,\"recovery_reached_cold\":true,\"recovery_timed_out\":false,\"post_wait_floating_windows\":4,\"post_wait_visible_floating_windows\":2,\"post_wait_smear_floating_windows\":2,\"post_wait_visible_smear_floating_windows\":1}}\n"
    );

    let summary = load_summary_value_from_reader(Schema::WindowSwitch, Cursor::new(log.as_bytes()))
        .expect("window-switch summary should parse");

    assert_eq!(summary["scenario"]["name"], "planner_heavy");
    assert_eq!(summary["phases"]["baseline"]["avg_us"], 10.0);
    assert_eq!(
        summary["diagnostics"]["post_recovery"]["perf_class"],
        "full"
    );
    assert_eq!(
        summary["summary"]["post_wait_visible_smear_floating_windows"],
        1
    );
}

#[test]
fn particle_toggle_summary_keeps_summary_fields() {
    let log = concat!(
        "PERF_JSON {\"schema\":\"particle-toggle\",\"version\":1,\"kind\":\"scenario\",\"payload\":{\"name\":\"particles_on\"}}\n",
        "PERF_JSON {\"schema\":\"particle-toggle\",\"version\":1,\"kind\":\"config\",\"payload\":{\"warmup_iterations\":600,\"benchmark_iterations\":2400,\"retarget_interval\":24,\"particles_enabled\":true,\"time_interval_ms\":8.333,\"particle_max_num\":100,\"anchor_count\":4}}\n",
        "PERF_JSON {\"schema\":\"particle-toggle\",\"version\":1,\"kind\":\"summary\",\"payload\":{\"avg_us\":14.2,\"avg_particles\":3.5,\"max_particles\":12,\"final_particles\":2,\"retargets\":100}}\n"
    );

    let summary =
        load_summary_value_from_reader(Schema::ParticleToggle, Cursor::new(log.as_bytes()))
            .expect("particle-toggle summary should parse");

    assert_eq!(summary["scenario"]["name"], "particles_on");
    assert_eq!(summary["config"]["particles_enabled"], true);
    assert_eq!(summary["summary"]["max_particles"], 12);
}
