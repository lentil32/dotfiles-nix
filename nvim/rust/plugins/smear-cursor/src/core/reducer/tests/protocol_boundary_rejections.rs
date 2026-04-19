use super::*;
use crate::test_support::cursor;

#[test]
fn activate_observation_rejects_phases_without_an_activation_slot() {
    let active_observation = observation_snapshot(cursor(7, 8));
    let ready = ready_state()
        .with_ready_observation(active_observation.clone())
        .expect("primed state should accept a ready observation");
    let observing = ready_state()
        .enter_observing_request(observation_request(
            17,
            ExternalDemandKind::ExternalCursor,
            32,
        ))
        .expect("primed state should stage a collecting observation request")
        .with_active_observation(active_observation.clone())
        .expect("collecting state should activate its first observation");
    let (planning_ready, planning_proposal_id) = ready.clone().allocate_proposal_id();
    let planning = planning_ready
        .enter_planning(planning_proposal_id)
        .expect("ready state should enter planning");
    let (applying, _) =
        applying_state_with_realization_plan(ready.clone(), noop_realization_plan(), false, None);

    for (label, mut state) in [
        ("idle", CoreState::default()),
        ("primed", ready_state()),
        ("observing", observing),
        ("ready", ready.clone()),
        ("planning", planning),
        ("applying", applying),
        ("recovering", ready.enter_recovering()),
    ] {
        let baseline = state.clone();

        assert!(
            !state.activate_observation(active_observation.clone()),
            "{label} should reject active observation injection",
        );
        pretty_assert_eq!(state, baseline);
    }
}

#[test]
fn enter_observing_request_rejects_phases_without_a_collecting_entry_slot() {
    let pending = observation_request(17, ExternalDemandKind::ExternalCursor, 32);
    let active_observation = observation_snapshot(cursor(7, 8));
    let collecting = ready_state()
        .enter_observing_request(pending.clone())
        .expect("primed state should stage a collecting observation request");
    let observing = collecting
        .clone()
        .with_active_observation(active_observation.clone())
        .expect("collecting state should activate its first observation");
    let ready = ready_state()
        .with_ready_observation(active_observation)
        .expect("primed state should accept a ready observation");
    let (planning_ready, planning_proposal_id) = ready.clone().allocate_proposal_id();
    let planning = planning_ready
        .enter_planning(planning_proposal_id)
        .expect("ready state should enter planning");
    let (applying, _) =
        applying_state_with_realization_plan(ready.clone(), noop_realization_plan(), false, None);

    for (label, state) in [
        ("idle", CoreState::default()),
        ("collecting", collecting),
        ("observing", observing),
        ("planning", planning),
        ("applying", applying),
        ("recovering", ready.enter_recovering()),
    ] {
        let baseline = state.clone();

        assert!(
            state
                .clone()
                .enter_observing_request(pending.clone())
                .is_none(),
            "{label} should reject pending observation injection outside primed or ready entry slots",
        );
        pretty_assert_eq!(state, baseline);
    }
}

#[test]
fn enter_ready_rejects_phases_without_a_collecting_completion_slot() {
    let active_observation = observation_snapshot(cursor(7, 8));
    let ready = ready_state()
        .with_ready_observation(active_observation.clone())
        .expect("primed state should accept a ready observation");
    let observing = ready_state()
        .enter_observing_request(observation_request(
            17,
            ExternalDemandKind::ExternalCursor,
            32,
        ))
        .expect("primed state should stage a collecting observation request")
        .with_active_observation(active_observation.clone())
        .expect("collecting state should activate its first observation");
    let (planning_ready, planning_proposal_id) = ready.clone().allocate_proposal_id();
    let planning = planning_ready
        .enter_planning(planning_proposal_id)
        .expect("ready state should enter planning");
    let (applying, _) =
        applying_state_with_realization_plan(ready.clone(), noop_realization_plan(), false, None);

    for (label, mut state) in [
        ("idle", CoreState::default()),
        ("primed", ready_state()),
        ("observing", observing),
        ("ready", ready.clone()),
        ("planning", planning),
        ("applying", applying),
        ("recovering", ready.enter_recovering()),
    ] {
        let baseline = state.clone();

        assert!(
            !state.enter_ready(active_observation.clone()),
            "{label} should reject ready-phase observation injection",
        );
        pretty_assert_eq!(state, baseline);
    }
}

#[test]
fn replace_active_observation_with_pending_rejects_phases_without_active_observation_ownership() {
    let active_observation = observation_snapshot(cursor(7, 8));
    let replacement_pending = observation_request(17, ExternalDemandKind::ExternalCursor, 32);
    let collecting = ready_state()
        .enter_observing_request(replacement_pending.clone())
        .expect("primed state should stage a collecting observation request");
    let ready = ready_state()
        .with_ready_observation(active_observation)
        .expect("primed state should accept a ready observation");
    let (planning_ready, planning_proposal_id) = ready.clone().allocate_proposal_id();
    let planning = planning_ready
        .enter_planning(planning_proposal_id)
        .expect("ready state should enter planning");
    let (applying, _) =
        applying_state_with_realization_plan(ready.clone(), noop_realization_plan(), false, None);

    for (label, mut state) in [
        ("idle", CoreState::default()),
        ("primed", ready_state()),
        ("collecting", collecting),
        ("ready", ready.clone()),
        ("planning", planning),
        ("applying", applying),
        ("recovering", ready.enter_recovering()),
    ] {
        let baseline = state.clone();

        assert!(
            !state.replace_active_observation_with_pending(replacement_pending.clone()),
            "{label} should reject replacing an active observation without owning one",
        );
        pretty_assert_eq!(state, baseline);
    }
}

