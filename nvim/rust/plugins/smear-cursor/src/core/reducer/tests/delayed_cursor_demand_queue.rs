use super::*;

fn delayed_ready_state() -> CoreState {
    ready_state_with_runtime_config(|runtime| {
        runtime.config.delay_event_to_smear = 40.0;
    })
}

#[test]
fn first_cursor_demand_arms_the_ingress_timer_before_observing() {
    let ready = delayed_ready_state();

    let first = reduce(
        &ready,
        external_demand_event(ExternalDemandKind::ExternalCursor, 20),
    );

    let ingress_token = first
        .next
        .timers()
        .active_token(TimerId::Ingress)
        .expect("cursor ingress delay should arm the ingress timer");
    pretty_assert_eq!(first.next.lifecycle(), ready.lifecycle());
    pretty_assert_eq!(
        first.effects,
        with_cleanup_invalidation(
            &first.next,
            20,
            vec![Effect::ScheduleTimer(ScheduleTimerEffect {
                token: ingress_token,
                delay: DelayBudgetMs::try_new(40).expect("ingress delay budget"),
                requested_at: Millis::new(20),
            })],
        )
    );
    assert!(first.next.demand_queue().latest_cursor().is_some());
    pretty_assert_eq!(
        first.next.ingress_policy().pending_delay_until(),
        Some(Millis::new(60))
    );
}

#[test]
fn delayed_cursor_timer_fire_starts_the_queued_observation() {
    let ready = delayed_ready_state();
    let delayed = reduce(
        &ready,
        external_demand_event(ExternalDemandKind::ExternalCursor, 20),
    )
    .next;
    let ingress_token = delayed
        .timers()
        .active_token(TimerId::Ingress)
        .expect("ingress timer token");

    let fired = reduce(
        &delayed,
        Event::TimerFiredWithToken(TimerFiredWithTokenEvent {
            token: ingress_token,
            observed_at: Millis::new(60),
        }),
    );

    pretty_assert_eq!(fired.next.lifecycle(), Lifecycle::Observing);
    assert!(matches!(
        fired.effects.as_slice(),
        [Effect::RequestObservationBase(
            RequestObservationBaseEffect { .. }
        )]
    ));
}

#[test]
fn newer_delayed_cursor_demand_replaces_the_pending_queue_without_rearming_the_timer() {
    let ready = delayed_ready_state();
    let first = reduce(
        &ready,
        external_demand_event(ExternalDemandKind::ExternalCursor, 20),
    );
    let first_token = first
        .next
        .timers()
        .active_token(TimerId::Ingress)
        .expect("first ingress timer token");

    let second = reduce(
        &first.next,
        external_demand_event(ExternalDemandKind::ExternalCursor, 21),
    );

    let second_token = second
        .next
        .timers()
        .active_token(TimerId::Ingress)
        .expect("existing ingress timer token");
    pretty_assert_eq!(second_token, first_token);
    pretty_assert_eq!(
        second.next.demand_queue().latest_cursor(),
        Some(&crate::core::state::QueuedDemand::ready(
            ExternalDemand::new(
                IngressSeq::new(2),
                ExternalDemandKind::ExternalCursor,
                Millis::new(21),
                BufferPerfClass::Full,
            )
        ))
    );
    pretty_assert_eq!(
        second.effects,
        vec![
            Effect::RecordEventLoopMetric(EventLoopMetricEffect::IngressCoalesced),
            Effect::RecordEventLoopMetric(EventLoopMetricEffect::DelayedIngressPendingUpdated,),
        ]
    );
    pretty_assert_eq!(
        second.next.ingress_policy().pending_delay_until(),
        Some(Millis::new(61))
    );
}

