use super::scheduled_effect_drain_support::ExecutorPlan;
use super::scheduled_effect_drain_support::ScheduledDrainHarness;
use super::scheduled_effect_drain_support::ScheduledDrainModel;
use super::scheduled_effect_drain_support::ScheduledDrainOperation;
use super::scheduled_effect_drain_support::cleanup_effect;
use super::scheduled_effect_drain_support::cleanup_timer_effect;
use super::scheduled_effect_drain_support::scheduled_drain_effects;
use super::scheduled_effect_drain_support::scheduled_drain_operation;
use super::scheduled_effect_drain_support::scheduled_drain_thermal;
use super::*;
use crate::core::effect::OrderedEffect;
use crate::core::state::RenderThermalState;
use crate::test_support::proptest::stateful_config;
use pretty_assertions::assert_eq;
use proptest::collection::vec;
use proptest::prelude::*;

struct DrainEdgeExpectation<'a> {
    label: &'a str,
    executed_effects: &'a [Effect],
    queued_work_count: usize,
    queue_is_marked_scheduled: bool,
    has_more_items: bool,
}

fn assert_drain_edge(
    harness: &ScheduledDrainHarness,
    executor: &mut RecordingExecutor,
    expectation: DrainEdgeExpectation<'_>,
) {
    let has_more_items = harness.drain_next_edge(executor);

    assert_eq!(
        executor.executed_effects.as_slice(),
        expectation.executed_effects,
        "{} should execute the expected effect snapshot",
        expectation.label
    );
    assert_eq!(
        harness.queued_work_count(),
        expectation.queued_work_count,
        "{} should leave the expected queued work count",
        expectation.label
    );
    assert_eq!(
        harness.queue_is_marked_scheduled(),
        expectation.queue_is_marked_scheduled,
        "{} should leave the scheduled flag in the expected state",
        expectation.label
    );
    assert_eq!(
        has_more_items, expectation.has_more_items,
        "{} should report whether deferred work remains",
        expectation.label
    );
}

#[test]
fn first_edge_leaves_new_follow_up_work_for_a_later_edge() {
    let harness = ScheduledDrainHarness::new();
    harness.stage_two_effect_batches();
    replace_core_state(ready_state());
    let mut executor = ExecutorPlan::new()
        .follow_up(vec![external_cursor_demand(21)])
        .no_follow_up()
        .build();

    assert_drain_edge(
        &harness,
        &mut executor,
        DrainEdgeExpectation {
            label: "mid-pass follow-up work",
            executed_effects: &[Effect::RedrawCmdline, cleanup_timer_effect(1)],
            queued_work_count: 1,
            queue_is_marked_scheduled: true,
            has_more_items: true,
        },
    );
    assert!(
        matches!(
            harness.queued_front_work_item(),
            Some(ScheduledWorkUnit::OrderedEffectBatch(ref effects))
                if contains_observation_base_request(effects)
        ),
        "newly staged follow-up work should remain queued for the next scheduled edge"
    );
    assert!(
        harness.queue_is_marked_scheduled(),
        "queue must stay armed when new follow-up work remains"
    );
}

#[test]
fn hot_edge_defers_cleanup_follow_up_waves_because_they_feed_the_reducer() {
    let harness = ScheduledDrainHarness::with_cleanup_thermal(RenderThermalState::Hot);
    assert!(harness.stage_batch(vec![cleanup_effect(12)]));
    let mut executor = ExecutorPlan::new()
        .follow_up(vec![CoreEvent::RenderCleanupApplied(
            crate::core::event::RenderCleanupAppliedEvent {
                observed_at: Millis::new(65),
                action: crate::core::event::RenderCleanupAppliedAction::SoftCleared {
                    retained_resources: 0,
                },
            },
        )])
        .build();

    let has_more_items = harness.drain_next_edge(&mut executor);

    assert!(
        has_more_items,
        "hot drain should defer cleanup follow-up work that can feed the reducer"
    );
    assert_eq!(
        executor.executed_effects.len(),
        1,
        "only the original cleanup effect should run in this scheduled callback"
    );
    assert!(
        matches!(
            executor.executed_effects.first(),
            Some(Effect::ApplyRenderCleanup(_))
        ),
        "the original cleanup effect should execute"
    );
    assert_eq!(
        harness.queued_work_count(),
        1,
        "follow-up cleanup should remain queued as ordered work"
    );
    assert!(
        matches!(
            harness.queued_front_work_item(),
            Some(ScheduledWorkUnit::OrderedEffectBatch(ref effects))
                if effects
                    .iter()
                    .any(|effect| matches!(effect, OrderedEffect::ApplyRenderCleanup(_)))
        ),
        "follow-up cleanup should stay in an ordered effect batch"
    );
}