#[test]
fn complete_active_observation_rejects_phases_without_an_active_completion_slot() {
    let active_observation = observation_snapshot(cursor(7, 8));
    let ready = ready_state()
        .with_ready_observation(active_observation)
        .expect("primed state should accept a ready observation");
    let collecting = ready_state()
        .enter_observing_request(observation_request(
            17,
            ExternalDemandKind::ExternalCursor,
            32,
        ))
        .expect("primed state should stage a collecting observation request");
    let (planning_ready, planning_proposal_id) = ready.clone().allocate_proposal_id();
    let planning = planning_ready
        .enter_planning(planning_proposal_id)
        .expect("ready state should enter planning");
    let (applying, _) =
        applying_state_with_realization_plan(ready.clone(), noop_realization_plan(), false, None);

    for (label, mut state) in [
        ("idle", CoreState::default()),
        ("primed", ready_state()),
        ("collecting", collecting),
        ("ready", ready.clone()),
        ("planning", planning),
        ("applying", applying),
        ("recovering", ready.enter_recovering()),
    ] {
        let baseline = state.clone();

        assert!(
            !state.complete_active_observation(),
            "{label} should reject completing an active observation without owning one",
        );
        pretty_assert_eq!(state, baseline);
    }
}

#[test]
fn enter_planning_rejects_non_ready_protocol_phases() {
    let active_observation = observation_snapshot(cursor(7, 8));
    let collecting = ready_state()
        .enter_observing_request(observation_request(
            17,
            ExternalDemandKind::ExternalCursor,
            32,
        ))
        .expect("primed state should stage a collecting observation request");
    let observing = collecting
        .clone()
        .with_active_observation(active_observation.clone())
        .expect("collecting state should activate an observation");
    let ready = ready_state()
        .with_ready_observation(active_observation)
        .expect("primed state should accept a ready observation");
    let (planning_ready, planning_proposal_id) = ready.clone().allocate_proposal_id();
    let planning = planning_ready
        .enter_planning(planning_proposal_id)
        .expect("ready state should enter planning");
    let (applying, applying_proposal_id) =
        applying_state_with_realization_plan(ready.clone(), noop_realization_plan(), false, None);

    for (label, state) in [
        ("idle", CoreState::default()),
        ("primed", ready_state()),
        ("collecting", collecting),
        ("observing", observing),
        ("planning", planning),
        ("applying", applying),
        ("recovering", ready.enter_recovering()),
    ] {
        assert!(
            state.enter_planning(applying_proposal_id).is_none(),
            "{label} should reject planning without a ready-phase observation owner",
        );
    }
}

#[test]
fn enter_applying_rejects_phase_skips_and_mismatched_proposal_ids() {
    let active_observation = observation_snapshot(cursor(7, 8));
    let collecting = ready_state()
        .enter_observing_request(observation_request(
            17,
            ExternalDemandKind::ExternalCursor,
            32,
        ))
        .expect("primed state should stage a collecting observation request");
    let observing = collecting
        .clone()
        .with_active_observation(active_observation.clone())
        .expect("collecting state should activate an observation");
    let ready = ready_state()
        .with_ready_observation(active_observation)
        .expect("primed state should accept a ready observation");
    let (planning_ready, planning_proposal_id) = ready.clone().allocate_proposal_id();
    let planning = planning_ready
        .enter_planning(planning_proposal_id)
        .expect("ready state should enter planning");
    let proposal = InFlightProposal::noop(
        planning_proposal_id,
        ScenePatch::derive(PatchBasis::new(None, None)),
        RenderCleanupAction::NoAction,
        RenderSideEffects::default(),
        crate::core::state::AnimationSchedule::Idle,
    )
    .expect("noop proposal should be constructible");

    for (label, state) in [
        ("idle", CoreState::default()),
        ("primed", ready_state()),
        ("collecting", collecting),
        ("observing", observing),
        ("ready", ready.clone()),
        (
            "applying",
            applying_state_with_realization_plan(
                ready.clone(),
                noop_realization_plan(),
                false,
                None,
            )
            .0,
        ),
        ("recovering", ready.enter_recovering()),
    ] {
        let baseline = state.clone();

        assert!(
            state.clone().enter_applying(proposal.clone()).is_none(),
            "{label} should reject applying without a matching planning slot",
        );
        pretty_assert_eq!(state, baseline);
    }

    let mismatched_proposal = InFlightProposal::noop(
        planning_proposal_id.next(),
        ScenePatch::derive(PatchBasis::new(None, None)),
        RenderCleanupAction::NoAction,
        RenderSideEffects::default(),
        crate::core::state::AnimationSchedule::Idle,
    )
    .expect("mismatched noop proposal should be constructible");
    let planning_baseline = planning.clone();

    assert!(
        planning
            .clone()
            .enter_applying(mismatched_proposal)
            .is_none(),
        "planning should reject a proposal whose id does not match the active planning slot",
    );
    pretty_assert_eq!(planning, planning_baseline);
}

