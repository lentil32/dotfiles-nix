use super::super::*;
use crate::core::effect::ObservationRuntimeContextArgs;
use crate::core::effect::RetainedCursorColorFallback;
use crate::core::effect::tracked_observation_inputs;
use crate::position::BufferLine;
use crate::position::CursorObservation;
use crate::position::ObservedCell;
use crate::position::ScreenCell;
use crate::position::SurfaceId;
use crate::position::ViewportBounds;
use crate::position::WindowSurfaceSnapshot;

pub(in crate::core::reducer::tests) fn ready_state() -> CoreState {
    let mut runtime = RuntimeState::default();
    runtime.config.delay_event_to_smear = 0.0;
    CoreState::default().with_runtime(runtime).into_primed()
}

pub(in crate::core::reducer::tests) fn ready_state_with_runtime_config(
    configure: impl FnOnce(&mut RuntimeState),
) -> CoreState {
    let ready = ready_state();
    let mut runtime = ready.runtime().clone();
    configure(&mut runtime);
    ready.with_runtime(runtime)
}

pub(in crate::core::reducer::tests) fn external_demand_event(
    kind: ExternalDemandKind,
    observed_at: u64,
) -> Event {
    external_demand_event_with_perf_class(kind, observed_at, BufferPerfClass::Full)
}

pub(in crate::core::reducer::tests) fn external_demand_event_with_perf_class(
    kind: ExternalDemandKind,
    observed_at: u64,
    buffer_perf_class: BufferPerfClass,
) -> Event {
    Event::ExternalDemandQueued(ExternalDemandQueuedEvent {
        kind,
        observed_at: Millis::new(observed_at),
        buffer_perf_class,
        ingress_cursor_presentation: None,
        ingress_observation_surface: None,
    })
}

pub(in crate::core::reducer::tests) fn observation_request(
    seq: u64,
    kind: ExternalDemandKind,
    observed_at: u64,
) -> PendingObservation {
    observation_request_with_perf_class(seq, kind, observed_at, BufferPerfClass::Full)
}

pub(in crate::core::reducer::tests) fn observation_request_with_perf_class(
    seq: u64,
    kind: ExternalDemandKind,
    observed_at: u64,
    buffer_perf_class: BufferPerfClass,
) -> PendingObservation {
    PendingObservation::new(
        ExternalDemand::new(
            IngressSeq::new(seq),
            kind,
            Millis::new(observed_at),
            buffer_perf_class,
        ),
        ProbeRequestSet::default(),
    )
}

pub(in crate::core::reducer::tests) fn observation_basis(
    position: Option<ScreenCell>,
    observed_at: u64,
) -> ObservationBasis {
    observation_basis_with_observed_cell(
        position.map_or(ObservedCell::Unavailable, ObservedCell::Exact),
        observed_at,
        "n",
    )
}

pub(in crate::core::reducer::tests) fn observation_basis_in_mode(
    position: Option<ScreenCell>,
    observed_at: u64,
    mode: &str,
) -> ObservationBasis {
    observation_basis_with_observed_cell(
        position.map_or(ObservedCell::Unavailable, ObservedCell::Exact),
        observed_at,
        mode,
    )
}

pub(in crate::core::reducer::tests) fn observation_basis_with_observed_cell(
    observed_cell: ObservedCell,
    observed_at: u64,
    mode: &str,
) -> ObservationBasis {
    ObservationBasis::new(
        Millis::new(observed_at),
        mode.to_string(),
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
            observed_cell,
        ),
        ViewportBounds::new(40, 120).expect("positive viewport bounds"),
    )
    .with_buffer_revision(Some(0))
}

pub(in crate::core::reducer::tests) fn observed_rows(rows: &[&str]) -> Vec<ObservedTextRow> {
    rows.iter()
        .map(|text| ObservedTextRow::new((*text).to_string()))
        .collect()
}

pub(in crate::core::reducer::tests) fn text_context(
    changedtick: u64,
    cursor_line: i64,
    rows: &[&str],
    tracked_rows: Option<&[&str]>,
) -> CursorTextContext {
    CursorTextContext::new(
        22,
        changedtick,
        cursor_line,
        observed_rows(rows),
        tracked_rows.map(observed_rows),
    )
}

