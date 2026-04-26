use super::EffectExecutor;
use super::ScheduledWorkItem;
use super::ScheduledWorkUnit;
use super::dispatch_core_event;
use super::drain_scheduled_work_with_executor;
use super::reset_scheduled_effect_queue;
use super::reset_scheduled_queue_after_failure;
use super::scheduled_drain_budget;
use super::scheduled_drain_budget_for_depth;
use super::scheduled_drain_budget_for_hot_effect_only_snapshot;
use super::scheduled_drain_budget_for_thermal;
use super::with_dispatch_queue;
use crate::core::effect::Effect;
use crate::core::effect::OrderedEffect;
use crate::core::event::Event as CoreEvent;
use crate::core::event::ExternalDemandQueuedEvent;
use crate::core::event::ObservationBaseCollectedEvent;
use crate::core::event::ProbeReportedEvent;
use crate::core::reducer::reduce as reduce_core_event;
use crate::core::state::BackgroundProbeBatch;
use crate::core::state::BackgroundProbeChunk;
use crate::core::state::BackgroundProbeChunkMask;
use crate::core::state::BackgroundProbePlan;
use crate::core::state::BufferPerfClass;
use crate::core::state::CoreState;
use crate::core::state::CursorColorSample;
use crate::core::state::ExternalDemandKind;
use crate::core::state::ObservationBasis;
use crate::core::state::ObservationMotion;
use crate::core::state::PendingObservation;
use crate::core::state::ProbeKind;
use crate::core::state::ProbeReuse;
use crate::core::types::Lifecycle;
use crate::core::types::Millis;
use crate::events::runtime::core_state;
use crate::events::runtime::set_core_state;
use crate::mutex::lock_with_poison_recovery;
use crate::position::BufferLine;
use crate::position::CursorObservation;
use crate::position::ObservedCell;
use crate::position::ScreenCell;
use crate::position::SurfaceId;
use crate::position::ViewportBounds;
use crate::position::WindowSurfaceSnapshot;
use crate::test_support::cursor;
use crate::test_support::sparse_probe_cells;
use nvim_oxi::Result;
use std::collections::VecDeque;
use std::sync::LazyLock;
use std::sync::Mutex;

mod deferred_multi_probe_observation;
mod dispatch_core_event;
mod refresh_required_probe_retry;
mod scheduled_effect_coalescing;
mod scheduled_effect_drain;
mod scheduled_effect_drain_support;
mod single_cursor_probe_observation;

static CORE_DISPATCH_TEST_MUTEX: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

fn core_dispatch_test_guard() -> std::sync::MutexGuard<'static, ()> {
    lock_with_poison_recovery(&CORE_DISPATCH_TEST_MUTEX, |_| (), |_| {})
}

#[derive(Default)]
struct RecordingExecutor {
    executed_effects: Vec<Effect>,
    planned_follow_ups: VecDeque<Vec<CoreEvent>>,
}

impl EffectExecutor for RecordingExecutor {
    fn execute_effect(&mut self, effect: Effect) -> Result<Vec<CoreEvent>> {
        self.executed_effects.push(effect);
        Ok(self.planned_follow_ups.pop_front().unwrap_or_default())
    }
}

fn ready_state() -> CoreState {
    let mut runtime = crate::state::RuntimeState::default();
    runtime.config.delay_event_to_smear = 0.0;
    CoreState::default().with_runtime(runtime).into_primed()
}

fn observation_basis(position: Option<ScreenCell>, observed_at: u64) -> ObservationBasis {
    ObservationBasis::new(
        Millis::new(observed_at),
        "n".to_string(),
        WindowSurfaceSnapshot::new(
            SurfaceId::new(11, 22).expect("positive handles"),
            BufferLine::new(3).expect("positive top buffer line"),
            0,
            0,
            ScreenCell::new(1, 1).expect("one-based window origin"),
            ViewportBounds::new(40, 120).expect("positive window size"),
        ),
        CursorObservation::new(
            BufferLine::new(4).expect("positive buffer line"),
            position.map_or(ObservedCell::Unavailable, ObservedCell::Exact),
        ),
        ViewportBounds::new(40, 120).expect("positive viewport bounds"),
    )
    .with_buffer_revision(Some(0))
}

fn refresh_required_probe_report(request: &PendingObservation) -> CoreEvent {
    CoreEvent::ProbeReported(ProbeReportedEvent::CursorColorReady {
        observation_id: request.observation_id(),
        reuse: ProbeReuse::RefreshRequired,
        sample: Some(CursorColorSample::new(0x00AB_CDEF)),
    })
}

fn compatible_probe_report(request: &PendingObservation) -> CoreEvent {
    CoreEvent::ProbeReported(ProbeReportedEvent::CursorColorReady {
        observation_id: request.observation_id(),
        reuse: ProbeReuse::Compatible,
        sample: Some(CursorColorSample::new(0x00AB_CDEF)),
    })
}

fn background_probe_report(request: &PendingObservation, _viewport: ViewportBounds) -> CoreEvent {
    CoreEvent::ProbeReported(ProbeReportedEvent::BackgroundReady {
        observation_id: request.observation_id(),
        reuse: ProbeReuse::Exact,
        batch: BackgroundProbeBatch::empty(),
    })
}