#[test]
fn take_pending_proposal_rejects_wrong_shape_without_mutating_state() {
    let active_observation = observation_snapshot(cursor(7, 8));
    let ready = ready_state()
        .with_ready_observation(active_observation.clone())
        .expect("primed state should accept a ready observation");
    let (planning_ready, planning_proposal_id) = ready.clone().allocate_proposal_id();
    let planning = planning_ready
        .enter_planning(planning_proposal_id)
        .expect("ready state should enter planning");
    let (applying, applying_proposal_id) =
        applying_state_with_realization_plan(ready.clone(), noop_realization_plan(), false, None);
    let mismatched_proposal_id = applying_proposal_id.next();

    for (label, mut state, proposal_id) in [
        ("ready", ready, applying_proposal_id),
        ("planning", planning, applying_proposal_id),
        (
            "recovering",
            ready_state()
                .with_ready_observation(active_observation)
                .expect("primed state should accept a ready observation")
                .enter_recovering(),
            applying_proposal_id,
        ),
        ("applying-mismatched-id", applying, mismatched_proposal_id),
    ] {
        let baseline = state.clone();

        assert!(
            state.take_pending_proposal(proposal_id).is_none(),
            "{label} should reject proposal clearing at the boundary",
        );
        pretty_assert_eq!(state, baseline);
    }
}

#[test]
fn restore_retained_observation_rejects_non_retained_phase_payloads() {
    let retained_observation = observation_snapshot(cursor(7, 8));
    let collecting = ready_state()
        .enter_observing_request(observation_request(
            17,
            ExternalDemandKind::ExternalCursor,
            32,
        ))
        .expect("primed state should stage a collecting observation request");
    let observing = collecting
        .with_active_observation(retained_observation.clone())
        .expect("collecting state should activate an observation");
    let ready = ready_state()
        .with_ready_observation(retained_observation.clone())
        .expect("primed state should accept a ready observation");
    let (planning_ready, planning_proposal_id) = ready.clone().allocate_proposal_id();
    let planning = planning_ready
        .enter_planning(planning_proposal_id)
        .expect("ready state should enter planning");
    let (applying, _) =
        applying_state_with_realization_plan(ready.clone(), noop_realization_plan(), false, None);

    for (label, mut state) in [
        ("idle", CoreState::default()),
        ("primed", ready_state()),
        ("observing", observing),
        ("ready", ready.clone()),
        ("planning", planning),
        ("applying", applying),
    ] {
        let baseline = state.clone();

        assert!(
            !state.restore_retained_observation(Some(retained_observation.clone())),
            "{label} should reject retained observation restoration outside retained slots",
        );
        pretty_assert_eq!(state, baseline);
    }

    let mut recovering = ready.enter_recovering();
    assert!(recovering.restore_retained_observation(Some(retained_observation.clone())));
    pretty_assert_eq!(
        recovering.retained_observation(),
        Some(&retained_observation),
    );
}

#[test]
fn restore_retained_observation_to_ready_rejects_phases_without_a_retained_ready_slot() {
    let retained_observation = observation_snapshot(cursor(7, 8));
    let collecting = ready_state()
        .enter_observing_request(observation_request(
            17,
            ExternalDemandKind::ExternalCursor,
            32,
        ))
        .expect("primed state should stage a collecting observation request");
    let observing = collecting
        .clone()
        .with_active_observation(retained_observation.clone())
        .expect("collecting state should activate an observation");
    let ready = ready_state()
        .with_ready_observation(retained_observation)
        .expect("primed state should accept a ready observation");
    let (planning_ready, planning_proposal_id) = ready.clone().allocate_proposal_id();
    let planning = planning_ready
        .enter_planning(planning_proposal_id)
        .expect("ready state should enter planning");
    let (applying, _) =
        applying_state_with_realization_plan(ready.clone(), noop_realization_plan(), false, None);
    let recovering_empty = ready_state().enter_recovering();

    for (label, mut state) in [
        ("idle", CoreState::default()),
        ("primed", ready_state()),
        ("collecting", collecting),
        ("observing", observing),
        ("ready", ready),
        ("planning", planning),
        ("applying", applying),
        ("recovering-empty", recovering_empty),
    ] {
        let baseline = state.clone();

        assert!(
            !state.restore_retained_observation_to_ready(),
            "{label} should reject restoring ready from a non-retained slot",
        );
        pretty_assert_eq!(state, baseline);
    }
}
