use super::super::logging::{trace_lazy, warn};
use super::super::runtime::{
    EffectExecutor, NeovimEffectExecutor, core_state, now_ms, record_scheduled_queue_depth,
    set_core_state, to_core_millis,
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

const MAX_SCHEDULED_WORK_ITEMS_PER_EDGE: usize = 1;

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
    let previous_state = core_state();
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
    set_core_state(transition.next);
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

pub(crate) fn reset_scheduled_effect_queue() {
    with_scheduled_effect_queue(ScheduledEffectQueueState::reset);
}

#[derive(Debug)]
struct ScheduledWorkExecutionError {
    work_name: &'static str,
    error: nvim_oxi::Error,
}

fn handle_scheduled_work_drain_failure(work_name: &'static str, error: &nvim_oxi::Error) {
    warn(&format!("scheduled core work failed: {work_name}: {error}"));
    reset_scheduled_effect_queue();
    let observed_at = to_core_millis(now_ms());
    if let Err(dispatch_err) =
        dispatch_core_event_with_default_scheduler(CoreEvent::EffectFailed(EffectFailedEvent {
            proposal_id: None,
            observed_at,
        }))
    {
        warn(&format!(
            "failed to queue scheduled effect failure recovery: {dispatch_err}"
        ));
    }
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
        if should_schedule_follow_up_event(&follow_up) {
            // retry-class probe reports stay typed reducer inputs, but they hop back onto
            // the scheduled queue so one probe edge cannot immediately replay the next observation.
            stage_core_event_on_default_queue(follow_up);
            continue;
        }

        let work_name = core_event_label(&follow_up);
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

fn dispatch_scheduled_core_event(
    event: CoreEvent,
) -> std::result::Result<(), ScheduledWorkExecutionError> {
    let work_name = core_event_label(&event);
    let mut stage_effect_batch = stage_effect_batch_on_default_queue;
    dispatch_core_event(event, &mut stage_effect_batch)
        .map_err(|error| ScheduledWorkExecutionError { work_name, error })
}

fn drain_scheduled_work_with_executor(
    executor: &mut impl EffectExecutor,
) -> std::result::Result<bool, ScheduledWorkExecutionError> {
    let mut remaining_budget = MAX_SCHEDULED_WORK_ITEMS_PER_EDGE;
    while remaining_budget > 0 {
        let Some(item) = with_scheduled_effect_queue(ScheduledEffectQueueState::pop_item) else {
            with_scheduled_effect_queue(|queue| {
                queue.drain_scheduled = false;
            });
            return Ok(false);
        };

        match item {
            ScheduledWorkItem::EffectBatch(effects) => {
                execute_scheduled_effect_batch(effects, executor)?;
            }
            ScheduledWorkItem::CoreEvent(event) => {
                dispatch_scheduled_core_event(*event)?;
            }
        }
        remaining_budget -= 1;
    }

    let has_more_items = with_scheduled_effect_queue(|queue| {
        let has_more_items = !queue.items.is_empty();
        if !has_more_items {
            queue.drain_scheduled = false;
        }
        has_more_items
    });

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
        reset_scheduled_effect_queue, with_scheduled_effect_queue,
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

    #[test]
    fn dispatch_core_event_stages_effects_without_executing_them_inline() {
        let _guard = core_dispatch_test_guard();
        reset_scheduled_effect_queue();
        set_core_state(ready_state());
        let mut staged_batches = Vec::new();

        dispatch_core_event(
            CoreEvent::ExternalDemandQueued(ExternalDemandQueuedEvent {
                kind: ExternalDemandKind::ExternalCursor,
                observed_at: Millis::new(21),
                requested_target: None,
                ingress_cursor_presentation: None,
            }),
            &mut |effects| staged_batches.push(effects),
        )
        .expect("dispatch should stage shell work");

        assert_eq!(staged_batches.len(), 1);
        assert!(
            staged_batches[0]
                .iter()
                .any(|effect| matches!(effect, Effect::RequestObservationBase(_))),
            "expected queued observation request effect"
        );

        let staged_state = core_state();
        assert_eq!(staged_state.lifecycle(), Lifecycle::Observing);
        assert!(
            staged_state.active_observation_request().is_some(),
            "dispatch should commit reducer state before shell work runs"
        );
        assert!(
            staged_state.observation().is_none(),
            "observation collection must stay deferred until the scheduled shell edge"
        );

        set_core_state(CoreState::default());
        reset_scheduled_effect_queue();
    }

    #[test]
    fn scheduled_effect_drain_processes_one_batch_per_edge() {
        let _guard = core_dispatch_test_guard();
        reset_scheduled_effect_queue();
        with_scheduled_effect_queue(|queue| {
            assert!(queue.stage_batch(vec![Effect::RedrawCmdline]));
            assert!(!queue.stage_batch(vec![Effect::RecordEventLoopMetric(
                EventLoopMetricEffect::StaleToken,
            )]));
        });

        let mut executor = RecordingExecutor::default();
        let has_more_items = drain_scheduled_work_with_executor(&mut executor)
            .expect("scheduled drain should execute one batch");

        assert!(
            has_more_items,
            "first scheduled edge should leave later work queued"
        );
        assert_eq!(executor.executed_effects, vec![Effect::RedrawCmdline]);
        with_scheduled_effect_queue(|queue| {
            assert_eq!(queue.items.len(), 1, "one work item should remain queued");
            assert!(
                queue.drain_scheduled,
                "queue should stay marked scheduled until the remaining work item is drained"
            );
        });

        let has_more_items = drain_scheduled_work_with_executor(&mut executor)
            .expect("second scheduled edge should drain the remaining work item");

        assert!(
            !has_more_items,
            "second scheduled edge should finish the remaining queued work"
        );
        assert_eq!(
            executor.executed_effects,
            vec![
                Effect::RedrawCmdline,
                Effect::RecordEventLoopMetric(EventLoopMetricEffect::StaleToken),
            ]
        );
        with_scheduled_effect_queue(|queue| {
            assert!(
                queue.items.is_empty(),
                "queue should be empty after the second drain edge"
            );
            assert!(
                !queue.drain_scheduled,
                "queue should clear its scheduled flag once all batches are drained"
            );
        });

        reset_scheduled_effect_queue();
    }

    #[test]
    fn refresh_required_probe_report_is_requeued_as_scheduled_core_event() {
        let _guard = core_dispatch_test_guard();
        reset_scheduled_effect_queue();

        let mut runtime = ready_state().runtime().clone();
        runtime.config.cursor_color = Some("none".to_string());
        let ready = ready_state().with_runtime(runtime);
        let observing = reduce_core_event(
            &ready,
            CoreEvent::ExternalDemandQueued(ExternalDemandQueuedEvent {
                kind: ExternalDemandKind::ExternalCursor,
                observed_at: Millis::new(25),
                requested_target: None,
                ingress_cursor_presentation: None,
            }),
        )
        .next;
        let request = observing
            .active_observation_request()
            .cloned()
            .expect("active observation request");
        let based = reduce_core_event(
            &observing,
            CoreEvent::ObservationBaseCollected(ObservationBaseCollectedEvent {
                request: request.clone(),
                basis: observation_basis(&request, Some(cursor(7, 8)), 26),
                motion: ObservationMotion::default(),
            }),
        );
        set_core_state(based.next.clone());

        let refresh_required = refresh_required_probe_report(&request);
        with_scheduled_effect_queue(|queue| {
            assert!(queue.stage_batch(vec![Effect::RedrawCmdline]));
        });

        let mut executor = RecordingExecutor {
            planned_follow_ups: VecDeque::from([vec![refresh_required.clone()]]),
            ..RecordingExecutor::default()
        };

        let has_more_items = drain_scheduled_work_with_executor(&mut executor)
            .expect("probe edge should requeue refresh-required follow-up");

        assert!(
            has_more_items,
            "refresh-required probe follow-up should remain queued for a later edge"
        );
        assert_eq!(core_state(), based.next);
        with_scheduled_effect_queue(|queue| {
            assert_eq!(
                queue.items.len(),
                1,
                "retry event should be queued explicitly"
            );
            assert!(matches!(
                queue.items.front(),
                Some(ScheduledWorkItem::CoreEvent(event)) if **event == refresh_required
            ));
        });

        let has_more_items = drain_scheduled_work_with_executor(&mut executor)
            .expect("queued retry event should dispatch on the next edge");

        assert!(
            has_more_items,
            "retry transition should stage a later observation batch"
        );
        let retried_state = core_state();
        assert_eq!(retried_state.lifecycle(), Lifecycle::Observing);
        assert_eq!(
            retried_state.active_observation_request(),
            Some(&request),
            "retry transition should keep the active request authoritative"
        );
        assert!(
            retried_state.observation().is_none(),
            "refresh-required retry should clear mixed-world observation data before replay"
        );
        with_scheduled_effect_queue(|queue| {
            assert_eq!(
                queue.items.len(),
                1,
                "retry transition should stage one later effect batch"
            );
            assert!(matches!(
                queue.items.front(),
                Some(ScheduledWorkItem::EffectBatch(effects))
                    if effects
                        .iter()
                        .any(|effect| matches!(effect, Effect::RequestObservationBase(_)))
            ));
        });

        set_core_state(CoreState::default());
        reset_scheduled_effect_queue();
    }

    #[test]
    fn ingress_event_defers_observation_and_probe_work_across_multiple_scheduled_edges() {
        let _guard = core_dispatch_test_guard();
        reset_scheduled_effect_queue();

        let mut runtime = ready_state().runtime().clone();
        runtime.config.cursor_color = Some("none".to_string());
        set_core_state(ready_state().with_runtime(runtime));

        dispatch_core_event(
            CoreEvent::ExternalDemandQueued(ExternalDemandQueuedEvent {
                kind: ExternalDemandKind::ExternalCursor,
                observed_at: Millis::new(25),
                requested_target: None,
                ingress_cursor_presentation: None,
            }),
            &mut |effects| {
                let should_schedule =
                    with_scheduled_effect_queue(|queue| queue.stage_batch(effects));
                assert!(
                    should_schedule,
                    "ingress dispatch should arm exactly one scheduled work item"
                );
            },
        )
        .expect("ingress dispatch should commit state and queue initial observation work");

        let request = core_state()
            .active_observation_request()
            .cloned()
            .expect("ingress dispatch should leave an active observation request");
        let after_ingress = core_state();
        assert_eq!(after_ingress.lifecycle(), Lifecycle::Observing);
        assert!(after_ingress.observation().is_none());
        assert!(after_ingress.pending_proposal().is_none());
        with_scheduled_effect_queue(|queue| {
            assert_eq!(
                queue.items.len(),
                1,
                "ingress should queue one effect batch"
            );
            assert!(matches!(
                queue.items.front(),
                Some(ScheduledWorkItem::EffectBatch(effects))
                    if effects
                        .iter()
                        .any(|effect| matches!(effect, Effect::RequestObservationBase(_)))
            ));
        });

        let mut executor = RecordingExecutor::default();
        executor
            .planned_follow_ups
            .push_back(vec![CoreEvent::ObservationBaseCollected(
                ObservationBaseCollectedEvent {
                    request: request.clone(),
                    basis: observation_basis(&request, Some(cursor(7, 8)), 26),
                    motion: ObservationMotion::default(),
                },
            )]);

        let has_more_items = drain_scheduled_work_with_executor(&mut executor)
            .expect("first scheduled edge should execute observation collection only");

        assert!(
            has_more_items,
            "observation collection should stage probe work for a later edge"
        );
        assert_eq!(executor.executed_effects.len(), 1);
        assert!(matches!(
            executor.executed_effects.as_slice(),
            [Effect::RequestObservationBase(_)]
        ));
        let after_observation = core_state();
        assert_eq!(after_observation.lifecycle(), Lifecycle::Observing);
        assert!(after_observation.observation().is_some());
        assert!(after_observation.pending_proposal().is_none());
        with_scheduled_effect_queue(|queue| {
            assert_eq!(queue.items.len(), 1, "probe work should remain queued");
            assert!(matches!(
                queue.items.front(),
                Some(ScheduledWorkItem::EffectBatch(effects))
                    if effects
                        .iter()
                        .any(|effect| matches!(effect, Effect::RequestProbe(_)))
            ));
        });

        executor
            .planned_follow_ups
            .push_back(vec![compatible_probe_report(&request)]);

        let has_more_items = drain_scheduled_work_with_executor(&mut executor)
            .expect("second scheduled edge should execute probe work only");

        assert_eq!(executor.executed_effects.len(), 2);
        assert!(matches!(
            executor.executed_effects.as_slice(),
            [Effect::RequestObservationBase(_), Effect::RequestProbe(_)]
        ));
        let after_probe = core_state();
        assert_eq!(
            after_probe
                .observation()
                .and_then(|observation| observation.cursor_color()),
            Some("#abcdef"),
            "probe completion should update the retained observation on a later edge"
        );
        assert!(
            !executor
                .executed_effects
                .iter()
                .any(|effect| matches!(effect, Effect::ApplyProposal(_))),
            "apply work must remain deferred after the probe shell read finishes because planning still runs first"
        );
        with_scheduled_effect_queue(|queue| {
            assert!(
                if has_more_items {
                    matches!(
                        queue.items.front(),
                        Some(ScheduledWorkItem::EffectBatch(effects))
                            if effects
                                .iter()
                                .any(|effect| matches!(
                                    effect,
                                    Effect::RequestRenderPlan(_) | Effect::ApplyProposal(_)
                                ))
                    )
                } else {
                    queue.items.is_empty()
                },
                "probe completion should either queue planning/apply work or finish without extra shell work"
            );
        });

        set_core_state(CoreState::default());
        reset_scheduled_effect_queue();
    }

    #[test]
    fn multi_probe_observation_runs_cursor_and_background_chunks_on_separate_edges() {
        let _guard = core_dispatch_test_guard();
        reset_scheduled_effect_queue();

        let mut runtime = ready_state().runtime().clone();
        runtime.config.cursor_color = Some("none".to_string());
        runtime.config.particles_enabled = true;
        runtime.config.particles_over_text = false;
        set_core_state(ready_state().with_runtime(runtime));

        dispatch_core_event(
            CoreEvent::ExternalDemandQueued(ExternalDemandQueuedEvent {
                kind: ExternalDemandKind::ExternalCursor,
                observed_at: Millis::new(25),
                requested_target: None,
                ingress_cursor_presentation: None,
            }),
            &mut |effects| {
                let should_schedule =
                    with_scheduled_effect_queue(|queue| queue.stage_batch(effects));
                assert!(should_schedule, "initial ingress work should arm the queue");
            },
        )
        .expect("ingress dispatch should queue observation work");

        let request = core_state()
            .active_observation_request()
            .cloned()
            .expect("active observation request");
        let basis = observation_basis(&request, Some(cursor(7, 8)), 26);
        let mut executor = RecordingExecutor::default();
        executor
            .planned_follow_ups
            .push_back(vec![CoreEvent::ObservationBaseCollected(
                ObservationBaseCollectedEvent {
                    request: request.clone(),
                    basis: basis.clone(),
                    motion: ObservationMotion::default(),
                },
            )]);

        let has_more_items = drain_scheduled_work_with_executor(&mut executor)
            .expect("observation base collection should run on the first edge");

        assert!(has_more_items, "first edge should queue the first probe");
        assert_eq!(executor.executed_effects.len(), 1);
        assert!(matches!(
            executor.executed_effects.as_slice(),
            [Effect::RequestObservationBase(_)]
        ));
        let after_base = core_state();
        assert_eq!(after_base.lifecycle(), Lifecycle::Observing);
        assert!(after_base.pending_proposal().is_none());
        with_scheduled_effect_queue(|queue| {
            assert_eq!(
                queue.items.len(),
                1,
                "only one probe batch should be queued"
            );
            assert!(matches!(
                queue.items.front(),
                Some(ScheduledWorkItem::EffectBatch(effects))
                    if matches!(
                        effects.as_slice(),
                        [Effect::RequestProbe(payload)] if payload.kind == ProbeKind::CursorColor
                    )
            ));
        });

        executor
            .planned_follow_ups
            .push_back(vec![compatible_probe_report(&request)]);

        let has_more_items = drain_scheduled_work_with_executor(&mut executor)
            .expect("cursor color probe should run on its own edge");

        assert!(
            has_more_items,
            "cursor probe should queue background probing next"
        );
        assert_eq!(executor.executed_effects.len(), 2);
        assert!(matches!(
            executor.executed_effects[1],
            Effect::RequestProbe(ref payload) if payload.kind == ProbeKind::CursorColor
        ));
        let after_cursor = core_state();
        assert_eq!(after_cursor.lifecycle(), Lifecycle::Observing);
        assert!(after_cursor.pending_proposal().is_none());
        let first_background_chunk = after_cursor
            .observation()
            .and_then(|observation| observation.background_progress())
            .and_then(crate::core::state::BackgroundProbeProgress::next_chunk)
            .expect("first background chunk");
        with_scheduled_effect_queue(|queue| {
            assert_eq!(
                queue.items.len(),
                1,
                "background probing should wait for a later edge"
            );
            assert!(matches!(
                queue.items.front(),
                Some(ScheduledWorkItem::EffectBatch(effects))
                    if matches!(
                        effects.as_slice(),
                        [Effect::RequestProbe(payload)]
                            if payload.kind == ProbeKind::Background
                                && payload.background_chunk == Some(first_background_chunk)
                    )
            ));
        });

        executor
            .planned_follow_ups
            .push_back(vec![background_chunk_probe_report(
                &request,
                first_background_chunk,
                basis.viewport(),
            )]);

        let has_more_items = drain_scheduled_work_with_executor(&mut executor)
            .expect("first background chunk should run on its own edge");

        assert!(
            has_more_items,
            "background chunk completion should queue the next chunk"
        );
        assert_eq!(executor.executed_effects.len(), 3);
        assert!(matches!(
            executor.executed_effects[2],
            Effect::RequestProbe(ref payload)
                if payload.kind == ProbeKind::Background
                    && payload.background_chunk == Some(first_background_chunk)
        ));
        let after_background = core_state();
        assert_eq!(after_background.lifecycle(), Lifecycle::Observing);
        assert!(after_background.pending_proposal().is_none());
        let second_background_chunk = after_background
            .observation()
            .and_then(|observation| observation.background_progress())
            .and_then(crate::core::state::BackgroundProbeProgress::next_chunk)
            .expect("second background chunk");
        with_scheduled_effect_queue(|queue| {
            assert_eq!(
                queue.items.len(),
                1,
                "the next background chunk should remain queued"
            );
            assert!(matches!(
                queue.items.front(),
                Some(ScheduledWorkItem::EffectBatch(effects))
                    if matches!(
                        effects.as_slice(),
                        [Effect::RequestProbe(payload)]
                            if payload.kind == ProbeKind::Background
                                && payload.background_chunk == Some(second_background_chunk)
                    )
            ));
        });

        executor
            .planned_follow_ups
            .push_back(vec![background_probe_report(&request, basis.viewport())]);

        let has_more_items = drain_scheduled_work_with_executor(&mut executor)
            .expect("final background completion should queue planning");

        assert!(
            has_more_items,
            "planning work should remain deferred to a later edge"
        );
        assert_eq!(executor.executed_effects.len(), 4);
        assert!(matches!(
            executor.executed_effects[3],
            Effect::RequestProbe(ref payload)
                if payload.kind == ProbeKind::Background
                    && payload.background_chunk == Some(second_background_chunk)
        ));
        let after_completion = core_state();
        assert_eq!(after_completion.lifecycle(), Lifecycle::Planning);
        assert!(after_completion.pending_proposal().is_none());
        assert!(after_completion.pending_plan_proposal_id().is_some());
        with_scheduled_effect_queue(|queue| {
            assert_eq!(queue.items.len(), 1, "planning work should remain queued");
            assert!(matches!(
                queue.items.front(),
                Some(ScheduledWorkItem::EffectBatch(effects))
                    if effects
                        .iter()
                        .any(|effect| matches!(effect, Effect::RequestRenderPlan(_)))
            ));
        });

        set_core_state(CoreState::default());
        reset_scheduled_effect_queue();
    }
}