pub(in crate::core::reducer::tests) fn observation_basis_with_text_context(
    position: Option<ScreenCell>,
    observed_at: u64,
    cursor_line: i64,
    changedtick: u64,
    rows: &[&str],
    tracked_rows: Option<&[&str]>,
) -> ObservationBasis {
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
            BufferLine::new(cursor_line).expect("positive buffer line"),
            position.map_or(ObservedCell::Unavailable, ObservedCell::Exact),
        ),
        ViewportBounds::new(40, 120).expect("positive viewport bounds"),
    )
    .with_buffer_revision(Some(changedtick))
    .with_cursor_text_context_state(CursorTextContextState::Sampled(text_context(
        changedtick,
        cursor_line,
        rows,
        tracked_rows,
    )))
}

pub(in crate::core::reducer::tests) fn observation_basis_with_text_context_boundary(
    position: Option<ScreenCell>,
    observed_at: u64,
    cursor_line: i64,
    boundary: crate::core::state::CursorTextContextBoundary,
) -> ObservationBasis {
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
            BufferLine::new(cursor_line).expect("positive buffer line"),
            position.map_or(ObservedCell::Unavailable, ObservedCell::Exact),
        ),
        ViewportBounds::new(40, 120).expect("positive viewport bounds"),
    )
    .with_buffer_revision(Some(0))
    .with_cursor_text_context_state(CursorTextContextState::BoundaryOnly(boundary))
}

pub(in crate::core::reducer::tests) fn cursor_color_probe_generations()
-> crate::core::state::CursorColorProbeGenerations {
    crate::core::state::CursorColorProbeGenerations::new(
        crate::core::types::Generation::INITIAL,
        crate::core::types::Generation::INITIAL,
    )
}

pub(in crate::core::reducer::tests) fn observation_motion() -> ObservationMotion {
    ObservationMotion::default()
}

pub(in crate::core::reducer::tests) fn observation_snapshot(
    position: ScreenCell,
) -> ObservationSnapshot {
    let request = observation_request(9, ExternalDemandKind::ExternalCursor, 90);
    let basis = observation_basis(Some(position), 91);
    ObservationSnapshot::new(request, basis, observation_motion())
}

pub(in crate::core::reducer::tests) fn observation_snapshot_with_cursor_color(
    position: ScreenCell,
    color: u32,
) -> ObservationSnapshot {
    observation_snapshot_with_cursor_color_reuse(position, color, ProbeReuse::Exact)
}

pub(in crate::core::reducer::tests) fn observation_snapshot_with_cursor_color_reuse(
    position: ScreenCell,
    color: u32,
    reuse: ProbeReuse,
) -> ObservationSnapshot {
    let request = PendingObservation::new(
        ExternalDemand::new(
            IngressSeq::new(9),
            ExternalDemandKind::ExternalCursor,
            Millis::new(90),
            BufferPerfClass::Full,
        ),
        ProbeRequestSet::only(ProbeKind::CursorColor),
    );
    let basis = observation_basis(Some(position), 91);
    let mut observation = ObservationSnapshot::new(request, basis, observation_motion())
        .with_cursor_color_probe_generations(Some(cursor_color_probe_generations()));
    assert!(
        observation
            .probes_mut()
            .set_cursor_color_state(ProbeState::ready(
                reuse,
                Some(CursorColorSample::new(color)),
            )),
        "cursor color probe should be requested",
    );
    observation
}

pub(in crate::core::reducer::tests) fn primed_state_with_ready_observation(
    state: CoreState,
    observation: ObservationSnapshot,
) -> CoreState {
    state
        .with_ready_observation(observation)
        .expect("primed state should accept ready observation fixtures")
}

pub(in crate::core::reducer::tests) fn observing_state_from_demand(
    ready: &CoreState,
    kind: ExternalDemandKind,
    observed_at: u64,
) -> CoreState {
    reduce(ready, external_demand_event(kind, observed_at)).next
}

pub(in crate::core::reducer::tests) fn active_request(state: &CoreState) -> PendingObservation {
    state
        .pending_observation()
        .cloned()
        .expect("collecting phase should own the pending observation")
}

pub(in crate::core::reducer::tests) fn collect_observation_base(
    state: &CoreState,
    request: &PendingObservation,
    basis: ObservationBasis,
    motion: ObservationMotion,
) -> Transition {
    let requested_cursor_color = request.requested_probes().cursor_color().then_some(());
    let cursor_color_probe_generations = requested_cursor_color.and_then(|()| {
        state
            .runtime()
            .config
            .requires_cursor_color_sampling_for_mode(basis.mode())
            .then_some(cursor_color_probe_generations())
    });
    reduce(
        state,
        Event::ObservationBaseCollected(ObservationBaseCollectedEvent {
            observation_id: request.observation_id(),
            basis,
            cursor_color_probe_generations,
            motion,
        }),
    )
}

