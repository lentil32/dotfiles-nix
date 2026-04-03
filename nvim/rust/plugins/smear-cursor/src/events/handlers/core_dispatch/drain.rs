use super::super::super::logging::warn;
use super::super::super::runtime::EffectExecutor;
use super::super::super::runtime::NeovimEffectExecutor;
use super::super::super::runtime::now_ms;
use super::super::super::runtime::record_scheduled_drain_items;
use super::super::super::runtime::record_scheduled_drain_items_for_thermal;
use super::super::super::runtime::record_scheduled_drain_reschedule;
use super::super::super::runtime::record_scheduled_drain_reschedule_for_thermal;
use super::super::super::runtime::to_core_millis;
use super::super::labels::core_event_label;
use super::current_cleanup_thermal_state;
use super::queue::MAX_CHAINED_SCHEDULED_WORK_ITEMS_PER_EDGE;
use super::queue::MAX_SCHEDULED_WORK_ITEMS_PER_EDGE;
use super::queue::MIN_SCHEDULED_WORK_ITEMS_PER_EDGE;
use super::queue::ScheduledEffectDrainEntry;
use super::queue::ScheduledEffectQueueState;
use super::queue::ScheduledWorkItem;
use super::queue::ScheduledWorkUnit;
use super::queue::with_scheduled_effect_queue;
use super::schedule_scheduled_effect_drain;
use super::stage_core_event_with_default_scheduler;
use crate::core::event::EffectFailedEvent;
use crate::core::event::Event as CoreEvent;
use crate::core::state::RenderThermalState;

use super::execute::ScheduledWorkExecutionError;
use super::execute::dispatch_scheduled_core_event;
use super::execute::execute_effect_only_step;
use super::execute::execute_scheduled_effect_batch;

pub(super) fn reset_scheduled_queue_after_failure() {
    super::reset_scheduled_effect_queue();
}

fn handle_scheduled_work_drain_failure(work_name: &'static str, error: &nvim_oxi::Error) {
    reset_scheduled_queue_after_failure();
    warn(&format!("scheduled core work failed: {work_name}: {error}"));
    let observed_at = to_core_millis(now_ms());
    stage_core_event_with_default_scheduler(CoreEvent::EffectFailed(EffectFailedEvent {
        proposal_id: None,
        observed_at,
    }));
}

