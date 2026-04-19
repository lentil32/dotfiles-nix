use super::*;
use crate::test_support::proptest::mode_case;
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

fn ready_state_for_observation_base_case(
    normal_mode_uses_sampling: bool,
    insert_mode_uses_sampling: bool,
    particles_enabled: bool,
    particles_over_text: bool,
    smear_to_cmd: bool,
    retain_cursor_color: bool,
) -> CoreState {
    let ready = ready_state_with_runtime_config(|runtime| {
        runtime.config.cursor_color = cursor_color_setting(normal_mode_uses_sampling);
        runtime.config.cursor_color_insert_mode = cursor_color_setting(insert_mode_uses_sampling);
        runtime.config.particles_enabled = particles_enabled;
        runtime.config.particles_over_text = particles_over_text;
        runtime.config.smear_to_cmd = smear_to_cmd;
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
    fn prop_observation_base_collection_preserves_probe_selection_order_and_mode_gating(
        normal_mode_uses_sampling in any::<bool>(),
        insert_mode_uses_sampling in any::<bool>(),
        particles_enabled in any::<bool>(),
        particles_over_text in any::<bool>(),
        smear_to_cmd in any::<bool>(),
        retain_cursor_color in any::<bool>(),
        buffer_perf_class in buffer_perf_class_strategy(),
        mode in mode_case(),
    ) {
        let ready = ready_state_for_observation_base_case(
            normal_mode_uses_sampling,
            insert_mode_uses_sampling,
            particles_enabled,
            particles_over_text,
            smear_to_cmd,
            retain_cursor_color,
        );
        let observing = reduce(
            &ready,
            external_demand_event_with_perf_class(
                ExternalDemandKind::ExternalCursor,
                25,
                buffer_perf_class,
            ),
        )
        .next;
        let request = active_request(&observing);
        let basis = observation_basis_in_mode(Some(cursor(7, 8)), 26, mode.mode());
        let based =
            collect_observation_base(&observing, &request, basis.clone(), observation_motion());
        let observation = based
            .next
            .observation()
            .expect("observation snapshot should stay available during planning");
        let request_needs_cursor_color = request.probes().cursor_color();
        let mode_needs_cursor_color = ready
            .runtime()
            .config
            .requires_cursor_color_sampling_for_mode(mode.mode());
        let expected_probe_policy = expected_probe_policy(
            request.demand().kind(),
            request.demand().buffer_perf_class(),
            observation_cursor_color_fallback(&observing).as_ref(),
        );

        if request_needs_cursor_color && mode_needs_cursor_color {
            prop_assert_eq!(based.next.lifecycle(), Lifecycle::Observing);
            prop_assert_eq!(
                based.effects,
                vec![Effect::RequestProbe(RequestProbeEffect {
                    observation_id: request.observation_id(),
                    observation_basis: Box::new(basis),
                    cursor_color_probe_generations: observation
                        .cursor_color_probe_generations(),
                    kind: ProbeKind::CursorColor,
                    cursor_position_policy: cursor_position_policy(&observing),
                    buffer_perf_class: request.demand().buffer_perf_class(),
                    probe_policy: expected_probe_policy,
                    background_chunk: None,
                    cursor_color_fallback: observation_cursor_color_fallback(&observing),
                })],
            );
            prop_assert!(observation.probes().cursor_color().is_pending());
            return Ok(());
        }

        match based.effects.as_slice() {
            [Effect::RequestProbe(RequestProbeEffect {
                observation_id,
                observation_basis,
                kind: ProbeKind::Background,
                cursor_position_policy: effect_cursor_position_policy,
                buffer_perf_class: effect_perf_class,
                probe_policy,
                background_chunk,
                cursor_color_fallback,
                ..
            })] => {
                let expected_background_chunk = observation.probes().background().next_chunk();
                prop_assert_eq!(based.next.lifecycle(), Lifecycle::Observing);
                prop_assert!(request.probes().background());
                prop_assert_eq!(observation_id, &request.observation_id());
                prop_assert_eq!(observation_basis.as_ref(), &basis);
                prop_assert_eq!(
                    effect_cursor_position_policy,
                    &cursor_position_policy(&observing)
                );
                prop_assert_eq!(effect_perf_class, &request.demand().buffer_perf_class());
                prop_assert_eq!(probe_policy, &expected_probe_policy);
                prop_assert_eq!(background_chunk, &expected_background_chunk);
                prop_assert_eq!(cursor_color_fallback, &None);
            }
            [Effect::RequestRenderPlan(_)] => {
                prop_assert_eq!(based.next.lifecycle(), Lifecycle::Planning);
            }
            other => prop_assert!(
                false,
                "unexpected observation-base effects for mode {:?}: {other:?}",
                mode.mode()
            ),
        }

        if request_needs_cursor_color {
            match observation.probes().cursor_color() {
                ProbeSlot::Requested(ProbeState::Ready { reuse, value, .. }) => {
                    prop_assert_eq!(reuse, &ProbeReuse::Exact);
                    prop_assert_eq!(value, &None);
                    prop_assert_eq!(observation.cursor_color(), None);
                }
                other => prop_assert!(
                    false,
                    "expected completed cursor color probe after mode-gated skip, got {other:?}"
                ),
            }
        } else {
            prop_assert!(matches!(
                observation.probes().cursor_color(),
                ProbeSlot::Unrequested
            ));
        }
    }
}

fn observing_state_with_latest_exact_cursor_cell(
    latest_exact_cursor_cell: Option<ScreenCell>,
) -> (CoreState, PendingObservation) {
    let ready = ready_state().with_latest_exact_cursor_cell(latest_exact_cursor_cell);
    let observing = reduce(
        &ready,
        Event::ExternalDemandQueued(ExternalDemandQueuedEvent {
            kind: ExternalDemandKind::ExternalCursor,
            observed_at: Millis::new(25),
            buffer_perf_class: BufferPerfClass::Full,
            ingress_cursor_presentation: None,
            ingress_observation_surface: None,
        }),
    )
    .next;
    let request = observing
        .pending_observation()
        .cloned()
        .expect("active observation");
    (observing, request)
}

#[test]
fn observation_base_collection_stores_exact_cursor_reads_and_refreshes_exact_anchor() {
    let previous_exact_anchor = cursor(3, 4);
    let exact_cell = cursor(7, 8);
    let (observing, request) =
        observing_state_with_latest_exact_cursor_cell(Some(previous_exact_anchor));

    let transition = collect_observation_base(
        &observing,
        &request,
        observation_basis_with_observed_cell(
            crate::position::ObservedCell::Exact(exact_cell),
            26,
            "n",
        ),
        observation_motion(),
    );

    let observation = transition
        .next
        .observation()
        .expect("exact cursor read should be retained");
    pretty_assert_eq!(
        observation.basis().cursor().cell(),
        crate::position::ObservedCell::Exact(exact_cell)
    );
    pretty_assert_eq!(observation.basis().cursor_position(), Some(exact_cell));
    pretty_assert_eq!(observation.exact_cursor_position(), Some(exact_cell));
    pretty_assert_eq!(transition.next.latest_exact_cursor_cell(), Some(exact_cell));
}

#[test]
fn observation_base_collection_stores_deferred_cursor_reads_without_overwriting_exact_anchor() {
    let previous_exact_anchor = cursor(3, 4);
    let deferred_cell = cursor(7, 8);
    let (observing, request) =
        observing_state_with_latest_exact_cursor_cell(Some(previous_exact_anchor));

    let transition = collect_observation_base(
        &observing,
        &request,
        observation_basis_with_observed_cell(
            crate::position::ObservedCell::Deferred(deferred_cell),
            26,
            "n",
        ),
        observation_motion(),
    );

    let observation = transition
        .next
        .observation()
        .expect("deferred cursor read should be retained");
    pretty_assert_eq!(
        observation.basis().cursor().cell(),
        crate::position::ObservedCell::Deferred(deferred_cell)
    );
    pretty_assert_eq!(observation.basis().cursor_position(), Some(deferred_cell));
    pretty_assert_eq!(observation.exact_cursor_position(), None);
    assert!(observation.requires_exact_cursor_position_refresh());
    pretty_assert_eq!(
        transition.next.latest_exact_cursor_cell(),
        Some(previous_exact_anchor)
    );
}

#[test]
fn observation_base_collection_stores_unavailable_cursor_reads_without_fabricating_exact_anchor() {
    let previous_exact_anchor = cursor(3, 4);
    let (observing, request) =
        observing_state_with_latest_exact_cursor_cell(Some(previous_exact_anchor));

    let transition = collect_observation_base(
        &observing,
        &request,
        observation_basis_with_observed_cell(crate::position::ObservedCell::Unavailable, 26, "n"),
        observation_motion(),
    );

    let observation = transition
        .next
        .observation()
        .expect("unavailable cursor read should be retained");
    pretty_assert_eq!(
        observation.basis().cursor().cell(),
        crate::position::ObservedCell::Unavailable
    );
    pretty_assert_eq!(observation.basis().cursor_position(), None);
    pretty_assert_eq!(observation.exact_cursor_position(), None);
    assert!(!observation.requires_exact_cursor_position_refresh());
    pretty_assert_eq!(
        transition.next.latest_exact_cursor_cell(),
        Some(previous_exact_anchor)
    );
}