pub(in crate::core::reducer::tests) fn cursor_color_probe_ready_state() -> CoreState {
    ready_state_with_runtime_config(|runtime| {
        runtime.config.cursor_color = Some("none".to_string());
    })
}

pub(in crate::core::reducer::tests) fn background_probe_ready_state() -> CoreState {
    ready_state_with_runtime_config(|runtime| {
        runtime.config.particles_enabled = true;
        runtime.config.particles_over_text = false;
    })
}

pub(in crate::core::reducer::tests) fn dual_probe_ready_state() -> CoreState {
    ready_state_with_runtime_config(|runtime| {
        runtime.config.cursor_color = Some("none".to_string());
        runtime.config.particles_enabled = true;
        runtime.config.particles_over_text = false;
    })
}

pub(in crate::core::reducer::tests) fn cursor_position_policy(
    state: &CoreState,
) -> CursorPositionReadPolicy {
    CursorPositionReadPolicy::new(state.runtime().config.smear_to_cmd)
}

pub(in crate::core::reducer::tests) fn observation_cursor_color_fallback(
    state: &CoreState,
) -> Option<CursorColorFallback> {
    let observation = state.phase_observation()?;
    let sample = observation.cursor_color().map(CursorColorSample::new)?;
    let witness = observation.cursor_color_probe_witness()?;
    Some(CursorColorFallback::new(sample, witness))
}

pub(in crate::core::reducer::tests) fn expected_probe_policy(
    demand_kind: ExternalDemandKind,
    buffer_perf_class: BufferPerfClass,
    cursor_color_fallback: Option<&CursorColorFallback>,
) -> ProbePolicy {
    let retained_cursor_color_fallback = match cursor_color_fallback {
        Some(_) => RetainedCursorColorFallback::CompatibleSample,
        None => RetainedCursorColorFallback::Unavailable,
    };
    ProbePolicy::for_demand(
        demand_kind,
        buffer_perf_class,
        retained_cursor_color_fallback,
    )
}

pub(in crate::core::reducer::tests) fn compatible_cursor_color_ready_state(
    configure_runtime: impl FnOnce(&mut RuntimeState),
) -> CoreState {
    let mut runtime = ready_state().runtime().clone();
    runtime.config.cursor_color = Some("none".to_string());
    runtime.initialize_cursor(
        RenderPoint { row: 9.0, col: 9.0 },
        CursorShape::block(),
        7,
        &TrackedCursor::fixture(11, 22, 3, 9),
    );
    configure_runtime(&mut runtime);
    ready_state()
        .with_latest_exact_cursor_cell(Some(cursor(9, 9)))
        .with_runtime(runtime)
        .with_ready_observation(observation_snapshot_with_cursor_color_reuse(
            cursor(9, 9),
            0x00AB_CDEF,
            ProbeReuse::Compatible,
        ))
        .expect("primed state should accept a compatible ready observation")
}

pub(in crate::core::reducer::tests) fn conceal_deferred_cursor_ready_state(
    configure_runtime: impl FnOnce(&mut RuntimeState),
) -> CoreState {
    let mut runtime = ready_state().runtime().clone();
    runtime.initialize_cursor(
        RenderPoint { row: 9.0, col: 9.0 },
        CursorShape::block(),
        7,
        &TrackedCursor::fixture(11, 22, 3, 9),
    );
    configure_runtime(&mut runtime);

    let request = observation_request_with_perf_class(
        9,
        ExternalDemandKind::ExternalCursor,
        90,
        BufferPerfClass::FastMotion,
    );
    let basis = observation_basis_with_observed_cell(ObservedCell::Deferred(cursor(9, 9)), 91, "n");
    let observation = ObservationSnapshot::new(request, basis, observation_motion());

    ready_state()
        .with_latest_exact_cursor_cell(Some(cursor(9, 9)))
        .with_runtime(runtime)
        .with_ready_observation(observation)
        .expect("primed state should accept a conceal-deferred ready observation")
}

pub(in crate::core::reducer::tests) fn observation_runtime_context(
    state: &CoreState,
    demand_kind: ExternalDemandKind,
) -> ObservationRuntimeContext {
    observation_runtime_context_with_perf_class(state, demand_kind, BufferPerfClass::Full)
}