#[test]
fn scheduled_drain_budget_shapes_match_expected_tables() {
    #[derive(Clone, Copy)]
    enum BudgetKind {
        Thermal(RenderThermalState),
        HotEffectOnly,
    }

    let cases = [
        (
            "hot thermal budget returns zero for empty queue",
            BudgetKind::Thermal(RenderThermalState::Hot),
            0,
            0,
        ),
        (
            "hot thermal budget clears small queue",
            BudgetKind::Thermal(RenderThermalState::Hot),
            3,
            3,
        ),
        (
            "hot thermal budget drains queue at the base cap",
            BudgetKind::Thermal(RenderThermalState::Hot),
            16,
            16,
        ),
        (
            "hot thermal budget starts fractional draining above the cap",
            BudgetKind::Thermal(RenderThermalState::Hot),
            24,
            16,
        ),
        (
            "hot thermal budget drains half of a medium backlog",
            BudgetKind::Thermal(RenderThermalState::Hot),
            40,
            20,
        ),
        (
            "hot thermal budget clamps to the chain cap for large backlogs",
            BudgetKind::Thermal(RenderThermalState::Hot),
            117,
            32,
        ),
        (
            "hot effect-only budget returns zero for empty queue",
            BudgetKind::HotEffectOnly,
            0,
            0,
        ),
        (
            "hot effect-only budget clears tiny queues",
            BudgetKind::HotEffectOnly,
            3,
            3,
        ),
        (
            "hot effect-only budget drains one full snapshot below the cap",
            BudgetKind::HotEffectOnly,
            40,
            40,
        ),
        (
            "hot effect-only budget clamps to the chain cap",
            BudgetKind::HotEffectOnly,
            117,
            96,
        ),
        (
            "cooling thermal budget returns zero for empty queue",
            BudgetKind::Thermal(RenderThermalState::Cooling),
            0,
            0,
        ),
        (
            "cooling thermal budget clears small queues",
            BudgetKind::Thermal(RenderThermalState::Cooling),
            3,
            3,
        ),
        (
            "cooling thermal budget drains the full queued snapshot",
            BudgetKind::Thermal(RenderThermalState::Cooling),
            40,
            40,
        ),
    ];

    for (label, budget_kind, queued_items, expected_budget) in cases {
        let actual_budget = match budget_kind {
            BudgetKind::Thermal(thermal) => {
                scheduled_drain_budget_for_thermal(thermal, queued_items)
            }
            BudgetKind::HotEffectOnly => {
                scheduled_drain_budget_for_hot_effect_only_snapshot(queued_items)
            }
        };

        assert_eq!(actual_budget, expected_budget, "{label}");
    }
}

#[test]
fn cooling_snapshot_drain_still_defers_mid_pass_follow_up_work() {
    let harness = ScheduledDrainHarness::with_cleanup_thermal(RenderThermalState::Cooling);
    harness.stage_redraw_waves(1);
    let mut executor = ExecutorPlan::new()
        .follow_up(vec![external_cursor_demand(21)])
        .build();

    assert_drain_edge(
        &harness,
        &mut executor,
        DrainEdgeExpectation {
            label: "cooling snapshot with mid-pass follow-up work",
            executed_effects: &[Effect::RedrawCmdline],
            queued_work_count: 1,
            queue_is_marked_scheduled: true,
            has_more_items: true,
        },
    );
    assert!(
        matches!(
            harness.queued_front_work_item(),
            Some(ScheduledWorkUnit::OrderedEffectBatch(ref effects))
                if contains_observation_base_request(effects)
        ),
        "mid-pass follow-up work should stay queued after the cooling snapshot drains"
    );
}

proptest! {
    #![proptest_config(stateful_config())]

    #[test]
    fn prop_scheduled_drain_matches_reference_model_across_stage_and_drain_sequences(
        thermal in scheduled_drain_thermal(),
        operations in vec(scheduled_drain_operation(), 1..=64),
    ) {
        let harness = ScheduledDrainHarness::with_cleanup_thermal(thermal);
        let mut model = ScheduledDrainModel::new(thermal);

        for operation in operations {
            match operation {
                ScheduledDrainOperation::Stage(specs) => {
                    let effects = scheduled_drain_effects(&specs);
                    prop_assert_eq!(
                        harness.stage_batch(effects.clone()),
                        model.stage_batch(effects),
                    );
                }
                ScheduledDrainOperation::Drain => {
                    let mut executor = RecordingExecutor::default();
                    let actual_has_more_items = harness.drain_next_edge(&mut executor);
                    let expected = model.drain_next_edge();

                    prop_assert_eq!(executor.executed_effects, expected.executed_effects);
                    prop_assert_eq!(actual_has_more_items, expected.has_more_items);
                }
            }

            prop_assert_eq!(harness.queued_work_count(), model.queued_work_count());
            prop_assert_eq!(
                harness.queue_is_marked_scheduled(),
                model.queue_is_marked_scheduled()
            );
        }

        while model.queued_work_count() > 0 {
            let mut executor = RecordingExecutor::default();
            let actual_has_more_items = harness.drain_next_edge(&mut executor);
            let expected = model.drain_next_edge();

            prop_assert_eq!(executor.executed_effects, expected.executed_effects);
            prop_assert_eq!(actual_has_more_items, expected.has_more_items);
            prop_assert_eq!(harness.queued_work_count(), model.queued_work_count());
            prop_assert_eq!(
                harness.queue_is_marked_scheduled(),
                model.queue_is_marked_scheduled()
            );
        }

        prop_assert_eq!(harness.queued_work_count(), 0);
        prop_assert!(!harness.queue_is_marked_scheduled());
    }
}
