use super::super::*;

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
    requested_target: Option<CursorPosition>,
) -> Event {
    external_demand_event_with_perf_class(
        kind,
        observed_at,
        requested_target,
        BufferPerfClass::Full,
    )
}

pub(in crate::core::reducer::tests) fn external_demand_event_with_perf_class(
    kind: ExternalDemandKind,
    observed_at: u64,
    requested_target: Option<CursorPosition>,
    buffer_perf_class: BufferPerfClass,
) -> Event {
    Event::ExternalDemandQueued(ExternalDemandQueuedEvent {
        kind,
        observed_at: Millis::new(observed_at),
        requested_target,
        buffer_perf_class,
        ingress_cursor_presentation: None,
    })
}

pub(in crate::core::reducer::tests) fn observation_request(
    seq: u64,
    kind: ExternalDemandKind,
    observed_at: u64,
) -> ObservationRequest {
    observation_request_with_perf_class(seq, kind, observed_at, BufferPerfClass::Full)
}

pub(in crate::core::reducer::tests) fn observation_request_with_perf_class(
    seq: u64,
    kind: ExternalDemandKind,
    observed_at: u64,
    buffer_perf_class: BufferPerfClass,
) -> ObservationRequest {
    ObservationRequest::new(
        ExternalDemand::new(
            IngressSeq::new(seq),
            kind,
            Millis::new(observed_at),
            None,
            buffer_perf_class,
        ),
        ProbeRequestSet::default(),
    )
}

pub(in crate::core::reducer::tests) fn observation_basis(
    request: &ObservationRequest,
    position: Option<CursorPosition>,
    observed_at: u64,
) -> ObservationBasis {
    observation_basis_in_mode(request, position, observed_at, "n")
}

pub(in crate::core::reducer::tests) fn observation_basis_in_mode(
    request: &ObservationRequest,
    position: Option<CursorPosition>,
    observed_at: u64,
    mode: &str,
) -> ObservationBasis {
    ObservationBasis::new(
        request.observation_id(),
        Millis::new(observed_at),
        mode.to_string(),
        position,
        CursorLocation::new(11, 22, 3, 4),
        ViewportSnapshot::new(CursorRow(40), CursorCol(120)),
    )
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
    request: &ObservationRequest,
    position: Option<CursorPosition>,
    observed_at: u64,
    cursor_line: i64,
    changedtick: u64,
    rows: &[&str],
    tracked_rows: Option<&[&str]>,
) -> ObservationBasis {
    ObservationBasis::new(
        request.observation_id(),
        Millis::new(observed_at),
        "n".to_string(),
        position,
        CursorLocation::new(11, 22, 3, cursor_line),
        ViewportSnapshot::new(CursorRow(40), CursorCol(120)),
    )
    .with_cursor_text_context(Some(text_context(
        changedtick,
        cursor_line,
        rows,
        tracked_rows,
    )))
}

pub(in crate::core::reducer::tests) fn observation_motion() -> ObservationMotion {
    ObservationMotion::default()
}

pub(in crate::core::reducer::tests) fn observation_snapshot(
    position: CursorPosition,
) -> ObservationSnapshot {
    let request = observation_request(9, ExternalDemandKind::ExternalCursor, 90);
    let basis = observation_basis(&request, Some(position), 91);
    ObservationSnapshot::new(request, basis, observation_motion())
}

pub(in crate::core::reducer::tests) fn observation_snapshot_with_cursor_color(
    position: CursorPosition,
    color: u32,
) -> ObservationSnapshot {
    observation_snapshot_with_cursor_color_reuse(position, color, ProbeReuse::Exact)
}

pub(in crate::core::reducer::tests) fn observation_snapshot_with_cursor_color_reuse(
    position: CursorPosition,
    color: u32,
    reuse: ProbeReuse,
) -> ObservationSnapshot {
    let request = ObservationRequest::new(
        ExternalDemand::new(
            IngressSeq::new(9),
            ExternalDemandKind::ExternalCursor,
            Millis::new(90),
            None,
            BufferPerfClass::Full,
        ),
        ProbeRequestSet::new(true, false),
    );
    let basis = observation_basis(&request, Some(position), 91).with_cursor_color_witness(Some(
        cursor_color_probe_witness(11, 22, 0, "n", Some(position), 0),
    ));
    ObservationSnapshot::new(request.clone(), basis, observation_motion())
        .with_cursor_color_probe(ProbeState::ready(
            ProbeKind::CursorColor.request_id(request.observation_id()),
            request.observation_id(),
            reuse,
            Some(CursorColorSample::new(color)),
        ))
        .expect("cursor color probe should be requested")
}

pub(in crate::core::reducer::tests) fn observing_state_from_demand(
    ready: &CoreState,
    kind: ExternalDemandKind,
    observed_at: u64,
    requested_target: Option<CursorPosition>,
) -> CoreState {
    reduce(
        ready,
        external_demand_event(kind, observed_at, requested_target),
    )
    .next
}

pub(in crate::core::reducer::tests) fn active_request(state: &CoreState) -> ObservationRequest {
    state
        .active_observation_request()
        .cloned()
        .expect("active observation request")
}

