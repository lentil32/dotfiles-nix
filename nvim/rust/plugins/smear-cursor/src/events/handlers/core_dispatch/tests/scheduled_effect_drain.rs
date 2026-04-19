use super::scheduled_effect_drain_support::ExecutorPlan;
use super::scheduled_effect_drain_support::FailingExecutor;
use super::scheduled_effect_drain_support::ScheduledDrainHarness;
use super::scheduled_effect_drain_support::ScheduledDrainModel;
use super::scheduled_effect_drain_support::ScheduledDrainOperation;
use super::scheduled_effect_drain_support::cleanup_effect;
use super::scheduled_effect_drain_support::cleanup_timer_effect;
use super::scheduled_effect_drain_support::non_coalescible_effect;
use super::scheduled_effect_drain_support::scheduled_drain_effects;
use super::scheduled_effect_drain_support::scheduled_drain_operation;
use super::scheduled_effect_drain_support::scheduled_drain_thermal;
use super::*;
use crate::core::effect::EventLoopMetricEffect;
use crate::core::state::RenderThermalState;
use crate::core::types::TimerId;
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

fn repeated_effects(effect: Effect, count: usize) -> Vec<Effect> {
    vec![effect; count]
}

#[test]
fn first_edge_executes_a_bounded_snapshot_and_clears_queue_state_when_snapshot_finishes() {
    let harness = ScheduledDrainHarness::new();
    harness.stage_two_effect_batches();
    let mut executor = RecordingExecutor::default();

    assert_drain_edge(
        &harness,
        &mut executor,
        DrainEdgeExpectation {
            label: "bounded snapshot that fits in budget",
            executed_effects: &[Effect::RedrawCmdline, cleanup_timer_effect(1)],
            queued_work_count: 0,
            queue_is_marked_scheduled: false,
            has_more_items: false,
        },
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
            Some(ScheduledWorkUnit::EffectBatch(ref effects))
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
fn staging_effect_only_work_keeps_payloads_out_of_core_state() {
    let _context = CoreDispatchTestContext::new();
    let initial = ready_state();
    replace_core_state(initial.clone());

    assert!(queue_stage_batch(vec![
        Effect::RedrawCmdline,
        cleanup_timer_effect(1)
    ]));

    assert_eq!(current_core_state(), initial);
    assert_eq!(queued_work_count(), 2);
    assert!(matches!(
        queued_front_work_item(),
        Some(ScheduledWorkUnit::EffectOnlyStep(_))
    ));
}

#[test]
fn hot_edge_continues_through_cleanup_follow_up_waves() {
    let harness = ScheduledDrainHarness::with_cleanup_thermal(RenderThermalState::Hot);
    assert!(harness.stage_batch(vec![cleanup_effect(12)]));
    let mut executor = ExecutorPlan::new()
        .follow_up(vec![CoreEvent::RenderCleanupApplied(
            crate::core::event::RenderCleanupAppliedEvent {
                observed_at: Millis::new(65),
                action: crate::core::event::RenderCleanupAppliedAction::SoftCleared,
            },
        )])
        .build();

    let has_more_items = harness.drain_next_edge(&mut executor);

    assert!(
        !has_more_items,
        "hot drain should finish cleanup-only follow-up waves without rearming"
    );
    assert_eq!(
        executor.executed_effects.len(),
        2,
        "follow-up cleanup work should run in the same scheduled callback"
    );
    assert!(
        matches!(
            executor.executed_effects.get(1),
            Some(Effect::ApplyRenderCleanup(_))
        ),
        "follow-up compaction should stay ordered after the original cleanup effect"
    );
    harness.assert_disarmed();
}

#[test]
fn drain_failure_resets_the_scheduled_queue_state() {
    let harness = ScheduledDrainHarness::new();
    harness.stage_two_effect_batches();
    let mut executor = FailingExecutor;

    let err = drain_scheduled_work_with_executor(&mut executor)
        .expect_err("planned executor failure should surface from the drain");
    let _ = err;
    reset_scheduled_queue_after_failure();

    harness.assert_disarmed();
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
fn hot_effect_only_snapshot_still_defers_mid_pass_follow_up_work() {
    let harness = ScheduledDrainHarness::with_cleanup_thermal(RenderThermalState::Hot);
    harness.stage_cleanup_backlog([12_usize, 13]);
    let mut executor = ExecutorPlan::new()
        .follow_up(vec![external_cursor_demand(21)])
        .no_follow_up()
        .build();

    assert_drain_edge(
        &harness,
        &mut executor,
        DrainEdgeExpectation {
            label: "hot effect-only snapshot with mid-pass follow-up work",
            executed_effects: &[cleanup_effect(12), cleanup_effect(13)],
            queued_work_count: 1,
            queue_is_marked_scheduled: true,
            has_more_items: true,
        },
    );
    assert!(
        matches!(
            harness.queued_front_work_item(),
            Some(ScheduledWorkUnit::EffectBatch(ref effects))
                if contains_observation_base_request(effects)
        ),
        "mid-pass reducer follow-up work should remain queued for the next edge"
    );
}

#[test]
fn hot_mixed_snapshot_continues_when_remaining_tail_is_effect_only() {
    let harness = ScheduledDrainHarness::with_cleanup_thermal(RenderThermalState::Hot);
    harness.stage_non_coalescible_backlog(16);
    harness.stage_cleanup_backlog([12_usize, 13]);
    let mut executor = RecordingExecutor::default();
    let mut expected_effects = repeated_effects(non_coalescible_effect(), 16);
    expected_effects.extend([cleanup_effect(12), cleanup_effect(13)]);

    assert_drain_edge(
        &harness,
        &mut executor,
        DrainEdgeExpectation {
            label: "hot mixed snapshot that collapses to an effect-only tail",
            executed_effects: &expected_effects,
            queued_work_count: 0,
            queue_is_marked_scheduled: false,
            has_more_items: false,
        },
    );
    assert!(
        executor.executed_effects[..16]
            .iter()
            .all(|effect| *effect == non_coalescible_effect()),
        "the bounded mixed prefix should preserve FIFO order before the effect-only tail"
    );
}

#[test]
fn hot_first_edge_reschedules_when_backlog_exceeds_the_bounded_fractional_budget() {
    let harness = ScheduledDrainHarness::with_cleanup_thermal(RenderThermalState::Hot);
    harness.stage_non_coalescible_backlog(40);
    let mut executor = RecordingExecutor::default();
    let expected_effects = repeated_effects(non_coalescible_effect(), 20);

    assert_drain_edge(
        &harness,
        &mut executor,
        DrainEdgeExpectation {
            label: "hot bounded backlog",
            executed_effects: &expected_effects,
            queued_work_count: 20,
            queue_is_marked_scheduled: true,
            has_more_items: true,
        },
    );
}

#[test]
fn adjacent_redraw_batches_coalesce_into_one_work_unit() {
    let harness = ScheduledDrainHarness::new();
    harness.stage_redraw_waves(40);

    assert_eq!(
        harness.queued_work_count(),
        1,
        "redraw waves should coalesce"
    );

    let mut executor = RecordingExecutor::default();
    assert_drain_edge(
        &harness,
        &mut executor,
        DrainEdgeExpectation {
            label: "coalesced redraw batch",
            executed_effects: &[Effect::RedrawCmdline],
            queued_work_count: 0,
            queue_is_marked_scheduled: false,
            has_more_items: false,
        },
    );
}

#[test]
fn adjacent_metric_batches_share_one_work_unit() {
    let harness = ScheduledDrainHarness::new();
    harness.stage_metric_batches([
        EventLoopMetricEffect::StaleToken,
        EventLoopMetricEffect::DelayedIngressPendingUpdated,
    ]);

    assert_eq!(
        harness.queued_work_count(),
        1,
        "adjacent metric work should aggregate"
    );
}

#[test]
fn adjacent_timer_rearms_for_the_same_kind_keep_only_the_newest_effect() {
    let harness = ScheduledDrainHarness::new();
    harness.stage_timer_rearms(TimerId::Cleanup, [1_u64, 2]);

    assert_eq!(
        harness.queued_work_count(),
        1,
        "same-kind timer rearm should replace in place"
    );

    let mut executor = RecordingExecutor::default();
    assert_drain_edge(
        &harness,
        &mut executor,
        DrainEdgeExpectation {
            label: "same-kind timer rearm",
            executed_effects: &[cleanup_timer_effect(2)],
            queued_work_count: 0,
            queue_is_marked_scheduled: false,
            has_more_items: false,
        },
    );
}

#[test]
fn hot_bounded_drain_matches_representative_backlog_boundaries() {
    let cases = [1_usize, 16, 17, 40, 64, 96];

    for queued_items in cases {
        let harness = ScheduledDrainHarness::with_cleanup_thermal(RenderThermalState::Hot);
        harness.stage_non_coalescible_backlog(queued_items);
        let mut executor = RecordingExecutor::default();
        let expected_drained = scheduled_drain_budget_for_depth(queued_items);
        let expected_remaining = queued_items.saturating_sub(expected_drained);

        assert_drain_edge(
            &harness,
            &mut executor,
            DrainEdgeExpectation {
                label: "representative hot bounded backlog",
                executed_effects: &vec![non_coalescible_effect(); expected_drained],
                queued_work_count: expected_remaining,
                queue_is_marked_scheduled: expected_remaining > 0,
                has_more_items: expected_remaining > 0,
            },
        );
    }
}

#[test]
fn cooling_convergence_leaves_no_queue_tail_after_idle_backlog_drains() {
    let harness = ScheduledDrainHarness::with_cleanup_thermal(RenderThermalState::Cooling);
    harness.stage_non_coalescible_backlog(40);
    let mut executor = RecordingExecutor::default();
    let expected_effects = repeated_effects(non_coalescible_effect(), 40);

    assert_drain_edge(
        &harness,
        &mut executor,
        DrainEdgeExpectation {
            label: "cooling backlog convergence",
            executed_effects: &expected_effects,
            queued_work_count: 0,
            queue_is_marked_scheduled: false,
            has_more_items: false,
        },
    );
    assert_eq!(
        scheduled_drain_budget(),
        0,
        "idle convergence should leave no remaining scheduled-drain budget"
    );
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
            Some(ScheduledWorkUnit::EffectBatch(ref effects))
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
