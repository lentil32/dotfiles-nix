use super::*;
use crate::core::state::CursorTextContextBoundary;
use crate::test_support::proptest::pure_config;
use proptest::prelude::*;

fn cursor_color_setting(requires_sampling: bool) -> Option<String> {
    Some(if requires_sampling {
        "none".to_string()
    } else {
        "#112233".to_string()
    })
}

fn buffer_perf_class_strategy() -> impl Strategy<Value = BufferPerfClass> {
    prop_oneof![
        Just(BufferPerfClass::Full),
        Just(BufferPerfClass::FastMotion),
        Just(BufferPerfClass::Skip),
    ]
}

#[expect(
    clippy::too_many_arguments,
    reason = "property fixtures keep the request-planning dimensions explicit at the callsite"
)]
fn ready_state_for_observation_request_case(
    normal_mode_uses_sampling: bool,
    insert_mode_uses_sampling: bool,
    particles_enabled: bool,
    particles_over_text: bool,
    scroll_buffer_space: bool,
    smear_to_cmd: bool,
    initialize_cursor: bool,
    tracked_row: u32,
    tracked_col: u32,
    window_handle: i64,
    buffer_handle: i64,
    top_row: i64,
    line: i64,
    retain_cursor_color: bool,
) -> CoreState {
    let ready = ready_state_with_runtime_config(|runtime| {
        runtime.config.cursor_color = cursor_color_setting(normal_mode_uses_sampling);
        runtime.config.cursor_color_insert_mode = cursor_color_setting(insert_mode_uses_sampling);
        runtime.config.particles_enabled = particles_enabled;
        runtime.config.particles_over_text = particles_over_text;
        runtime.config.scroll_buffer_space = scroll_buffer_space;
        runtime.config.smear_to_cmd = smear_to_cmd;
        if initialize_cursor {
            runtime.initialize_cursor(
                Point {
                    row: f64::from(tracked_row),
                    col: f64::from(tracked_col),
                },
                CursorShape::new(false, false),
                7,
                &CursorLocation::new(window_handle, buffer_handle, top_row, line),
            );
        }
    });

    if retain_cursor_color {
        ready
            .with_ready_observation(observation_snapshot_with_cursor_color(
                cursor(7, 8),
                0x00AB_CDEF,
            ))
            .expect("primed state should accept a retained ready observation")
    } else {
        ready
    }
}

proptest! {
    #![proptest_config(pure_config())]

    #[test]
    fn prop_observation_request_planning_uses_runtime_probe_selection_and_context(
        normal_mode_uses_sampling in any::<bool>(),
        insert_mode_uses_sampling in any::<bool>(),
        particles_enabled in any::<bool>(),
        particles_over_text in any::<bool>(),
        scroll_buffer_space in any::<bool>(),
        smear_to_cmd in any::<bool>(),
        initialize_cursor in any::<bool>(),
        tracked_row in 1_u32..=40_u32,
        tracked_col in 1_u32..=120_u32,
        window_handle in 1_i64..=64_i64,
        buffer_handle in 1_i64..=64_i64,
        top_row in 1_i64..=20_i64,
        line in 1_i64..=200_i64,
        retain_cursor_color in any::<bool>(),
        buffer_perf_class in buffer_perf_class_strategy(),
        observed_at in 0_u64..=500_u64,
    ) {
        let ready = ready_state_for_observation_request_case(
            normal_mode_uses_sampling,
            insert_mode_uses_sampling,
            particles_enabled,
            particles_over_text,
            scroll_buffer_space,
            smear_to_cmd,
            initialize_cursor,
            tracked_row,
            tracked_col,
            window_handle,
            buffer_handle,
            top_row,
            line,
            retain_cursor_color,
        );

        let transition = reduce(
            &ready,
            external_demand_event_with_perf_class(
                ExternalDemandKind::ExternalCursor,
                observed_at,
                None,
                buffer_perf_class,
            ),
        );

        let mut expected_probes = ProbeRequestSet::none();
        if normal_mode_uses_sampling || insert_mode_uses_sampling {
            expected_probes = expected_probes.with_requested(ProbeKind::CursorColor);
        }
        if particles_enabled
            && !particles_over_text
            && buffer_perf_class.keeps_ornamental_effects()
        {
            expected_probes = expected_probes.with_requested(ProbeKind::Background);
        }

        let expected_request = PendingObservation::new(
            ExternalDemand::new(
                IngressSeq::new(1),
                ExternalDemandKind::ExternalCursor,
                Millis::new(observed_at),
                None,
                buffer_perf_class,
            ),
            expected_probes,
        );

        let mut expected_effects = Vec::new();
        if retain_cursor_color {
            expected_effects.push(Effect::RecordEventLoopMetric(
                EventLoopMetricEffect::IngressCoalesced,
            ));
        }
        expected_effects.push(Effect::RequestObservationBase(RequestObservationBaseEffect {
            request: expected_request,
            context: observation_runtime_context_with_perf_class(
                &ready,
                ExternalDemandKind::ExternalCursor,
                buffer_perf_class,
            ),
        }));

        prop_assert_eq!(transition.next.lifecycle(), Lifecycle::Observing);
        prop_assert_eq!(
            transition.effects,
            with_cleanup_invalidation(&transition.next, observed_at, expected_effects),
        );
    }
}

#[test]
fn retained_cursor_text_context_boundary_is_carried_into_observation_runtime_context() {
    let request = observation_request(9, ExternalDemandKind::ExternalCursor, 90);
    let retained = ObservationSnapshot::new(
        request,
        observation_basis_with_text_context(
            Some(cursor(9, 9)),
            91,
            9,
            10,
            &["before", "alpha", "after"],
            None,
        ),
        observation_motion(),
    );
    let ready = ready_state()
        .with_ready_observation(retained)
        .expect("primed state should accept a retained ready observation");

    let transition = reduce(
        &ready,
        external_demand_event(ExternalDemandKind::ExternalCursor, 100, None),
    );

    let Some(Effect::RequestObservationBase(payload)) = transition
        .effects
        .iter()
        .find(|effect| matches!(effect, Effect::RequestObservationBase(_)))
    else {
        panic!("expected observation base request effect");
    };

    pretty_assert_eq!(
        payload.context.cursor_text_context_boundary(),
        Some(CursorTextContextBoundary::new(22, 10))
    );
}

#[test]
fn retained_cursor_text_context_boundary_survives_without_sampled_rows() {
    let request = observation_request(9, ExternalDemandKind::ExternalCursor, 90);
    let retained = ObservationSnapshot::new(
        request,
        observation_basis_with_text_context_boundary(
            Some(cursor(9, 9)),
            91,
            9,
            CursorTextContextBoundary::new(22, 10),
        ),
        observation_motion(),
    );
    let ready = ready_state()
        .with_ready_observation(retained)
        .expect("primed state should accept a retained ready observation");

    let transition = reduce(
        &ready,
        external_demand_event(ExternalDemandKind::ExternalCursor, 100, None),
    );

    let Some(Effect::RequestObservationBase(payload)) = transition
        .effects
        .iter()
        .find(|effect| matches!(effect, Effect::RequestObservationBase(_)))
    else {
        panic!("expected observation base request effect");
    };

    pretty_assert_eq!(
        payload.context.cursor_text_context_boundary(),
        Some(CursorTextContextBoundary::new(22, 10))
    );
}
