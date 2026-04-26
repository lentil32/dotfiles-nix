#[cfg(test)]
use super::super::runtime::EffectExecutor;
use super::super::runtime::ScheduledEffectDrainEntry;
use super::super::runtime::ScheduledEffectQueueState;
#[cfg(test)]
use super::super::runtime::ScheduledWorkItem;
#[cfg(test)]
use super::super::runtime::ScheduledWorkUnit;
use super::super::runtime::record_scheduled_queue_depth;
use super::super::runtime::record_scheduled_queue_depth_for_thermal;
use super::super::runtime::with_core_read;
use super::super::runtime::with_core_transition;
use super::super::runtime::with_dispatch_queue;
use super::super::timers::schedule_guarded;
use super::super::trace::core_event_summary;
use super::super::trace::core_state_summary;
use crate::core::effect::Effect;
use crate::core::event::Event as CoreEvent;
use crate::core::reducer::reduce_owned as reduce_core_event_owned;
use crate::core::state::RenderThermalState;
use nvim_oxi::Result;

mod drain;
mod execute;
#[cfg(test)]
mod tests;

#[cfg(test)]
use drain::drain_scheduled_work_with_executor;
#[cfg(test)]
use drain::reset_scheduled_queue_after_failure;
use drain::run_scheduled_effect_drain;
#[cfg(test)]
use drain::scheduled_drain_budget;
#[cfg(test)]
use drain::scheduled_drain_budget_for_depth;
#[cfg(test)]
use drain::scheduled_drain_budget_for_hot_effect_only_snapshot;
#[cfg(test)]
use drain::scheduled_drain_budget_for_thermal;
use execute::dispatch_core_event_with_effect_handler;

fn schedule_scheduled_effect_drain(entrypoint: ScheduledEffectDrainEntry) {
    schedule_guarded(entrypoint.context(), move || {
        run_scheduled_effect_drain(entrypoint);
    });
}

fn current_cleanup_thermal_state() -> Option<RenderThermalState> {
    with_core_read(|state| state.render_cleanup().thermal()).ok()
}

fn stage_effect_batch_on_default_queue(effects: Vec<Effect>) {
    if effects.is_empty() {
        return;
    }

    let stage = with_dispatch_queue(|queue| queue.stage_batch(effects));
    record_scheduled_queue_depth(stage.depth);
    if let Some(thermal) = current_cleanup_thermal_state() {
        record_scheduled_queue_depth_for_thermal(thermal, stage.depth);
    }
    if stage.should_schedule {
        schedule_scheduled_effect_drain(ScheduledEffectDrainEntry::NextItem);
    }
}

fn stage_core_event_on_default_queue(event: CoreEvent) {
    let stage = with_dispatch_queue(|queue| queue.stage_core_event(event));
    record_scheduled_queue_depth(stage.depth);
    if let Some(thermal) = current_cleanup_thermal_state() {
        record_scheduled_queue_depth_for_thermal(thermal, stage.depth);
    }
    if stage.should_schedule {
        schedule_scheduled_effect_drain(ScheduledEffectDrainEntry::NextItem);
    }
}

pub(crate) fn dispatch_core_event(
    initial_event: CoreEvent,
    stage_effect_batch: &mut impl FnMut(Vec<Effect>),
) -> Result<()> {
    let mut handle_effects = |effects| {
        stage_effect_batch(effects);
        Ok(())
    };
    dispatch_core_event_with_effect_handler(initial_event, &mut handle_effects)
}

pub(crate) fn dispatch_core_events(
    initial_events: impl IntoIterator<Item = CoreEvent>,
    stage_effect_batch: &mut impl FnMut(Vec<Effect>),
) -> Result<()> {
    for event in initial_events {
        dispatch_core_event(event, stage_effect_batch)?;
    }
    Ok(())
}

pub(crate) fn dispatch_core_event_with_default_scheduler(initial_event: CoreEvent) -> Result<()> {
    dispatch_core_events_with_default_scheduler([initial_event])
}

pub(crate) fn dispatch_core_events_with_default_scheduler(
    initial_events: impl IntoIterator<Item = CoreEvent>,
) -> Result<()> {
    let mut stage_effect_batch = stage_effect_batch_on_default_queue;
    dispatch_core_events(initial_events, &mut stage_effect_batch)
}

pub(crate) fn stage_core_event_with_default_scheduler(initial_event: CoreEvent) {
    stage_core_event_on_default_queue(initial_event);
}

pub(crate) fn reset_scheduled_effect_queue() {
    with_dispatch_queue(ScheduledEffectQueueState::reset);
}
