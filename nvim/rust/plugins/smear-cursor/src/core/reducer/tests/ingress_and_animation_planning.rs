use super::*;
use crate::test_support::proptest::pure_config;
use proptest::prelude::*;

fn expected_ingress_presentation_effect(
    cell: ScreenCell,
    shape: crate::types::CursorCellShape,
    zindex: u32,
) -> IngressCursorPresentationEffect {
    IngressCursorPresentationEffect::HideCursorAndPrepaint {
        cell,
        shape,
        zindex,
    }
}

fn cursor_cell_shape() -> impl Strategy<Value = crate::types::CursorCellShape> {
    prop_oneof![
        Just(crate::types::CursorCellShape::Block),
        Just(crate::types::CursorCellShape::VerticalBar),
        Just(crate::types::CursorCellShape::HorizontalBar),
    ]
}

#[test]
fn cursor_autocmd_demands_refresh_ingress_policy_state() {
    let transition = reduce(
        &ready_state(),
        Event::ExternalDemandQueued(ExternalDemandQueuedEvent {
            kind: ExternalDemandKind::ModeChanged,
            observed_at: Millis::new(77),
            requested_target: None,
            buffer_perf_class: BufferPerfClass::Full,
            ingress_cursor_presentation: None,
        }),
    );

    pretty_assert_eq!(
        transition.next.ingress_policy().last_cursor_autocmd_at(),
        Some(Millis::new(77))
    );
}

#[test]
fn cursor_ingress_emits_explicit_presentation_effect_before_observation_request() {
    let state = ready_state();
    let cell = ScreenCell::new(6, 9).expect("valid test cell");

    let transition = reduce(
        &state,
        Event::ExternalDemandQueued(ExternalDemandQueuedEvent {
            kind: ExternalDemandKind::ExternalCursor,
            observed_at: Millis::new(78),
            requested_target: None,
            buffer_perf_class: BufferPerfClass::Full,
            ingress_cursor_presentation: Some(IngressCursorPresentationRequest::new(
                true,
                true,
                Some(cell),
                crate::types::CursorCellShape::Block,
            )),
        }),
    );

    pretty_assert_eq!(transition.next.lifecycle(), Lifecycle::Observing);
    pretty_assert_eq!(
        transition.effects,
        with_cleanup_invalidation(
            &transition.next,
            78,
            vec![
                Effect::ApplyIngressCursorPresentation(expected_ingress_presentation_effect(
                    cell,
                    crate::types::CursorCellShape::Block,
                    state.runtime().config.windows_zindex,
                )),
                Effect::RequestObservationBase(RequestObservationBaseEffect {
                    request: observation_request(1, ExternalDemandKind::ExternalCursor, 78),
                    context: observation_runtime_context(
                        &state,
                        ExternalDemandKind::ExternalCursor,
                    ),
                }),
            ],
        )
    );
}

proptest! {
    #![proptest_config(pure_config())]

    #[test]
    fn prop_cursor_ingress_preserves_requested_presentation_shape(
        shape in cursor_cell_shape(),
    ) {
        let state = ready_state();
        let cell = ScreenCell::new(6, 9).expect("valid test cell");

        let transition = reduce(
            &state,
            Event::ExternalDemandQueued(ExternalDemandQueuedEvent {
                kind: ExternalDemandKind::ExternalCursor,
                observed_at: Millis::new(78),
                requested_target: None,
                buffer_perf_class: BufferPerfClass::Full,
                ingress_cursor_presentation: Some(IngressCursorPresentationRequest::new(
                    true,
                    true,
                    Some(cell),
                    shape,
                )),
            }),
        );

        let [Effect::ApplyIngressCursorPresentation(effect), ..] = transition.effects.as_slice() else {
            panic!("expected ingress presentation effect before observation request");
        };
        prop_assert_eq!(
            *effect,
            expected_ingress_presentation_effect(
                cell,
                shape,
                state.runtime().config.windows_zindex,
            )
        );
    }
}

#[test]
fn animation_timer_from_ready_enters_planning_and_requests_render_plan() {
    let base = ready_state_with_observation(cursor(9, 9));
    let (state, token) = timer_armed_state(base);

    let transition = reduce(&state, animation_tick_event(token, 50));

    pretty_assert_eq!(transition.next.lifecycle(), Lifecycle::Planning);
    assert!(transition.next.pending_proposal().is_none());
    assert!(transition.next.pending_plan_proposal_id().is_some());
    assert!(matches!(
        transition.effects.as_slice(),
        [Effect::RequestRenderPlan(payload)] if payload.requested_at == Millis::new(50)
    ));
}