fn background_chunk_probe_report(
    request: &PendingObservation,
    chunk: &BackgroundProbeChunk,
    _viewport: ViewportBounds,
) -> CoreEvent {
    let allowed_mask = vec![false; chunk.len()];
    CoreEvent::ProbeReported(ProbeReportedEvent::BackgroundChunkReady {
        observation_id: request.observation_id(),
        chunk: chunk.clone(),
        allowed_mask: BackgroundProbeChunkMask::from_allowed_mask(&allowed_mask),
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

    fn dispatch_external_cursor_ingress_to_queue(&self, observed_at: u64) -> PendingObservation {
        dispatch_core_event(external_cursor_demand(observed_at), &mut |effects| {
            // CONTEXT: `stage_batch` reports whether this enqueue operation also needs to arm
            // the drain edge; it does not signal whether the batch was accepted.
            let should_schedule =
                with_dispatch_queue(|queue| queue.stage_batch(effects).should_schedule);
            assert!(
                should_schedule,
                "ingress dispatch should arm exactly one scheduled work item"
            );
        })
        .expect("ingress dispatch should commit reducer state");

        current_core_state()
            .pending_observation()
            .cloned()
            .expect("ingress dispatch should leave an active pending observation")
    }

    fn observing_state_after_base_collection(&self) -> (PendingObservation, CoreState) {
        self.set_core_state(ready_state_with_cursor_color_probe());
        let observing = reduce_core_event(&current_core_state(), external_cursor_demand(25)).next;
        let request = observing
            .pending_observation()
            .cloned()
            .expect("active pending observation");
        let based = reduce_core_event(
            &observing,
            observation_base_collected(&request, observation_basis(Some(cursor(7, 8)), 26)),
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
        buffer_perf_class: BufferPerfClass::Full,
        ingress_cursor_presentation: None,
        ingress_observation_surface: None,
    })
}

fn observation_base_collected(request: &PendingObservation, basis: ObservationBasis) -> CoreEvent {
    CoreEvent::ObservationBaseCollected(ObservationBaseCollectedEvent {
        observation_id: request.observation_id(),
        basis,
        cursor_color_probe_generations: request.requested_probes().cursor_color().then_some(
            crate::core::state::CursorColorProbeGenerations::new(
                crate::core::types::Generation::INITIAL,
                crate::core::types::Generation::INITIAL,
            ),
        ),
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

fn install_background_probe_plan(basis: &ObservationBasis) {
    let mut next = current_core_state().with_latest_exact_cursor_cell(Some(cursor(7, 8)));
    let Some(active_observation) = next.observation_mut() else {
        panic!("active observation state");
    };
    *active_observation.probes_mut().background_mut() =
        crate::core::state::BackgroundProbeState::from_plan(BackgroundProbePlan::from_cells(
            sparse_probe_cells(basis.viewport(), 2050),
        ));
    replace_core_state(next);
}

fn queue_stage_batch(effects: Vec<Effect>) -> bool {
    with_dispatch_queue(|queue| queue.stage_batch(effects).should_schedule)
}

fn queued_work_count() -> usize {
    with_dispatch_queue(|queue| queue.pending_work_units)
}

fn queued_item_capacity() -> usize {
    with_dispatch_queue(|queue| queue.items.capacity())
}

fn queued_front_work_item() -> Option<ScheduledWorkUnit> {
    with_dispatch_queue(|queue| match queue.items.front()? {
        ScheduledWorkItem::OrderedEffectBatch(effects) => {
            Some(ScheduledWorkUnit::OrderedEffectBatch(effects.clone()))
        }
        ScheduledWorkItem::CoreEvent(event) => Some(ScheduledWorkUnit::CoreEvent(event.clone())),
        ScheduledWorkItem::ShellOnlyAgenda(agenda) => agenda
            .steps
            .front()
            .cloned()
            .map(ScheduledWorkUnit::ShellOnlyStep),
    })
}

fn queue_contains_observation_base_request() -> bool {
    with_dispatch_queue(|queue| {
        queue.items.iter().any(|item| match item {
            ScheduledWorkItem::OrderedEffectBatch(effects) => {
                contains_observation_base_request(effects)
            }
            ScheduledWorkItem::CoreEvent(_) | ScheduledWorkItem::ShellOnlyAgenda(_) => false,
        })
    })
}

fn queue_is_marked_scheduled() -> bool {
    with_dispatch_queue(|queue| queue.drain_scheduled)
}

fn drain_next_edge(executor: &mut RecordingExecutor) -> bool {
    drain_scheduled_work_with_executor(executor)
        .expect("scheduled drain should execute one queued edge")
}

fn contains_observation_base_request(effects: &[OrderedEffect]) -> bool {
    effects
        .iter()
        .any(|effect| matches!(effect, OrderedEffect::RequestObservationBase(_)))
}

fn contains_probe_request(effects: &[OrderedEffect]) -> bool {
    effects
        .iter()
        .any(|effect| matches!(effect, OrderedEffect::RequestProbe(_)))
}

fn contains_render_plan_request(effects: &[OrderedEffect]) -> bool {
    effects
        .iter()
        .any(|effect| matches!(effect, OrderedEffect::RequestRenderPlan(_)))
}

fn is_apply_proposal(effect: &OrderedEffect) -> bool {
    matches!(effect, OrderedEffect::ApplyProposal(_))
}

fn only_cursor_color_probe_request(effects: &[OrderedEffect]) -> bool {
    matches!(
        effects,
        [OrderedEffect::RequestProbe(payload)] if payload.kind == ProbeKind::CursorColor
    )
}

fn only_background_probe_request_for_chunk(
    effects: &[OrderedEffect],
    expected_chunk: &BackgroundProbeChunk,
) -> bool {
    matches!(
        effects,
        [OrderedEffect::RequestProbe(payload)]
            if payload.kind == ProbeKind::Background
                && payload.background_chunk.as_ref() == Some(expected_chunk)
    )
}
