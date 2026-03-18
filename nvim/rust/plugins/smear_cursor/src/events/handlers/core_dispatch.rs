use super::super::logging::{trace_lazy, warn};
use super::super::runtime::{
    EffectExecutor, NeovimEffectExecutor, core_state, now_ms, record_scheduled_drain_items,
    record_scheduled_drain_reschedule, record_scheduled_queue_depth, set_core_state,
    to_core_millis,
};
use super::super::timers::schedule_guarded;
use super::super::trace::{core_event_summary, core_state_summary, effect_summary};
use super::labels::{core_event_label, effect_label};
use crate::core::effect::Effect;
use crate::core::event::{EffectFailedEvent, Event as CoreEvent, ProbeReportedEvent};
use crate::core::reducer::reduce as reduce_core_event;
use crate::core::state::ProbeReuse;
use nvim_oxi::Result;
use std::cell::RefCell;
use std::collections::VecDeque;

const MIN_SCHEDULED_WORK_ITEMS_PER_EDGE: usize = 16;
const MAX_SCHEDULED_WORK_ITEMS_PER_EDGE: usize = 32;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum ScheduledEffectDrainEntry {
    NextItem,
}

impl ScheduledEffectDrainEntry {
    const fn context(self) -> &'static str {
        match self {
            Self::NextItem => "core effect drain",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum ScheduledWorkItem {
    EffectBatch(Vec<Effect>),
    CoreEvent(Box<CoreEvent>),
}

#[derive(Default)]
struct ScheduledEffectQueueState {
    items: VecDeque<ScheduledWorkItem>,
    drain_scheduled: bool,
}

impl ScheduledEffectQueueState {
    fn stage_item(&mut self, item: ScheduledWorkItem) -> bool {
        self.items.push_back(item);
        record_scheduled_queue_depth(self.items.len());
        if self.drain_scheduled {
            false
        } else {
            self.drain_scheduled = true;
            true
        }
    }

    fn stage_batch(&mut self, effects: Vec<Effect>) -> bool {
        self.stage_item(ScheduledWorkItem::EffectBatch(effects))
    }

    fn stage_core_event(&mut self, event: CoreEvent) -> bool {
        self.stage_item(ScheduledWorkItem::CoreEvent(Box::new(event)))
    }

    fn pop_item(&mut self) -> Option<ScheduledWorkItem> {
        self.items.pop_front()
    }

    fn reset(&mut self) {
        self.items.clear();
        self.drain_scheduled = false;
    }
}

thread_local! {
    static SCHEDULED_EFFECT_QUEUE: RefCell<ScheduledEffectQueueState> =
        RefCell::new(ScheduledEffectQueueState::default());
}

fn with_scheduled_effect_queue<R>(mutator: impl FnOnce(&mut ScheduledEffectQueueState) -> R) -> R {
    SCHEDULED_EFFECT_QUEUE.with(|queue| {
        // Keep queue borrows scoped to staging/pop bookkeeping only. Reducer execution and effect
        // dispatch always happen after this borrow is released, so re-entering here would signal a
        // structural bug we should fix directly instead of silently dropping queued work.
        let mut queue = queue.borrow_mut();
        mutator(&mut queue)
    })
}

fn schedule_scheduled_effect_drain(entrypoint: ScheduledEffectDrainEntry) {
    schedule_guarded(entrypoint.context(), move || {
        run_scheduled_effect_drain(entrypoint);
    });
}

fn stage_effect_batch_on_default_queue(effects: Vec<Effect>) {
    if effects.is_empty() {
        return;
    }

    let should_schedule = with_scheduled_effect_queue(|queue| queue.stage_batch(effects));
    if should_schedule {
        schedule_scheduled_effect_drain(ScheduledEffectDrainEntry::NextItem);
    }
}

fn stage_core_event_on_default_queue(event: CoreEvent) {
    let should_schedule = with_scheduled_effect_queue(|queue| queue.stage_core_event(event));
    if should_schedule {
        schedule_scheduled_effect_drain(ScheduledEffectDrainEntry::NextItem);
    }
}

pub(crate) fn dispatch_core_event(
    initial_event: CoreEvent,
    stage_effect_batch: &mut impl FnMut(Vec<Effect>),
) -> Result<()> {
    let previous_state = core_state()?;
    let event_label = core_event_label(&initial_event);
    let event_summary = core_event_summary(&initial_event);
    let transition = reduce_core_event(&previous_state, initial_event);
    trace_lazy(|| {
        format!(
            "core_transition event={} details={} from=[{}] to=[{}] effects={}",
            event_label,
            event_summary,
            core_state_summary(&previous_state),
            core_state_summary(&transition.next),
            transition.effects.len()
        )
    });

    let effects = transition.effects;
    set_core_state(transition.next)?;
    if !effects.is_empty() {
        stage_effect_batch(effects);
    }
    Ok(())
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
    with_scheduled_effect_queue(ScheduledEffectQueueState::reset);
}

#[derive(Debug)]
struct ScheduledWorkExecutionError {
    work_name: &'static str,
    error: nvim_oxi::Error,
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

fn reset_scheduled_queue_after_failure() {
    reset_scheduled_effect_queue();
}

fn execute_scheduled_effect_batch(
    effects: Vec<Effect>,
    executor: &mut impl EffectExecutor,
) -> std::result::Result<(), ScheduledWorkExecutionError> {
    let mut follow_ups = VecDeque::new();
    for effect in effects {
        let effect_name = effect_label(&effect);
        let effect_details = effect_summary(&effect);
        trace_lazy(|| format!("effect_dispatch effect={effect_name} details={effect_details}"));
        match executor.execute_effect(effect) {
            Ok(new_follow_ups) => {
                trace_lazy(|| {
                    format!(
                        "effect_outcome effect={effect_name} details={effect_details} result=ok follow_ups={}",
                        new_follow_ups.len()
                    )
                });
                follow_ups.extend(new_follow_ups);
            }
            Err(err) => {
                trace_lazy(|| {
                    format!(
                        "effect_outcome effect={effect_name} details={effect_details} result=err error={err}"
                    )
                });
                return Err(ScheduledWorkExecutionError {
                    work_name: effect_name,
                    error: err,
                });
            }
        }
    }

    if follow_ups.is_empty() {
        return Ok(());
    }

    for follow_up in follow_ups {
        let work_name = core_event_label(&follow_up);
        if should_schedule_follow_up_event(&follow_up) {
            // retry-class probe reports stay typed reducer inputs, but they hop back onto
            // the scheduled queue so one probe edge cannot immediately replay the next observation.
            stage_core_event_on_default_queue(follow_up);
            continue;
        }

        let mut stage_effect_batch = stage_effect_batch_on_default_queue;
        dispatch_core_event(follow_up, &mut stage_effect_batch)
            .map_err(|error| ScheduledWorkExecutionError { work_name, error })?;
    }

    Ok(())
}

fn should_schedule_follow_up_event(event: &CoreEvent) -> bool {
    matches!(
        event,
        CoreEvent::ProbeReported(
            ProbeReportedEvent::CursorColorReady {
                reuse: ProbeReuse::RefreshRequired,
                ..
            } | ProbeReportedEvent::BackgroundReady {
                reuse: ProbeReuse::RefreshRequired,
                ..
            }
        )
    )
}

fn dispatch_scheduled_core_event(event: CoreEvent) -> Result<()> {
    let mut stage_effect_batch = stage_effect_batch_on_default_queue;
    dispatch_core_event(event, &mut stage_effect_batch)
}

fn scheduled_drain_budget() -> usize {
    with_scheduled_effect_queue(|queue| {
        let queued_items = queue.items.len();
        if queued_items == 0 {
            queue.drain_scheduled = false;
            return 0;
        }

        scheduled_drain_budget_for_depth(queued_items)
    })
}

fn scheduled_drain_budget_for_depth(queued_items: usize) -> usize {
    if queued_items == 0 {
        return 0;
    }

    // Drain a bounded fraction of the queued snapshot so backlog converges geometrically under
    // burst load, while smaller queues still clear in a single shell edge.
    let half_backlog = queued_items.saturating_add(1) / 2;
    let bounded_half = half_backlog.clamp(
        MIN_SCHEDULED_WORK_ITEMS_PER_EDGE,
        MAX_SCHEDULED_WORK_ITEMS_PER_EDGE,
    );
    queued_items.min(bounded_half)
}

fn drain_scheduled_work_with_executor(
    executor: &mut impl EffectExecutor,
) -> std::result::Result<bool, ScheduledWorkExecutionError> {
    // Drain a bounded snapshot of already-staged work so backlog can recover faster, while any
    // reducer follow-ups created during this pass still remain deferred to a later scheduled edge.
    let mut remaining_budget = scheduled_drain_budget();
    let mut drained_items = 0_usize;
    while remaining_budget > 0 {
        let Some(item) = with_scheduled_effect_queue(ScheduledEffectQueueState::pop_item) else {
            with_scheduled_effect_queue(|queue| {
                queue.drain_scheduled = false;
            });
            if drained_items > 0 {
                record_scheduled_drain_items(drained_items);
            }
            return Ok(false);
        };

        match item {
            ScheduledWorkItem::EffectBatch(effects) => {
                execute_scheduled_effect_batch(effects, executor)?;
            }
            ScheduledWorkItem::CoreEvent(event) => {
                let work_name = core_event_label(&event);
                dispatch_scheduled_core_event(*event)
                    .map_err(|error| ScheduledWorkExecutionError { work_name, error })?;
            }
        }
        drained_items = drained_items.saturating_add(1);
        remaining_budget -= 1;
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
    }
    if has_more_items {
        record_scheduled_drain_reschedule();
    }

    Ok(has_more_items)
}

fn run_scheduled_effect_drain(entrypoint: ScheduledEffectDrainEntry) {
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

#[cfg(test)]
mod tests {
    use super::{
        ScheduledWorkItem, dispatch_core_event, drain_scheduled_work_with_executor,
        reset_scheduled_effect_queue, reset_scheduled_queue_after_failure,
        scheduled_drain_budget_for_depth, with_scheduled_effect_queue,
    };
    use crate::core::effect::{Effect, EventLoopMetricEffect};
    use crate::core::event::{
        Event as CoreEvent, ExternalDemandQueuedEvent, ObservationBaseCollectedEvent,
        ProbeReportedEvent,
    };
    use crate::core::reducer::reduce as reduce_core_event;
    use crate::core::state::{
        BackgroundProbeBatch, BackgroundProbeChunk, CoreState, CursorColorSample,
        ExternalDemandKind, ObservationBasis, ObservationMotion, ObservationRequest, ProbeKind,
        ProbeReuse,
    };
    use crate::core::types::{
        CursorCol, CursorPosition, CursorRow, Lifecycle, Millis, ViewportSnapshot,
    };
    use crate::events::runtime::{core_state, set_core_state};
    use crate::mutex::lock_with_poison_recovery;
    use crate::state::CursorLocation;
    use nvim_oxi::Result;
    use std::collections::VecDeque;
    use std::sync::{LazyLock, Mutex};

    static CORE_DISPATCH_TEST_MUTEX: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    fn core_dispatch_test_guard() -> std::sync::MutexGuard<'static, ()> {
        lock_with_poison_recovery(&CORE_DISPATCH_TEST_MUTEX, |_| (), |_| {})
    }

    #[derive(Default)]
    struct RecordingExecutor {
        executed_effects: Vec<Effect>,
        planned_follow_ups: VecDeque<Vec<CoreEvent>>,
    }

    impl super::EffectExecutor for RecordingExecutor {
        fn execute_effect(&mut self, effect: Effect) -> Result<Vec<CoreEvent>> {
            self.executed_effects.push(effect);
            Ok(self.planned_follow_ups.pop_front().unwrap_or_default())
        }
    }

    struct FailingExecutor;

    impl super::EffectExecutor for FailingExecutor {
        fn execute_effect(&mut self, _effect: Effect) -> Result<Vec<CoreEvent>> {
            Err(nvim_oxi::api::Error::Other("planned scheduled drain failure".to_string()).into())
        }
    }

    fn ready_state() -> CoreState {
        CoreState::default().initialize()
    }

    fn cursor(row: u32, col: u32) -> CursorPosition {
        CursorPosition {
            row: CursorRow(row),
            col: CursorCol(col),
        }
    }

    fn observation_basis(
        request: &ObservationRequest,
        position: Option<CursorPosition>,
        observed_at: u64,
    ) -> ObservationBasis {
        ObservationBasis::new(
            request.observation_id(),
            Millis::new(observed_at),
            "n".to_string(),
            position,
            CursorLocation::new(11, 22, 3, 4),
            ViewportSnapshot::new(CursorRow(40), CursorCol(120)),
        )
    }

    fn refresh_required_probe_report(request: &ObservationRequest) -> CoreEvent {
        CoreEvent::ProbeReported(ProbeReportedEvent::CursorColorReady {
            observation_id: request.observation_id(),
            probe_request_id: ProbeKind::CursorColor.request_id(request.observation_id()),
            reuse: ProbeReuse::RefreshRequired,
            sample: Some(CursorColorSample::new("#abcdef".to_string())),
        })
    }

    fn compatible_probe_report(request: &ObservationRequest) -> CoreEvent {
        CoreEvent::ProbeReported(ProbeReportedEvent::CursorColorReady {
            observation_id: request.observation_id(),
            probe_request_id: ProbeKind::CursorColor.request_id(request.observation_id()),
            reuse: ProbeReuse::Compatible,
            sample: Some(CursorColorSample::new("#abcdef".to_string())),
        })
    }

    fn background_probe_report(
        request: &ObservationRequest,
        viewport: ViewportSnapshot,
    ) -> CoreEvent {
        CoreEvent::ProbeReported(ProbeReportedEvent::BackgroundReady {
            observation_id: request.observation_id(),
            probe_request_id: ProbeKind::Background.request_id(request.observation_id()),
            reuse: ProbeReuse::Exact,
            batch: BackgroundProbeBatch::empty(viewport),
        })
    }

    fn background_chunk_probe_report(
        request: &ObservationRequest,
        chunk: BackgroundProbeChunk,
        viewport: ViewportSnapshot,
    ) -> CoreEvent {
        let width = usize::try_from(viewport.max_col.value()).expect("viewport width");
        let row_count = usize::try_from(chunk.row_count()).expect("chunk row count");
        CoreEvent::ProbeReported(ProbeReportedEvent::BackgroundChunkReady {
            observation_id: request.observation_id(),
            probe_request_id: ProbeKind::Background.request_id(request.observation_id()),
            chunk,
            allowed_mask: vec![false; width * row_count],
        })
    }

    struct CoreDispatchTestContext {
        _guard: std::sync::MutexGuard<'static, ()>,
    }

    impl CoreDispatchTestContext {
        fn new() -> Self {
            let guard = core_dispatch_test_guard();
            replace_core_state(CoreState::default());
            reset_scheduled_effect_queue();
            Self { _guard: guard }
        }

        fn set_core_state(&self, state: CoreState) {
            replace_core_state(state);
        }

        fn dispatch_external_cursor_ingress_to_queue(
            &self,
            observed_at: u64,
        ) -> ObservationRequest {
            dispatch_core_event(external_cursor_demand(observed_at), &mut |effects| {
                // CONTEXT: `stage_batch` reports whether this enqueue operation also needs to arm
                // the drain edge; it does not signal whether the batch was accepted.
                let should_schedule =
                    with_scheduled_effect_queue(|queue| queue.stage_batch(effects));
                assert!(
                    should_schedule,
                    "ingress dispatch should arm exactly one scheduled work item"
                );
            })
            .expect("ingress dispatch should commit reducer state");

            current_core_state()
                .active_observation_request()
                .cloned()
                .expect("ingress dispatch should leave an active observation request")
        }

        fn observing_state_after_base_collection(&self) -> (ObservationRequest, CoreState) {
            self.set_core_state(ready_state_with_cursor_color_probe());
            let observing =
                reduce_core_event(&current_core_state(), external_cursor_demand(25)).next;
            let request = observing
                .active_observation_request()
                .cloned()
                .expect("active observation request");
            let based = reduce_core_event(
                &observing,
                observation_base_collected(
                    &request,
                    observation_basis(&request, Some(cursor(7, 8)), 26),
                ),
            );
            self.set_core_state(based.next.clone());
            (request, based.next)
        }
    }

    impl Drop for CoreDispatchTestContext {
        fn drop(&mut self) {
            replace_core_state(CoreState::default());
            reset_scheduled_effect_queue();
        }
    }

    fn current_core_state() -> CoreState {
        core_state().expect("test core state access should not re-enter")
    }

    fn replace_core_state(state: CoreState) {
        set_core_state(state).expect("test core state write should not re-enter")
    }

    fn external_cursor_demand(observed_at: u64) -> CoreEvent {
        CoreEvent::ExternalDemandQueued(ExternalDemandQueuedEvent {
            kind: ExternalDemandKind::ExternalCursor,
            observed_at: Millis::new(observed_at),
            requested_target: None,
            ingress_cursor_presentation: None,
        })
    }

    fn observation_base_collected(
        request: &ObservationRequest,
        basis: ObservationBasis,
    ) -> CoreEvent {
        CoreEvent::ObservationBaseCollected(ObservationBaseCollectedEvent {
            request: request.clone(),
            basis,
            motion: ObservationMotion::default(),
        })
    }

    fn ready_state_with_cursor_color_probe() -> CoreState {
        let mut runtime = ready_state().runtime().clone();
        runtime.config.cursor_color = Some("none".to_string());
        ready_state().with_runtime(runtime)
    }

    fn ready_state_with_cursor_and_background_probes() -> CoreState {
        let mut runtime = ready_state().runtime().clone();
        runtime.config.cursor_color = Some("none".to_string());
        runtime.config.particles_enabled = true;
        runtime.config.particles_over_text = false;
        ready_state().with_runtime(runtime)
    }

    fn queue_stage_batch(effects: Vec<Effect>) -> bool {
        with_scheduled_effect_queue(|queue| queue.stage_batch(effects))
    }

    fn queued_work_count() -> usize {
        with_scheduled_effect_queue(|queue| queue.items.len())
    }

    fn queued_front_work_item() -> Option<ScheduledWorkItem> {
        with_scheduled_effect_queue(|queue| queue.items.front().cloned())
    }

    fn queue_is_marked_scheduled() -> bool {
        with_scheduled_effect_queue(|queue| queue.drain_scheduled)
    }

    fn drain_next_edge(executor: &mut RecordingExecutor) -> bool {
        drain_scheduled_work_with_executor(executor)
            .expect("scheduled drain should execute one queued edge")
    }

    fn contains_observation_base_request(effects: &[Effect]) -> bool {
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::RequestObservationBase(_)))
    }

    fn contains_probe_request(effects: &[Effect]) -> bool {
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::RequestProbe(_)))
    }

    fn contains_render_plan_request(effects: &[Effect]) -> bool {
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::RequestRenderPlan(_)))
    }

    fn is_apply_proposal(effect: &Effect) -> bool {
        matches!(effect, Effect::ApplyProposal(_))
    }

    fn only_cursor_color_probe_request(effects: &[Effect]) -> bool {
        matches!(
            effects,
            [Effect::RequestProbe(payload)] if payload.kind == ProbeKind::CursorColor
        )
    }

    fn only_background_probe_request_for_chunk(
        effects: &[Effect],
        expected_chunk: BackgroundProbeChunk,
    ) -> bool {
        matches!(
            effects,
            [Effect::RequestProbe(payload)]
                if payload.kind == ProbeKind::Background
                    && payload.background_chunk == Some(expected_chunk)
        )
    }

    mod dispatch_core_event {
        use super::*;

        #[test]
        fn stages_observation_request_work_for_external_cursor_ingress() {
            let scope = CoreDispatchTestContext::new();
            scope.set_core_state(ready_state());
            let mut staged_batches = Vec::new();

            dispatch_core_event(external_cursor_demand(21), &mut |effects| {
                staged_batches.push(effects)
            })
            .expect("external ingress dispatch should succeed");

            assert_eq!(staged_batches.len(), 1);
            assert!(
                contains_observation_base_request(&staged_batches[0]),
                "expected queued observation request effect"
            );
        }

        #[test]
        fn commits_observing_state_before_shell_work_runs() {
            let scope = CoreDispatchTestContext::new();
            scope.set_core_state(ready_state());

            dispatch_core_event(external_cursor_demand(21), &mut |_| {})
                .expect("external ingress dispatch should succeed");

            let staged_state = current_core_state();
            assert_eq!(staged_state.lifecycle(), Lifecycle::Observing);
            assert!(
                staged_state.active_observation_request().is_some(),
                "dispatch should commit reducer state before shell work runs"
            );
            assert!(
                staged_state.observation().is_none(),
                "observation collection must stay deferred until the scheduled shell edge"
            );
        }
    }

    mod scheduled_effect_drain {
        use super::*;

        fn stage_two_effect_batches() -> CoreDispatchTestContext {
            let scope = CoreDispatchTestContext::new();
            assert!(queue_stage_batch(vec![Effect::RedrawCmdline]));
            assert!(!queue_stage_batch(vec![Effect::RecordEventLoopMetric(
                EventLoopMetricEffect::StaleToken,
            )]));
            scope
        }

        #[test]
        fn first_edge_executes_a_bounded_snapshot_of_existing_batches() {
            let _scope = stage_two_effect_batches();
            let mut executor = RecordingExecutor::default();

            let has_more_items = drain_next_edge(&mut executor);

            assert!(
                !has_more_items,
                "bounded drain should finish the queued snapshot when it fits in budget"
            );
            assert_eq!(
                executor.executed_effects,
                vec![
                    Effect::RedrawCmdline,
                    Effect::RecordEventLoopMetric(EventLoopMetricEffect::StaleToken),
                ]
            );
        }

        #[test]
        fn first_edge_clears_the_queue_and_scheduled_flag_when_snapshot_finishes() {
            let _scope = stage_two_effect_batches();
            let mut executor = RecordingExecutor::default();

            let _ = drain_next_edge(&mut executor);

            assert_eq!(queued_work_count(), 0, "queued snapshot should fully drain");
            assert!(
                !queue_is_marked_scheduled(),
                "queue should clear its scheduled flag once the staged snapshot is drained"
            );
        }

        #[test]
        fn first_edge_leaves_new_follow_up_work_for_a_later_edge() {
            let _scope = stage_two_effect_batches();
            replace_core_state(ready_state());
            let mut executor = RecordingExecutor::default();
            executor
                .planned_follow_ups
                .push_back(vec![external_cursor_demand(21)]);
            executor.planned_follow_ups.push_back(Vec::new());

            let has_more_items = drain_next_edge(&mut executor);

            assert!(
                has_more_items,
                "follow-up work staged during the drain should remain deferred"
            );
            assert_eq!(
                executor.executed_effects,
                vec![
                    Effect::RedrawCmdline,
                    Effect::RecordEventLoopMetric(EventLoopMetricEffect::StaleToken),
                ]
            );
            assert!(
                matches!(
                    queued_front_work_item(),
                    Some(ScheduledWorkItem::EffectBatch(ref effects))
                        if contains_observation_base_request(effects)
                ),
                "newly staged follow-up work should remain queued for the next scheduled edge"
            );
            assert!(
                queue_is_marked_scheduled(),
                "queue must stay armed when new follow-up work remains"
            );
        }

        #[test]
        fn drain_failure_resets_the_scheduled_queue_state() {
            let _scope = stage_two_effect_batches();
            let mut executor = FailingExecutor;

            let err = drain_scheduled_work_with_executor(&mut executor)
                .expect_err("planned executor failure should surface from the drain");
            let _ = err;
            reset_scheduled_queue_after_failure();

            assert_eq!(
                queued_work_count(),
                0,
                "failure reset should clear queued work"
            );
            assert!(
                !queue_is_marked_scheduled(),
                "failure reset should disarm the scheduled drain flag"
            );
        }

        #[test]
        fn backlog_aware_budget_clears_small_queues_and_caps_large_backlogs() {
            assert_eq!(scheduled_drain_budget_for_depth(0), 0);
            assert_eq!(scheduled_drain_budget_for_depth(3), 3);
            assert_eq!(scheduled_drain_budget_for_depth(16), 16);
            assert_eq!(scheduled_drain_budget_for_depth(24), 16);
            assert_eq!(scheduled_drain_budget_for_depth(40), 20);
            assert_eq!(scheduled_drain_budget_for_depth(117), 32);
        }

        #[test]
        fn first_edge_reschedules_when_backlog_exceeds_the_bounded_fractional_budget() {
            let _scope = CoreDispatchTestContext::new();
            for _ in 0..40 {
                let should_schedule = queue_stage_batch(vec![Effect::RedrawCmdline]);
                if queued_work_count() == 1 {
                    assert!(should_schedule, "first staged item should arm the drain");
                } else {
                    assert!(
                        !should_schedule,
                        "subsequent staged items should reuse the armed drain edge"
                    );
                }
            }
            let mut executor = RecordingExecutor::default();

            let has_more_items = drain_next_edge(&mut executor);

            assert!(
                has_more_items,
                "large backlog should keep work queued for a later edge"
            );
            assert_eq!(
                executor.executed_effects.len(),
                20,
                "40 queued items should drain half the backlog on the first edge"
            );
            assert_eq!(
                queued_work_count(),
                20,
                "remaining backlog should stay queued after the first bounded drain"
            );
            assert!(
                queue_is_marked_scheduled(),
                "queue should remain armed while bounded backlog remains"
            );
        }
    }

    mod refresh_required_probe_retry {
        use super::*;

        fn setup_refresh_required_retry() -> (
            CoreDispatchTestContext,
            ObservationRequest,
            CoreState,
            CoreEvent,
            RecordingExecutor,
        ) {
            let scope = CoreDispatchTestContext::new();
            let (request, based_state) = scope.observing_state_after_base_collection();
            let refresh_required = refresh_required_probe_report(&request);
            assert!(queue_stage_batch(vec![Effect::RedrawCmdline]));
            let executor = RecordingExecutor {
                planned_follow_ups: VecDeque::from([vec![refresh_required.clone()]]),
                ..RecordingExecutor::default()
            };
            (scope, request, based_state, refresh_required, executor)
        }

        #[test]
        fn probe_edge_requeues_refresh_required_follow_up_as_a_core_event() {
            let (_scope, _request, _based_state, refresh_required, mut executor) =
                setup_refresh_required_retry();

            let has_more_items = drain_next_edge(&mut executor);

            assert!(
                has_more_items,
                "refresh-required probe follow-up should remain queued for a later edge"
            );
            assert_eq!(
                queued_work_count(),
                1,
                "retry event should be queued explicitly"
            );
            assert!(matches!(
                queued_front_work_item(),
                Some(ScheduledWorkItem::CoreEvent(event)) if *event == refresh_required
            ));
        }

        #[test]
        fn probe_edge_leaves_the_active_state_unchanged_until_the_retry_event_runs() {
            let (_scope, _request, based_state, _refresh_required, mut executor) =
                setup_refresh_required_retry();

            let _ = drain_next_edge(&mut executor);

            assert_eq!(current_core_state(), based_state);
        }

        #[test]
        fn retry_edge_keeps_the_active_request_authoritative() {
            let (_scope, request, _based_state, _refresh_required, mut executor) =
                setup_refresh_required_retry();

            let _ = drain_next_edge(&mut executor);
            let _ = drain_next_edge(&mut executor);

            let retried_state = current_core_state();
            assert_eq!(retried_state.lifecycle(), Lifecycle::Observing);
            assert_eq!(retried_state.active_observation_request(), Some(&request));
        }

        #[test]
        fn retry_edge_clears_the_mixed_world_observation_before_replay() {
            let (_scope, _request, _based_state, _refresh_required, mut executor) =
                setup_refresh_required_retry();

            let _ = drain_next_edge(&mut executor);
            let _ = drain_next_edge(&mut executor);

            assert!(
                current_core_state().observation().is_none(),
                "refresh-required retry should clear retained observation data before replay"
            );
        }

        #[test]
        fn retry_edge_stages_a_new_observation_base_request_for_a_later_edge() {
            let (_scope, _request, _based_state, _refresh_required, mut executor) =
                setup_refresh_required_retry();

            let _ = drain_next_edge(&mut executor);
            let has_more_items = drain_next_edge(&mut executor);

            assert!(
                has_more_items,
                "retry transition should stage a later observation batch"
            );
            assert_eq!(queued_work_count(), 1);
            assert!(matches!(
                queued_front_work_item(),
                Some(ScheduledWorkItem::EffectBatch(ref effects))
                    if contains_observation_base_request(effects)
            ));
        }
    }

    mod deferred_single_probe_observation {
        use super::*;

        fn setup_cursor_probe_ingress() -> (CoreDispatchTestContext, ObservationRequest) {
            let scope = CoreDispatchTestContext::new();
            scope.set_core_state(ready_state_with_cursor_color_probe());
            let request = scope.dispatch_external_cursor_ingress_to_queue(25);
            (scope, request)
        }

        #[test]
        fn ingress_dispatch_queues_one_observation_base_batch() {
            let (_scope, _request) = setup_cursor_probe_ingress();
            let after_ingress = current_core_state();

            assert_eq!(after_ingress.lifecycle(), Lifecycle::Observing);
            assert!(after_ingress.observation().is_none());
            assert!(after_ingress.pending_proposal().is_none());
            assert_eq!(
                queued_work_count(),
                1,
                "ingress should queue one effect batch"
            );
            assert!(matches!(
                queued_front_work_item(),
                Some(ScheduledWorkItem::EffectBatch(ref effects))
                    if contains_observation_base_request(effects)
            ));
        }

        #[test]
        fn observation_base_edge_executes_only_the_observation_request() {
            let (_scope, request) = setup_cursor_probe_ingress();
            let mut executor = RecordingExecutor::default();
            executor
                .planned_follow_ups
                .push_back(vec![observation_base_collected(
                    &request,
                    observation_basis(&request, Some(cursor(7, 8)), 26),
                )]);

            let has_more_items = drain_next_edge(&mut executor);

            assert!(
                has_more_items,
                "observation collection should stage probe work for a later edge"
            );
            assert!(matches!(
                executor.executed_effects.as_slice(),
                [Effect::RequestObservationBase(_)]
            ));
        }

        #[test]
        fn observation_base_edge_records_the_observation_and_queues_probe_work() {
            let (_scope, request) = setup_cursor_probe_ingress();
            let mut executor = RecordingExecutor::default();
            executor
                .planned_follow_ups
                .push_back(vec![observation_base_collected(
                    &request,
                    observation_basis(&request, Some(cursor(7, 8)), 26),
                )]);

            let _ = drain_next_edge(&mut executor);
            let after_observation = current_core_state();

            assert_eq!(after_observation.lifecycle(), Lifecycle::Observing);
            assert!(after_observation.observation().is_some());
            assert!(after_observation.pending_proposal().is_none());
            assert_eq!(queued_work_count(), 1, "probe work should remain queued");
            assert!(matches!(
                queued_front_work_item(),
                Some(ScheduledWorkItem::EffectBatch(ref effects)) if contains_probe_request(effects)
            ));
        }

        fn setup_after_observation_base_edge() -> (
            CoreDispatchTestContext,
            ObservationRequest,
            RecordingExecutor,
        ) {
            let (scope, request) = setup_cursor_probe_ingress();
            let mut executor = RecordingExecutor::default();
            executor
                .planned_follow_ups
                .push_back(vec![observation_base_collected(
                    &request,
                    observation_basis(&request, Some(cursor(7, 8)), 26),
                )]);
            let _ = drain_next_edge(&mut executor);
            (scope, request, executor)
        }

        #[test]
        fn cursor_color_probe_edge_updates_the_retained_observation() {
            let (_scope, request, mut executor) = setup_after_observation_base_edge();
            executor
                .planned_follow_ups
                .push_back(vec![compatible_probe_report(&request)]);

            let _ = drain_next_edge(&mut executor);

            assert_eq!(
                current_core_state()
                    .observation()
                    .and_then(|observation| observation.cursor_color()),
                Some("#abcdef")
            );
        }

        #[test]
        fn cursor_color_probe_edge_keeps_apply_work_deferred() {
            let (_scope, request, mut executor) = setup_after_observation_base_edge();
            executor
                .planned_follow_ups
                .push_back(vec![compatible_probe_report(&request)]);

            let _ = drain_next_edge(&mut executor);

            assert!(
                !executor.executed_effects.iter().any(is_apply_proposal),
                "apply work must remain deferred after the probe shell read finishes because planning still runs first"
            );
        }

        #[test]
        fn cursor_color_probe_edge_leaves_follow_up_shell_work_for_later_edges() {
            let (_scope, request, mut executor) = setup_after_observation_base_edge();
            executor
                .planned_follow_ups
                .push_back(vec![compatible_probe_report(&request)]);

            let has_more_items = drain_next_edge(&mut executor);
            let queued_follow_up = queued_front_work_item();

            assert!(
                if has_more_items {
                    matches!(
                        queued_follow_up,
                        Some(ScheduledWorkItem::EffectBatch(ref effects))
                            if contains_render_plan_request(effects)
                                || effects.iter().any(is_apply_proposal)
                    )
                } else {
                    queued_follow_up.is_none()
                },
                "probe completion should either queue planning/apply work or finish without extra shell work"
            );
        }
    }

    mod deferred_multi_probe_observation {
        use super::*;

        fn setup_multi_probe_ingress() -> (
            CoreDispatchTestContext,
            ObservationRequest,
            ObservationBasis,
        ) {
            let scope = CoreDispatchTestContext::new();
            scope.set_core_state(ready_state_with_cursor_and_background_probes());
            let request = scope.dispatch_external_cursor_ingress_to_queue(25);
            let basis = observation_basis(&request, Some(cursor(7, 8)), 26);
            (scope, request, basis)
        }

        fn setup_after_observation_base_edge() -> (
            CoreDispatchTestContext,
            ObservationRequest,
            ObservationBasis,
            RecordingExecutor,
        ) {
            let (scope, request, basis) = setup_multi_probe_ingress();
            let mut executor = RecordingExecutor::default();
            executor
                .planned_follow_ups
                .push_back(vec![observation_base_collected(&request, basis.clone())]);
            let _ = drain_next_edge(&mut executor);
            (scope, request, basis, executor)
        }

        fn setup_after_cursor_color_probe_edge() -> (
            CoreDispatchTestContext,
            ObservationRequest,
            ObservationBasis,
            BackgroundProbeChunk,
            RecordingExecutor,
        ) {
            let (scope, request, basis, mut executor) = setup_after_observation_base_edge();
            executor
                .planned_follow_ups
                .push_back(vec![compatible_probe_report(&request)]);
            let _ = drain_next_edge(&mut executor);
            let first_background_chunk = current_core_state()
                .observation()
                .and_then(|observation| observation.background_progress())
                .and_then(crate::core::state::BackgroundProbeProgress::next_chunk)
                .expect("first background chunk");
            (scope, request, basis, first_background_chunk, executor)
        }

        #[test]
        fn observation_base_edge_queues_only_the_cursor_color_probe() {
            let (_scope, _request, _basis, executor) = setup_after_observation_base_edge();

            assert!(matches!(
                executor.executed_effects.as_slice(),
                [Effect::RequestObservationBase(_)]
            ));
            assert_eq!(
                queued_work_count(),
                1,
                "only one probe batch should be queued"
            );
            assert!(matches!(
                queued_front_work_item(),
                Some(ScheduledWorkItem::EffectBatch(ref effects))
                    if only_cursor_color_probe_request(effects)
            ));
        }

        #[test]
        fn cursor_color_probe_edge_queues_the_first_background_chunk() {
            let (_scope, _request, _basis, first_background_chunk, executor) =
                setup_after_cursor_color_probe_edge();

            assert!(matches!(
                executor.executed_effects[1],
                Effect::RequestProbe(ref payload) if payload.kind == ProbeKind::CursorColor
            ));
            assert_eq!(queued_work_count(), 1);
            assert!(matches!(
                queued_front_work_item(),
                Some(ScheduledWorkItem::EffectBatch(ref effects))
                    if only_background_probe_request_for_chunk(effects, first_background_chunk)
            ));
        }

        #[test]
        fn background_chunk_edge_queues_the_next_background_chunk() {
            let (_scope, request, basis, first_background_chunk, mut executor) =
                setup_after_cursor_color_probe_edge();
            executor
                .planned_follow_ups
                .push_back(vec![background_chunk_probe_report(
                    &request,
                    first_background_chunk,
                    basis.viewport(),
                )]);

            let has_more_items = drain_next_edge(&mut executor);
            let second_background_chunk = current_core_state()
                .observation()
                .and_then(|observation| observation.background_progress())
                .and_then(crate::core::state::BackgroundProbeProgress::next_chunk)
                .expect("second background chunk");

            assert!(
                has_more_items,
                "background chunk completion should queue the next chunk"
            );
            assert!(matches!(
                executor.executed_effects[2],
                Effect::RequestProbe(ref payload)
                    if payload.kind == ProbeKind::Background
                        && payload.background_chunk == Some(first_background_chunk)
            ));
            assert_eq!(queued_work_count(), 1);
            assert!(matches!(
                queued_front_work_item(),
                Some(ScheduledWorkItem::EffectBatch(ref effects))
                    if only_background_probe_request_for_chunk(effects, second_background_chunk)
            ));
        }

        #[test]
        fn final_background_edge_transitions_the_runtime_to_planning() {
            let (_scope, request, basis, first_background_chunk, mut executor) =
                setup_after_cursor_color_probe_edge();
            executor
                .planned_follow_ups
                .push_back(vec![background_chunk_probe_report(
                    &request,
                    first_background_chunk,
                    basis.viewport(),
                )]);
            let _ = drain_next_edge(&mut executor);
            executor
                .planned_follow_ups
                .push_back(vec![background_probe_report(&request, basis.viewport())]);

            let _ = drain_next_edge(&mut executor);
            let after_completion = current_core_state();

            assert_eq!(after_completion.lifecycle(), Lifecycle::Planning);
            assert!(after_completion.pending_proposal().is_none());
            assert!(after_completion.pending_plan_proposal_id().is_some());
        }

        #[test]
        fn final_background_edge_queues_render_plan_work_for_a_later_edge() {
            let (_scope, request, basis, first_background_chunk, mut executor) =
                setup_after_cursor_color_probe_edge();
            executor
                .planned_follow_ups
                .push_back(vec![background_chunk_probe_report(
                    &request,
                    first_background_chunk,
                    basis.viewport(),
                )]);
            let _ = drain_next_edge(&mut executor);
            executor
                .planned_follow_ups
                .push_back(vec![background_probe_report(&request, basis.viewport())]);

            let has_more_items = drain_next_edge(&mut executor);

            assert!(
                has_more_items,
                "planning work should remain deferred to a later edge"
            );
            assert_eq!(queued_work_count(), 1, "planning work should remain queued");
            assert!(matches!(
                queued_front_work_item(),
                Some(ScheduledWorkItem::EffectBatch(ref effects))
                    if contains_render_plan_request(effects)
            ));
        }
    }
}