#[test]
fn animation_timer_uses_timer_timestamp_when_observation_clock_is_stale() {
    let request = observation_request(9, ExternalDemandKind::ExternalCursor, 100);
    let observation = ObservationSnapshot::new(
        request.clone(),
        observation_basis(&request, Some(cursor(9, 9)), 100),
        observation_motion(),
    );
    let mut runtime = ready_state().runtime().clone();
    runtime.initialize_cursor(
        Point { row: 9.0, col: 9.0 },
        CursorShape::new(false, false),
        7,
        &CursorLocation::new(11, 22, 3, 4),
    );
    runtime.start_tail_drain(2);
    runtime.set_last_tick_ms(Some(100.0));
    let base = ready_state()
        .with_last_cursor(Some(cursor(9, 9)))
        .with_runtime(runtime)
        .into_ready_with_observation(observation);
    let (state, token) = timer_armed_state(base);

    let transition = reduce(&state, animation_tick_event(token, 116));

    let [Effect::RequestRenderPlan(payload)] = transition.effects.as_slice() else {
        panic!("expected render plan request after stale-clock animation tick");
    };
    pretty_assert_eq!(transition.next.lifecycle(), Lifecycle::Planning);
    let planned_render = crate::core::reducer::build_planned_render(
        &payload.planning_state,
        payload.proposal_id,
        &payload.render_decision,
        payload.animation_schedule,
    )
    .expect("animation planning should preserve proposal shape invariants");
    let proposal = planned_render.proposal();
    let RealizationPlan::Draw(_) = proposal.realization() else {
        panic!(
            "expected draw proposal after timer-driven drain progress, got {:?}",
            proposal.realization()
        );
    };
    let next_animation_at = proposal
        .animation_schedule()
        .deadline()
        .expect("draw proposal should schedule another animation tick");
    assert!(
        next_animation_at.value() > 116,
        "next animation deadline should advance from timer time, got {}",
        next_animation_at.value()
    );
}

#[test]
fn animation_timer_keeps_rendering_compatible_cursor_color_observation_while_runtime_is_animating()
{
    let base = compatible_cursor_color_ready_state(|runtime| {
        runtime.start_animation();
        runtime.set_last_tick_ms(Some(100.0));
    });
    let (state, token) = timer_armed_state(base);

    let transition = reduce(&state, animation_tick_event(token, 116));

    pretty_assert_eq!(transition.next.lifecycle(), Lifecycle::Planning);
    assert!(matches!(
        transition.effects.as_slice(),
        [Effect::RequestRenderPlan(payload)] if payload.requested_at == Millis::new(116)
    ));
}

#[test]
fn animation_timer_requests_boundary_refresh_for_compatible_cursor_color_when_motion_stops() {
    let base = compatible_cursor_color_ready_state(|_runtime| {});
    let (state, token) = timer_armed_state(base);

    let transition = reduce(&state, animation_tick_event(token, 116));

    let [Effect::RequestObservationBase(payload)] = transition.effects.as_slice() else {
        panic!("expected boundary refresh observation after compatible cursor color reuse");
    };
    let request = active_request(&transition.next);

    pretty_assert_eq!(transition.next.lifecycle(), Lifecycle::Observing);
    pretty_assert_eq!(payload.request, request);
    pretty_assert_eq!(payload.context.buffer_perf_class(), BufferPerfClass::Full);
    pretty_assert_eq!(
        payload.context.probe_policy().quality(),
        ProbeQuality::Exact
    );
    pretty_assert_eq!(request.demand().kind(), ExternalDemandKind::BoundaryRefresh);
    pretty_assert_eq!(request.demand().buffer_perf_class(), BufferPerfClass::Full);
    pretty_assert_eq!(request.demand().requested_target(), Some(cursor(9, 9)));
}