pub(in crate::core::reducer::tests) fn observation_runtime_context_with_perf_class(
    state: &CoreState,
    demand_kind: ExternalDemandKind,
    buffer_perf_class: BufferPerfClass,
) -> ObservationRuntimeContext {
    let cursor_color_fallback = observation_cursor_color_fallback(state);
    let cursor_text_context_boundary = state
        .phase_observation()
        .and_then(|observation| observation.basis().cursor_text_context_boundary());
    let (tracked_surface, tracked_buffer_position) =
        tracked_observation_inputs(state.runtime().tracked_cursor_ref());
    ObservationRuntimeContext::new(ObservationRuntimeContextArgs {
        cursor_position_policy: cursor_position_policy(state),
        scroll_buffer_space: state.runtime().config.scroll_buffer_space,
        tracked_surface,
        tracked_buffer_position,
        cursor_text_context_boundary,
        current_corners: state.runtime().current_corners(),
        ingress_observation_surface: None,
        buffer_perf_class,
        probe_policy: expected_probe_policy(
            demand_kind,
            buffer_perf_class,
            cursor_color_fallback.as_ref(),
        ),
    })
}

pub(in crate::core::reducer::tests) fn cursor_color_probe_report(
    request: &PendingObservation,
    reuse: ProbeReuse,
    color: Option<u32>,
) -> Event {
    Event::ProbeReported(ProbeReportedEvent::CursorColorReady {
        observation_id: request.observation_id(),
        reuse,
        sample: color.map(CursorColorSample::new),
    })
}

pub(in crate::core::reducer::tests) fn cursor_color_probe_failed(
    request: &PendingObservation,
) -> Event {
    Event::ProbeReported(ProbeReportedEvent::CursorColorFailed {
        observation_id: request.observation_id(),
        failure: ProbeFailure::ShellReadFailed,
    })
}

pub(in crate::core::reducer::tests) fn background_probe_batch(
    viewport: ViewportBounds,
    allowed_cells: &[(u32, u32)],
) -> BackgroundProbeBatch {
    let width = usize::try_from(viewport.max_col()).expect("viewport width");
    let height = usize::try_from(viewport.max_row()).expect("viewport height");
    let mut allowed_mask = vec![false; width * height];
    for &(row, col) in allowed_cells {
        let row_index = usize::try_from(row.saturating_sub(1)).expect("row index");
        let col_index = usize::try_from(col.saturating_sub(1)).expect("col index");
        let index = row_index * width + col_index;
        allowed_mask[index] = true;
    }

    BackgroundProbeBatch::from_allowed_mask(viewport, allowed_mask)
}

pub(in crate::core::reducer::tests) fn background_probe_report(
    request: &PendingObservation,
    viewport: ViewportBounds,
    allowed_cells: &[(u32, u32)],
    reuse: ProbeReuse,
) -> Event {
    Event::ProbeReported(ProbeReportedEvent::BackgroundReady {
        observation_id: request.observation_id(),
        reuse,
        batch: background_probe_batch(viewport, allowed_cells),
    })
}

pub(in crate::core::reducer::tests) fn background_chunk_probe_report(
    request: &PendingObservation,
    chunk: &BackgroundProbeChunk,
    _viewport: ViewportBounds,
    allowed_cells: &[(u32, u32)],
) -> Event {
    let allowed_mask = chunk
        .iter_cells()
        .map(|cell| {
            let Ok(row) = u32::try_from(cell.row()) else {
                return false;
            };
            let Ok(col) = u32::try_from(cell.col()) else {
                return false;
            };
            allowed_cells.contains(&(row, col))
        })
        .collect::<Vec<_>>();

    Event::ProbeReported(ProbeReportedEvent::BackgroundChunkReady {
        observation_id: request.observation_id(),
        chunk: chunk.clone(),
        allowed_mask: BackgroundProbeChunkMask::from_allowed_mask(&allowed_mask),
    })
}

pub(in crate::core::reducer::tests) fn ready_state_with_observation(
    position: ScreenCell,
) -> CoreState {
    primed_state_with_ready_observation(
        ready_state().with_latest_exact_cursor_cell(Some(position)),
        observation_snapshot(position),
    )
}

pub(in crate::core::reducer::tests) fn recovering_state_with_observation(
    position: ScreenCell,
) -> CoreState {
    ready_state_with_observation(position).enter_recovering()
}