pub(in crate::core::reducer::tests) fn collect_observation_base(
    state: &CoreState,
    request: &ObservationRequest,
    basis: ObservationBasis,
    motion: ObservationMotion,
) -> Transition {
    reduce(
        state,
        Event::ObservationBaseCollected(ObservationBaseCollectedEvent {
            request: request.clone(),
            basis,
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

pub(in crate::core::reducer::tests) fn retained_cursor_color_fallback(
    state: &CoreState,
) -> Option<CursorColorFallback> {
    let observation = state.retained_observation()?;
    let sample = observation.cursor_color().map(CursorColorSample::new)?;
    let witness = observation.basis().cursor_color_witness()?.clone();
    Some(CursorColorFallback::new(sample, witness))
}

pub(in crate::core::reducer::tests) fn expected_probe_policy(
    demand_kind: ExternalDemandKind,
    buffer_perf_class: BufferPerfClass,
    cursor_color_fallback: Option<&CursorColorFallback>,
) -> ProbePolicy {
    ProbePolicy::for_demand(
        demand_kind,
        buffer_perf_class,
        cursor_color_fallback.is_some(),
    )
}

pub(in crate::core::reducer::tests) fn compatible_cursor_color_ready_state(
    configure_runtime: impl FnOnce(&mut RuntimeState),
) -> CoreState {
    let mut runtime = ready_state().runtime().clone();
    runtime.config.cursor_color = Some("none".to_string());
    runtime.initialize_cursor(
        Point { row: 9.0, col: 9.0 },
        CursorShape::new(false, false),
        7,
        &CursorLocation::new(11, 22, 3, 9),
    );
    configure_runtime(&mut runtime);
    ready_state()
        .with_last_cursor(Some(cursor(9, 9)))
        .with_runtime(runtime)
        .into_ready_with_observation(observation_snapshot_with_cursor_color_reuse(
            cursor(9, 9),
            0x00AB_CDEF,
            ProbeReuse::Compatible,
        ))
}

pub(in crate::core::reducer::tests) fn conceal_deferred_cursor_ready_state(
    configure_runtime: impl FnOnce(&mut RuntimeState),
) -> CoreState {
    let mut runtime = ready_state().runtime().clone();
    runtime.initialize_cursor(
        Point { row: 9.0, col: 9.0 },
        CursorShape::new(false, false),
        7,
        &CursorLocation::new(11, 22, 3, 9),
    );
    configure_runtime(&mut runtime);

    let request = observation_request_with_perf_class(
        9,
        ExternalDemandKind::ExternalCursor,
        90,
        BufferPerfClass::FastMotion,
    );
    let basis = observation_basis(&request, Some(cursor(9, 9)), 91);
    let observation = ObservationSnapshot::new(
        request,
        basis,
        observation_motion().with_cursor_position_sync(CursorPositionSync::ConcealDeferred),
    );

    ready_state()
        .with_last_cursor(Some(cursor(9, 9)))
        .with_runtime(runtime)
        .into_ready_with_observation(observation)
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
    let cursor_color_fallback = retained_cursor_color_fallback(state);
    ObservationRuntimeContext::new(
        cursor_position_policy(state),
        state.runtime().config.scroll_buffer_space,
        state.runtime().tracked_location(),
        state.runtime().current_corners(),
        buffer_perf_class,
        expected_probe_policy(
            demand_kind,
            buffer_perf_class,
            cursor_color_fallback.as_ref(),
        ),
    )
}

pub(in crate::core::reducer::tests) fn cursor_color_probe_report(
    request: &ObservationRequest,
    reuse: ProbeReuse,
    color: Option<u32>,
) -> Event {
    Event::ProbeReported(ProbeReportedEvent::CursorColorReady {
        observation_id: request.observation_id(),
        probe_request_id: ProbeKind::CursorColor.request_id(request.observation_id()),
        reuse,
        sample: color.map(CursorColorSample::new),
    })
}

pub(in crate::core::reducer::tests) fn cursor_color_probe_failed(
    request: &ObservationRequest,
) -> Event {
    Event::ProbeReported(ProbeReportedEvent::CursorColorFailed {
        observation_id: request.observation_id(),
        probe_request_id: ProbeKind::CursorColor.request_id(request.observation_id()),
        failure: ProbeFailure::ShellReadFailed,
    })
}

pub(in crate::core::reducer::tests) fn background_probe_batch(
    viewport: ViewportSnapshot,
    allowed_cells: &[(u32, u32)],
) -> BackgroundProbeBatch {
    let width = usize::try_from(viewport.max_col.value()).expect("viewport width");
    let height = usize::try_from(viewport.max_row.value()).expect("viewport height");
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
    request: &ObservationRequest,
    viewport: ViewportSnapshot,
    allowed_cells: &[(u32, u32)],
    reuse: ProbeReuse,
) -> Event {
    Event::ProbeReported(ProbeReportedEvent::BackgroundReady {
        observation_id: request.observation_id(),
        probe_request_id: ProbeKind::Background.request_id(request.observation_id()),
        reuse,
        batch: background_probe_batch(viewport, allowed_cells),
    })
}

pub(in crate::core::reducer::tests) fn background_chunk_probe_report(
    request: &ObservationRequest,
    chunk: &BackgroundProbeChunk,
    _viewport: ViewportSnapshot,
    allowed_cells: &[(u32, u32)],
) -> Event {
    let allowed_mask = chunk
        .cells()
        .iter()
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
        probe_request_id: ProbeKind::Background.request_id(request.observation_id()),
        chunk: chunk.clone(),
        allowed_mask: BackgroundProbeChunkMask::from_allowed_mask(&allowed_mask),
    })
}

pub(in crate::core::reducer::tests) fn ready_state_with_observation(
    position: CursorPosition,
) -> CoreState {
    ready_state()
        .with_last_cursor(Some(position))
        .into_ready_with_observation(observation_snapshot(position))
}

pub(in crate::core::reducer::tests) fn recovering_state_with_observation(
    position: CursorPosition,
) -> CoreState {
    ready_state_with_observation(position).into_recovering()
}
