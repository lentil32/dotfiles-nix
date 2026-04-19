use super::*;
use crate::core::state::ObservationSlotKind;
use crate::core::state::ProtocolWorkflowKind;
use crate::core::state::workflow_allows_observation_slot_for_tests;
use crate::test_support::cursor;
use crate::test_support::proptest::pure_config;
use proptest::prelude::*;

fn workflow_kind_strategy() -> impl Strategy<Value = ProtocolWorkflowKind> {
    prop_oneof![
        Just(ProtocolWorkflowKind::Idle),
        Just(ProtocolWorkflowKind::Primed),
        Just(ProtocolWorkflowKind::ObservingRequest),
        Just(ProtocolWorkflowKind::ObservingActive),
        Just(ProtocolWorkflowKind::Ready),
        Just(ProtocolWorkflowKind::Planning),
        Just(ProtocolWorkflowKind::Applying),
        Just(ProtocolWorkflowKind::Recovering),
    ]
}

fn observation_slot_kind_strategy() -> impl Strategy<Value = ObservationSlotKind> {
    prop_oneof![
        Just(ObservationSlotKind::Empty),
        Just(ObservationSlotKind::Retained),
        Just(ObservationSlotKind::Active),
    ]
}

proptest! {
    #![proptest_config(pure_config())]

    #[test]
    fn prop_workflow_slot_matrix_matches_the_protocol_contract(
        workflow in workflow_kind_strategy(),
        observation in observation_slot_kind_strategy(),
    ) {
        let expected = match workflow {
            ProtocolWorkflowKind::Idle | ProtocolWorkflowKind::Primed => {
                observation == ObservationSlotKind::Empty
            }
            ProtocolWorkflowKind::ObservingRequest | ProtocolWorkflowKind::Recovering => matches!(
                observation,
                ObservationSlotKind::Empty | ObservationSlotKind::Retained
            ),
            ProtocolWorkflowKind::ObservingActive
            | ProtocolWorkflowKind::Ready
            | ProtocolWorkflowKind::Planning
            | ProtocolWorkflowKind::Applying => observation == ObservationSlotKind::Active,
        };

        prop_assert_eq!(
            workflow_allows_observation_slot_for_tests(workflow, observation),
            expected
        );
    }
}

#[test]
fn enter_observing_request_demotes_active_observation_into_the_retained_slot() {
    let observation = observation_snapshot(cursor(7, 8));
    let request = observation_request(3, ExternalDemandKind::ExternalCursor, 20);
    let observing = ready_state()
        .enter_ready(observation.clone())
        .enter_observing_request(request);

    pretty_assert_eq!(observing.lifecycle(), Lifecycle::Observing);
    pretty_assert_eq!(
        observing.protocol().workflow_kind(),
        ProtocolWorkflowKind::ObservingRequest
    );
    pretty_assert_eq!(
        observing.protocol().observation_slot_kind(),
        ObservationSlotKind::Retained
    );
    pretty_assert_eq!(observing.observation(), None);
    pretty_assert_eq!(observing.retained_observation(), Some(&observation));
}

#[test]
fn enter_planning_keeps_the_active_observation_slot_populated() {
    let observation = observation_snapshot(cursor(11, 12));
    let ready = ready_state().enter_ready(observation.clone());
    let (ready, proposal_id) = ready.allocate_proposal_id();
    let planning = ready
        .enter_planning(proposal_id)
        .expect("ready state should enter planning");

    pretty_assert_eq!(planning.lifecycle(), Lifecycle::Planning);
    pretty_assert_eq!(
        planning.protocol().workflow_kind(),
        ProtocolWorkflowKind::Planning
    );
    pretty_assert_eq!(
        planning.protocol().observation_slot_kind(),
        ObservationSlotKind::Active
    );
    pretty_assert_eq!(planning.observation(), Some(&observation));
}

#[test]
fn applying_and_ready_transitions_preserve_the_active_observation() {
    let observation = observation_snapshot(cursor(9, 10));
    let ready = ready_state().enter_ready(observation.clone());
    let (applying, proposal_id) =
        applying_state_with_realization_plan(ready, noop_realization_plan(), false, None);

    pretty_assert_eq!(applying.lifecycle(), Lifecycle::Applying);
    pretty_assert_eq!(
        applying.protocol().workflow_kind(),
        ProtocolWorkflowKind::Applying
    );
    pretty_assert_eq!(
        applying.protocol().observation_slot_kind(),
        ObservationSlotKind::Active
    );
    pretty_assert_eq!(applying.observation(), Some(&observation));

    let (restored_ready, _) = applying
        .clear_pending_for(proposal_id)
        .expect("clearing the pending proposal should return to ready");

    pretty_assert_eq!(restored_ready.lifecycle(), Lifecycle::Ready);
    pretty_assert_eq!(
        restored_ready.protocol().workflow_kind(),
        ProtocolWorkflowKind::Ready
    );
    pretty_assert_eq!(
        restored_ready.protocol().observation_slot_kind(),
        ObservationSlotKind::Active
    );
    pretty_assert_eq!(restored_ready.observation(), Some(&observation));
}
