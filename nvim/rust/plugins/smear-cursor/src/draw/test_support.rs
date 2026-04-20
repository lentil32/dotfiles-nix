use super::context::clear_draw_context_for_test;
use crate::core::types::StrokeId;
use crate::mutex::lock_with_poison_recovery;
use crate::position::RenderPoint;
use crate::types::BASE_TIME_INTERVAL;
use crate::types::ModeClass;
use crate::types::RenderFrame;
use crate::types::RenderStepSample;
use crate::types::StaticRenderConfig;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::Mutex;

static DRAW_TEST_MUTEX: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

struct DrawContextResetGuard;

impl Drop for DrawContextResetGuard {
    fn drop(&mut self) {
        clear_draw_context_for_test();
    }
}

pub(super) fn with_isolated_draw_context<T>(test: impl FnOnce() -> T) -> T {
    let _guard = lock_with_poison_recovery(&DRAW_TEST_MUTEX, |_| (), |_| {});
    clear_draw_context_for_test();
    let _reset = DrawContextResetGuard;
    test()
}

pub(super) fn base_frame() -> RenderFrame {
    let corners = [
        RenderPoint {
            row: 10.0,
            col: 10.0,
        },
        RenderPoint {
            row: 10.0,
            col: 11.0,
        },
        RenderPoint {
            row: 11.0,
            col: 11.0,
        },
        RenderPoint {
            row: 11.0,
            col: 10.0,
        },
    ];
    RenderFrame {
        mode: ModeClass::NormalLike,
        corners,
        step_samples: vec![RenderStepSample::new(corners, BASE_TIME_INTERVAL)].into(),
        planner_idle_steps: 0,
        target: RenderPoint {
            row: 10.0,
            col: 10.0,
        },
        target_corners: corners,
        vertical_bar: false,
        trail_stroke_id: StrokeId::new(1),
        retarget_epoch: 0,
        particle_count: 0,
        aggregated_particle_cells: Arc::default(),
        particle_screen_cells: Arc::default(),
        color_at_cursor: None,
        projection_policy_revision: crate::core::types::ProjectionPolicyRevision::INITIAL,
        static_config: Arc::new(StaticRenderConfig {
            cursor_color: None,
            cursor_color_insert_mode: None,
            normal_bg: None,
            transparent_bg_fallback_color: "#303030".to_string(),
            cterm_cursor_colors: None,
            cterm_bg: None,
            hide_target_hack: false,
            max_kept_windows: 32,
            never_draw_over_target: false,
            particle_max_lifetime: 1.0,
            particle_switch_octant_braille: 0.3,
            particles_over_text: true,
            color_levels: 16,
            gamma: 2.2,
            block_aspect_ratio: crate::config::DEFAULT_BLOCK_ASPECT_RATIO,
            tail_duration_ms: 180.0,
            simulation_hz: 120.0,
            trail_thickness: 1.0,
            trail_thickness_x: 1.0,
            spatial_coherence_weight: 1.0,
            temporal_stability_weight: 0.12,
            top_k_per_cell: 5,
            windows_zindex: 200,
        }),
    }
}