#[test]
fn animation_timer_preserves_perf_class_across_boundary_refresh() {
    let mut runtime = ready_state().runtime().clone();
    runtime.config.cursor_color = Some("none".to_string());
    runtime.initialize_cursor(
        Point { row: 9.0, col: 9.0 },
        CursorShape::new(false, false),
        7,
        &CursorLocation::new(11, 22, 3, 9),
    );
    let request = ObservationRequest::new(
        ExternalDemand::new(
            IngressSeq::new(9),
            ExternalDemandKind::ExternalCursor,
            Millis::new(90),
            None,
            BufferPerfClass::FastMotion,
        ),
        ProbeRequestSet::new(true, false),
    );
    let observation = ObservationSnapshot::new(
        request.clone(),
        observation_basis(&request, Some(cursor(9, 9)), 91),
        observation_motion(),
    )
    .with_cursor_color_probe(ProbeState::ready(
        ProbeKind::CursorColor.request_id(request.observation_id()),
        request.observation_id(),
        ProbeReuse::Compatible,
        Some(CursorColorSample::new(0x00AB_CDEF)),
    ))
    .expect("cursor color probe should be requested");
    let base = ready_state()
        .with_last_cursor(Some(cursor(9, 9)))
        .with_runtime(runtime)
        .into_ready_with_observation(observation);
    let (state, token) = timer_armed_state(base);

    let transition = reduce(&state, animation_tick_event(token, 116));

    let [Effect::RequestObservationBase(payload)] = transition.effects.as_slice() else {
        panic!("expected boundary refresh observation after compatible cursor color reuse");
    };
    let next_request = active_request(&transition.next);

    pretty_assert_eq!(transition.next.lifecycle(), Lifecycle::Observing);
    pretty_assert_eq!(payload.request, next_request);
    pretty_assert_eq!(
        payload.context.buffer_perf_class(),
        BufferPerfClass::FastMotion
    );
    pretty_assert_eq!(
        payload.context.probe_policy().quality(),
        ProbeQuality::Exact
    );
    pretty_assert_eq!(
        next_request.demand().buffer_perf_class(),
        BufferPerfClass::FastMotion
    );
    pretty_assert_eq!(
        next_request.demand().kind(),
        ExternalDemandKind::BoundaryRefresh
    );
}

#[test]
fn animation_timer_requests_boundary_refresh_for_conceal_deferred_cursor_position() {
    let base = conceal_deferred_cursor_ready_state(|_runtime| {});
    let (state, token) = timer_armed_state(base);

    let transition = reduce(&state, animation_tick_event(token, 116));

    let [Effect::RequestObservationBase(payload)] = transition.effects.as_slice() else {
        panic!("expected boundary refresh observation after deferred conceal correction");
    };
    let request = active_request(&transition.next);

    pretty_assert_eq!(transition.next.lifecycle(), Lifecycle::Observing);
    pretty_assert_eq!(payload.request, request);
    pretty_assert_eq!(
        payload.context.buffer_perf_class(),
        BufferPerfClass::FastMotion
    );
    pretty_assert_eq!(
        payload.context.probe_policy().quality(),
        ProbeQuality::Exact
    );
    pretty_assert_eq!(request.demand().kind(), ExternalDemandKind::BoundaryRefresh);
    pretty_assert_eq!(
        request.demand().buffer_perf_class(),
        BufferPerfClass::FastMotion
    );
    pretty_assert_eq!(request.demand().requested_target(), Some(cursor(9, 9)));
}

#[test]
fn animation_timer_skips_boundary_refresh_when_current_mode_has_explicit_cursor_color() {
    let base = compatible_cursor_color_ready_state(|runtime| {
        runtime.config.cursor_color = Some("#112233".to_string());
        runtime.config.cursor_color_insert_mode = Some("none".to_string());
    });
    let (state, token) = timer_armed_state(base);

    let transition = reduce(&state, animation_tick_event(token, 116));

    pretty_assert_eq!(transition.next.lifecycle(), Lifecycle::Planning);
    assert!(matches!(
        transition.effects.as_slice(),
        [Effect::RequestRenderPlan(payload)] if payload.requested_at == Millis::new(116)
    ));
}

#[test]
fn idle_apply_completion_requests_boundary_refresh_for_compatible_cursor_color() {
    let (applying, proposal_id) = applying_state_with_realization_plan(
        compatible_cursor_color_ready_state(|_runtime| {}),
        noop_realization_plan(),
        false,
        None,
    );

    let transition = reduce(
        &applying,
        Event::ApplyReported(ApplyReport::AppliedFully {
            proposal_id,
            observed_at: Millis::new(101),
            visual_change: false,
        }),
    );

    let request = active_request(&transition.next);
    pretty_assert_eq!(transition.next.lifecycle(), Lifecycle::Observing);
    pretty_assert_eq!(request.demand().kind(), ExternalDemandKind::BoundaryRefresh);
    assert!(transition.effects.iter().any(|effect| {
        matches!(
            effect,
            Effect::RequestObservationBase(payload) if payload.request == request
                && payload.context.probe_policy().quality() == ProbeQuality::Exact
        )
    }));
}