#[test]
fn delayed_cursor_burst_updates_pending_deadline_without_stale_timer_token_churn() {
    let ready = delayed_ready_state();
    let first = reduce(
        &ready,
        external_demand_event(ExternalDemandKind::ExternalCursor, 20),
    );
    let first_token = first
        .next
        .timers()
        .active_token(TimerId::Ingress)
        .expect("first ingress timer token");

    let second = reduce(
        &first.next,
        external_demand_event(ExternalDemandKind::ExternalCursor, 21),
    );
    let third = reduce(
        &second.next,
        external_demand_event(ExternalDemandKind::ExternalCursor, 22),
    );

    let third_token = third
        .next
        .timers()
        .active_token(TimerId::Ingress)
        .expect("existing ingress timer token");
    pretty_assert_eq!(third_token, first_token);
    pretty_assert_eq!(
        third.next.demand_queue().latest_cursor(),
        Some(&crate::core::state::QueuedDemand::ready(
            ExternalDemand::new(
                IngressSeq::new(3),
                ExternalDemandKind::ExternalCursor,
                Millis::new(22),
                BufferPerfClass::Full,
            )
        ))
    );
    pretty_assert_eq!(
        second.effects,
        vec![
            Effect::RecordEventLoopMetric(EventLoopMetricEffect::IngressCoalesced),
            Effect::RecordEventLoopMetric(EventLoopMetricEffect::DelayedIngressPendingUpdated,),
        ]
    );
    pretty_assert_eq!(
        third.effects,
        vec![
            Effect::RecordEventLoopMetric(EventLoopMetricEffect::IngressCoalesced),
            Effect::RecordEventLoopMetric(EventLoopMetricEffect::DelayedIngressPendingUpdated,),
        ]
    );
    assert!(
        second
            .effects
            .iter()
            .chain(third.effects.iter())
            .all(|effect| !matches!(
                effect,
                Effect::ScheduleTimer(_)
                    | Effect::RecordEventLoopMetric(EventLoopMetricEffect::StaleToken)
            )),
        "burst updates should reuse the original timer generation instead of churning timer tokens"
    );
    pretty_assert_eq!(
        third.next.ingress_policy().pending_delay_until(),
        Some(Millis::new(62))
    );
}

#[test]
fn early_delayed_cursor_timer_fire_rearms_once_for_the_remaining_deadline() {
    let ready = delayed_ready_state();
    let first = reduce(
        &ready,
        external_demand_event(ExternalDemandKind::ExternalCursor, 20),
    );
    let delayed = reduce(
        &first.next,
        external_demand_event(ExternalDemandKind::ExternalCursor, 21),
    );
    let first_token = delayed
        .next
        .timers()
        .active_token(TimerId::Ingress)
        .expect("ingress timer token");

    let early_fire = reduce(
        &delayed.next,
        Event::TimerFiredWithToken(TimerFiredWithTokenEvent {
            token: first_token,
            observed_at: Millis::new(60),
        }),
    );

    let rearmed_token = early_fire
        .next
        .timers()
        .active_token(TimerId::Ingress)
        .expect("ingress timer should be rearmed for the remaining delay");
    assert_ne!(rearmed_token, first_token);
    pretty_assert_eq!(
        early_fire.effects,
        vec![Effect::ScheduleTimer(ScheduleTimerEffect {
            token: rearmed_token,
            delay: DelayBudgetMs::try_new(1).expect("remaining ingress delay budget"),
            requested_at: Millis::new(60),
        })]
    );
    pretty_assert_eq!(
        early_fire.next.ingress_policy().pending_delay_until(),
        Some(Millis::new(61))
    );
}

#[test]
fn starting_observation_clears_the_pending_delayed_cursor_deadline() {
    let ready = delayed_ready_state();
    let delayed = reduce(
        &ready,
        external_demand_event(ExternalDemandKind::ExternalCursor, 20),
    )
    .next;
    let ingress_token = delayed
        .timers()
        .active_token(TimerId::Ingress)
        .expect("ingress timer token");

    let fired = reduce(
        &delayed,
        Event::TimerFiredWithToken(TimerFiredWithTokenEvent {
            token: ingress_token,
            observed_at: Millis::new(60),
        }),
    );

    pretty_assert_eq!(fired.next.ingress_policy().pending_delay_until(), None);
}