pub(super) fn scheduled_drain_budget() -> usize {
    let thermal = current_cleanup_thermal_state().unwrap_or(RenderThermalState::Cold);
    with_scheduled_effect_queue(|queue| {
        let queued_work_units = queue.pending_work_units;
        if queued_work_units == 0 {
            queue.drain_scheduled = false;
            return 0;
        }

        if thermal == RenderThermalState::Hot && queue_has_only_effect_only_agendas(queue) {
            return scheduled_drain_budget_for_hot_effect_only_snapshot(queued_work_units);
        }

        scheduled_drain_budget_for_thermal(thermal, queued_work_units)
    })
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct ScheduledDrainSnapshot {
    thermal: RenderThermalState,
    budget: usize,
}

fn scheduled_drain_snapshot() -> ScheduledDrainSnapshot {
    let thermal = current_cleanup_thermal_state().unwrap_or(RenderThermalState::Cold);
    ScheduledDrainSnapshot {
        thermal,
        budget: scheduled_drain_budget(),
    }
}

fn queue_has_only_effect_only_agendas(queue: &ScheduledEffectQueueState) -> bool {
    !queue.items.is_empty()
        && queue
            .items
            .iter()
            .all(|item| matches!(item, ScheduledWorkItem::EffectOnlyAgenda(_)))
}

pub(super) fn scheduled_drain_budget_for_thermal(
    thermal: RenderThermalState,
    queued_work_units: usize,
) -> usize {
    match thermal {
        RenderThermalState::Cooling => {
            // Surprising: Cooling still drains only the snapshot that was already queued when this
            // edge started. Refresh-required probe retries must remain deferred to the next edge,
            // so aggressive convergence expands the snapshot budget instead of recursively draining
            // follow-up work staged mid-pass.
            queued_work_units
        }
        RenderThermalState::Hot | RenderThermalState::Cold => {
            scheduled_drain_budget_for_depth(queued_work_units)
        }
    }
}

pub(super) fn scheduled_drain_budget_for_hot_effect_only_snapshot(
    queued_work_units: usize,
) -> usize {
    if queued_work_units == 0 {
        return 0;
    }

    // CONTEXT: once Hot queue state has collapsed down to effect-only cleanup/timer/metric/redraw
    // steps, draining only half the backlog turns one reducer burst into repeated shell edges even
    // though the queue no longer carries irreducible reducer work. Drain that typed snapshot in
    // one callback, but keep the existing chained-edge ceiling so a pathological wave still yields
    // back to the shell.
    queued_work_units.min(MAX_CHAINED_SCHEDULED_WORK_ITEMS_PER_EDGE)
}

pub(super) fn scheduled_drain_budget_for_depth(queued_work_units: usize) -> usize {
    if queued_work_units == 0 {
        return 0;
    }

    // Drain a bounded fraction of the queued snapshot so backlog converges geometrically under
    // burst load, while smaller queues still clear in a single shell edge.
    let half_backlog = queued_work_units.saturating_add(1) / 2;
    let bounded_half = half_backlog.clamp(
        MIN_SCHEDULED_WORK_ITEMS_PER_EDGE,
        MAX_SCHEDULED_WORK_ITEMS_PER_EDGE,
    );
    queued_work_units.min(bounded_half)
}

fn should_continue_effect_only_follow_up_chain(
    snapshot: ScheduledDrainSnapshot,
    drained_items_this_pass: usize,
    drained_items_total: usize,
) -> bool {
    if snapshot.thermal != RenderThermalState::Hot
        || drained_items_this_pass != snapshot.budget
        || drained_items_total >= MAX_CHAINED_SCHEDULED_WORK_ITEMS_PER_EDGE
    {
        return false;
    }

    with_scheduled_effect_queue(|queue| {
        // Mixed hot snapshots often drain their irreducible reducer work first and leave only
        // cleanup/timer/metric/redraw tail work queued. Once that remaining queue has collapsed
        // to effect-only agendas, continue immediately in the same callback instead of paying an
        // extra scheduled shell edge for the tail.
        !queue.items.is_empty()
            && queue
                .items
                .iter()
                .all(|item| matches!(item, ScheduledWorkItem::EffectOnlyAgenda(_)))
    })
}

pub(super) fn drain_scheduled_work_with_executor(
    executor: &mut impl EffectExecutor,
) -> std::result::Result<bool, ScheduledWorkExecutionError> {
    // Drain a bounded snapshot of already-staged work so backlog can recover faster, while any
    // reducer follow-ups created during this pass still remain deferred to a later scheduled edge.
    let drain_thermal = current_cleanup_thermal_state();
    let mut drained_items = 0_usize;
    loop {
        let snapshot = scheduled_drain_snapshot();
        let mut remaining_budget = snapshot.budget;
        let mut drained_items_this_pass = 0_usize;
        while remaining_budget > 0 {
            let Some(item) = with_scheduled_effect_queue(ScheduledEffectQueueState::pop_work_unit)
            else {
                with_scheduled_effect_queue(|queue| {
                    queue.drain_scheduled = false;
                });
                if drained_items > 0 {
                    record_scheduled_drain_items(drained_items);
                    if let Some(thermal) = drain_thermal {
                        record_scheduled_drain_items_for_thermal(thermal, drained_items);
                    }
                }
                return Ok(false);
            };

            match item {
                ScheduledWorkUnit::EffectBatch(effects) => {
                    execute_scheduled_effect_batch(effects, executor)?;
                }
                ScheduledWorkUnit::CoreEvent(event) => {
                    let work_name = core_event_label(&event);
                    dispatch_scheduled_core_event(*event)
                        .map_err(|error| ScheduledWorkExecutionError { work_name, error })?;
                }
                ScheduledWorkUnit::EffectOnlyStep(step) => {
                    execute_effect_only_step(step, executor)?;
                }
            }
            drained_items = drained_items.saturating_add(1);
            drained_items_this_pass = drained_items_this_pass.saturating_add(1);
            remaining_budget -= 1;
        }

        // Once Hot drains the full pre-existing snapshot, any remaining cleanup/timer batches were
        // spawned mid-pass by reducer follow-ups inside this callback. Continue through a bounded
        // number of those low-risk waves here so immediate cleanup convergence does not pay an
        // extra schedule edge per reducer hop. Observation batches and queued probe batches still
        // remain deferred; only the single cursor-color probe attached directly to an
        // ObservationBaseCollected follow-up can execute inline before that queue hop exists.
        if should_continue_effect_only_follow_up_chain(
            snapshot,
            drained_items_this_pass,
            drained_items,
        ) {
            continue;
        }

        break;
    }

    let has_more_items = with_scheduled_effect_queue(|queue| {
        let has_more_items = !queue.items.is_empty();
        if !has_more_items {
            queue.drain_scheduled = false;
        }
        has_more_items
    });

    if drained_items > 0 {
        record_scheduled_drain_items(drained_items);
        if let Some(thermal) = drain_thermal {
            record_scheduled_drain_items_for_thermal(thermal, drained_items);
        }
    }
    if has_more_items {
        record_scheduled_drain_reschedule();
        if let Some(thermal) = drain_thermal {
            record_scheduled_drain_reschedule_for_thermal(thermal);
        }
    }

    Ok(has_more_items)
}

pub(super) fn run_scheduled_effect_drain(entrypoint: ScheduledEffectDrainEntry) {
    let mut executor = match NeovimEffectExecutor::new() {
        Ok(executor) => executor,
        Err(err) => {
            handle_scheduled_work_drain_failure(entrypoint.context(), &err);
            return;
        }
    };
    let drain_outcome = match entrypoint {
        ScheduledEffectDrainEntry::NextItem => drain_scheduled_work_with_executor(&mut executor),
    };

    match drain_outcome {
        Ok(true) => schedule_scheduled_effect_drain(entrypoint),
        Ok(false) => {}
        Err(err) => {
            handle_scheduled_work_drain_failure(err.work_name, &err.error);
        }
    }
}
