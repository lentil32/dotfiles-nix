use crate::core::effect::Effect;
use crate::core::effect::IngressCursorPresentationEffect;
use crate::core::effect::RenderCleanupExecution;
use crate::core::event::ApplyReport;
use crate::core::event::Event as CoreEvent;
use crate::core::event::ProbeReportedEvent;
use crate::core::event::RenderPlanComputedEvent;
use crate::core::event::RenderPlanFailedEvent;
use crate::core::runtime_reducer::CursorVisibilityEffect;
use crate::core::runtime_reducer::RenderCleanupAction;
use crate::core::runtime_reducer::RenderSideEffects;
use crate::core::runtime_reducer::TargetCellPresentation;
use crate::core::runtime_reducer::render_cleanup_idle_target_budget;
use crate::core::runtime_reducer::render_cleanup_max_prune_per_tick;
use crate::core::state::AnimationSchedule;
use crate::core::state::ApplyFailureKind;
use crate::core::state::CoreState;
use crate::core::state::DemandQueue;
use crate::core::state::ExternalDemand;
use crate::core::state::ExternalDemandKind;
use crate::core::state::InFlightProposal;
use crate::core::state::ObservationBasis;
use crate::core::state::ObservationSnapshot;
use crate::core::state::PatchBasis;
use crate::core::state::PendingObservation;
use crate::core::state::PlannedRender;
use crate::core::state::ProbeKind;
use crate::core::state::ProjectionHandle;
use crate::core::state::ProtocolPhaseKind;
use crate::core::state::QueuedDemand;
use crate::core::state::RealizationDivergence;
use crate::core::state::RealizationLedger;
use crate::core::state::RealizationPlan;
use crate::core::state::RenderCleanupState;
use crate::core::state::RenderThermalState;
use crate::core::state::ScenePatch;
use crate::core::state::ScenePatchKind;
use crate::core::types::Millis;
use crate::core::types::ObservationId;
use crate::core::types::TimerToken;
use crate::position::CursorObservation;
use crate::position::ObservedCell;
use crate::position::ScreenCell;
use crate::position::ViewportBounds;
use crate::position::WindowSurfaceSnapshot;
use crate::state::RuntimeState;
use crate::state::TrackedCursor;

pub(super) fn timer_token_summary(token: TimerToken) -> String {
    format!("{}@{}", token.id().name(), token.generation().value())
}

fn millis_summary(millis: Millis) -> u64 {
    millis.value()
}

fn optional_millis_summary(millis: Option<Millis>) -> String {
    millis.map_or_else(
        || "none".to_string(),
        |millis| millis_summary(millis).to_string(),
    )
}

fn cursor_position_summary(position: Option<ScreenCell>) -> String {
    position.map_or_else(
        || "none".to_string(),
        |position| format!("{}:{}", position.row(), position.col()),
    )
}

fn observed_cell_summary(cell: ObservedCell) -> String {
    match cell {
        ObservedCell::Unavailable => "unavailable".to_string(),
        ObservedCell::Exact(cell) => format!("exact({})", cursor_position_summary(Some(cell))),
        ObservedCell::Deferred(cell) => {
            format!("deferred({})", cursor_position_summary(Some(cell)))
        }
    }
}

fn surface_summary(surface: WindowSurfaceSnapshot) -> String {
    format!(
        "id=(win={} buf={}) top={} left={} textoff={} origin={} size={}",
        surface.id().window_handle(),
        surface.id().buffer_handle(),
        surface.top_buffer_line().value(),
        surface.left_col0(),
        surface.text_offset0(),
        cursor_position_summary(Some(surface.window_origin())),
        viewport_summary(surface.window_size()),
    )
}

fn runtime_target_tracked_cursor_summary(tracked_cursor: &TrackedCursor) -> String {
    let surface = format!("({})", surface_summary(tracked_cursor.surface()));
    let buffer_line = tracked_cursor.buffer_line().value().to_string();
    format!("surface={surface} buffer_line={buffer_line}")
}

fn runtime_target_summary(runtime: &RuntimeState) -> String {
    format!(
        "cell={} epoch={} tracked={}",
        cursor_position_summary(ScreenCell::from_rounded_point(runtime.target_position())),
        runtime.retarget_epoch(),
        runtime.tracked_cursor_ref().map_or_else(
            || "none".to_string(),
            |tracked_cursor| {
                format!(
                    "({})",
                    runtime_target_tracked_cursor_summary(tracked_cursor)
                )
            }
        ),
    )
}

fn cursor_observation_summary(surface: WindowSurfaceSnapshot, cursor: CursorObservation) -> String {
    format!(
        "surface=({}) cursor=(line={} cell={})",
        surface_summary(surface),
        cursor.buffer_line().value(),
        observed_cell_summary(cursor.cell()),
    )
}

fn viewport_summary(viewport: ViewportBounds) -> String {
    format!("{}x{}", viewport.max_row(), viewport.max_col())
}

fn external_demand_kind_name(kind: ExternalDemandKind) -> &'static str {
    match kind {
        ExternalDemandKind::ExternalCursor => "external_cursor",
        ExternalDemandKind::ModeChanged => "mode_changed",
        ExternalDemandKind::BufferEntered => "buffer_entered",
        ExternalDemandKind::BoundaryRefresh => "boundary_refresh",
    }
}

fn buffer_perf_class_name(class: crate::core::state::BufferPerfClass) -> &'static str {
    class.diagnostic_name()
}

fn external_demand_summary(demand: &ExternalDemand) -> String {
    format!(
        "kind={} perf_class={} seq={} observed_at={}",
        external_demand_kind_name(demand.kind()),
        buffer_perf_class_name(demand.buffer_perf_class()),
        demand.seq().value(),
        millis_summary(demand.observed_at()),
    )
}

fn queued_demand_summary(demand: &QueuedDemand) -> String {
    format!("ready({})", external_demand_summary(demand.as_demand()))
}

fn demand_queue_summary(queue: &DemandQueue) -> String {
    let cursor = queue
        .latest_cursor()
        .map_or_else(|| "none".to_string(), queued_demand_summary);
    format!("cursor={} ordered={}", cursor, queue.ordered().len())
}

fn pending_observation_summary(pending: &PendingObservation) -> String {
    let probes = pending.requested_probes();
    format!(
        "obs={} demand=({}) probes=cursor_color:{} background:{}",
        pending.observation_id().value(),
        external_demand_summary(pending.demand()),
        probes.cursor_color(),
        probes.background(),
    )
}

fn protocol_phase_kind_name(kind: ProtocolPhaseKind) -> &'static str {
    match kind {
        ProtocolPhaseKind::Idle => "idle",
        ProtocolPhaseKind::Primed => "primed",
        ProtocolPhaseKind::Collecting => "collecting",
        ProtocolPhaseKind::Observing => "observing",
        ProtocolPhaseKind::Ready => "ready",
        ProtocolPhaseKind::Planning => "planning",
        ProtocolPhaseKind::Applying => "applying",
        ProtocolPhaseKind::Recovering => "recovering",
    }
}

fn observation_id_summary(observation_id: ObservationId) -> String {
    format!("obs={}", observation_id.value())
}

fn active_observation_summary(observation: &ObservationSnapshot) -> String {
    format!(
        "obs={} demand=({}) probes=cursor_color:{} background:{}",
        observation.observation_id().value(),
        external_demand_summary(observation.demand()),
        observation.probes().cursor_color().is_requested(),
        observation.probes().background().is_requested(),
    )
}

fn observation_basis_summary(basis: &ObservationBasis) -> String {
    let buffer_revision = basis
        .buffer_revision()
        .map_or_else(|| "none".to_string(), |revision| revision.to_string());
    format!(
        "observed_at={} mode={} {} viewport={} buffer_revision={}",
        millis_summary(basis.observed_at()),
        basis.mode(),
        cursor_observation_summary(basis.surface(), basis.cursor()),
        viewport_summary(basis.viewport()),
        buffer_revision,
    )
}

fn projection_summary(projection: &ProjectionHandle) -> String {
    let witness = projection.witness();
    format!(
        "render_rev=(motion={} semantics={}) obs={} projector_rev={} viewport={}",
        witness.render_revision().motion().value(),
        witness.render_revision().semantics().value(),
        witness.observation_id().value(),
        witness.projector_revision().value(),
        viewport_summary(witness.viewport()),
    )
}

fn patch_basis_summary(basis: &PatchBasis) -> String {
    let acknowledged = basis
        .acknowledged_handle()
        .map_or_else(|| "none".to_string(), projection_summary);
    let target = basis
        .target_handle()
        .map_or_else(|| "none".to_string(), projection_summary);
    format!("ack={acknowledged} target={target}")
}

fn scene_patch_kind_name(kind: ScenePatchKind) -> &'static str {
    match kind {
        ScenePatchKind::Noop => "noop",
        ScenePatchKind::Clear => "clear",
        ScenePatchKind::Replace => "replace",
    }
}

pub(super) fn scene_patch_summary(patch: &ScenePatch) -> String {
    format!(
        "kind={} basis=({})",
        scene_patch_kind_name(patch.kind()),
        patch_basis_summary(patch.basis()),
    )
}

fn render_cleanup_state_summary(
    cleanup: RenderCleanupState,
    config: &crate::config::RuntimeConfig,
) -> String {
    match cleanup.thermal() {
        RenderThermalState::Hot => format!(
            "hot(next_compaction_due_at={} hard_purge_due_at={} max_kept_windows={} idle_target_budget={} max_prune_per_tick={})",
            optional_millis_summary(cleanup.next_compaction_due_at()),
            optional_millis_summary(cleanup.hard_purge_due_at()),
            config.max_kept_windows,
            render_cleanup_idle_target_budget(config),
            render_cleanup_max_prune_per_tick(config),
        ),
        RenderThermalState::Cooling => format!(
            "cooling(entered_cooling_at={} hard_purge_due_at={} max_kept_windows={} idle_target_budget={} max_prune_per_tick={})",
            optional_millis_summary(cleanup.entered_cooling_at()),
            optional_millis_summary(cleanup.hard_purge_due_at()),
            config.max_kept_windows,
            render_cleanup_idle_target_budget(config),
            render_cleanup_max_prune_per_tick(config),
        ),
        RenderThermalState::Cold => format!(
            "cold(idle_target_budget={} max_prune_per_tick={})",
            render_cleanup_idle_target_budget(config),
            render_cleanup_max_prune_per_tick(config),
        ),
    }
}

fn timer_state_summary(state: crate::core::state::TimerState) -> String {
    let tokens = state
        .active_tokens()
        .map(timer_token_summary)
        .collect::<Vec<_>>();

    if tokens.is_empty() {
        "none".to_string()
    } else {
        tokens.join(",")
    }
}

fn realization_divergence_summary(divergence: RealizationDivergence) -> String {
    match divergence {
        RealizationDivergence::ApplyMetrics(metrics) => format!(
            "apply_metrics(planned={} applied={} skipped_capacity={} reuse_missing_window={} reuse_reconfigure={} reuse_missing_buffer={} recovered={})",
            metrics.planned_ops(),
            metrics.applied_ops(),
            metrics.skipped_ops_capacity(),
            metrics.reuse_failed_missing_window(),
            metrics.reuse_failed_reconfigure(),
            metrics.reuse_failed_missing_buffer(),
            metrics.windows_recovered(),
        ),
        RealizationDivergence::ShellStateUnknown => "shell_state_unknown".to_string(),
    }
}

fn realization_ledger_summary(ledger: &RealizationLedger) -> String {
    match ledger {
        RealizationLedger::Cleared => "cleared".to_string(),
        RealizationLedger::Consistent { acknowledged } => {
            format!("consistent(ack={})", projection_summary(acknowledged))
        }
        RealizationLedger::Diverged {
            last_consistent,
            divergence,
        } => format!(
            "diverged(last_consistent={} divergence={})",
            last_consistent
                .as_ref()
                .map_or_else(|| "none".to_string(), projection_summary),
            realization_divergence_summary(*divergence),
        ),
    }
}

fn apply_failure_kind_name(kind: ApplyFailureKind) -> &'static str {
    match kind {
        ApplyFailureKind::MissingProjection => "missing_projection",
        ApplyFailureKind::MissingRequiredProbe => "missing_required_probe",
        ApplyFailureKind::ShellError => "shell_error",
        ApplyFailureKind::ViewportDrift => "viewport_drift",
    }
}

fn render_cleanup_action_name(action: RenderCleanupAction) -> &'static str {
    match action {
        RenderCleanupAction::NoAction => "none",
        RenderCleanupAction::Schedule => "schedule",
        RenderCleanupAction::Invalidate => "invalidate",
    }
}

fn target_cell_presentation_name(effect: TargetCellPresentation) -> &'static str {
    match effect {
        TargetCellPresentation::None => "none",
        TargetCellPresentation::OverlayCursorCell(crate::types::CursorCellShape::Block) => {
            "overlay_block_cell"
        }
        TargetCellPresentation::OverlayCursorCell(crate::types::CursorCellShape::VerticalBar) => {
            "overlay_vertical_bar_cell"
        }
        TargetCellPresentation::OverlayCursorCell(crate::types::CursorCellShape::HorizontalBar) => {
            "overlay_horizontal_bar_cell"
        }
    }
}

fn cursor_visibility_effect_name(effect: CursorVisibilityEffect) -> &'static str {
    match effect {
        CursorVisibilityEffect::Keep => "keep",
        CursorVisibilityEffect::Hide => "hide",
        CursorVisibilityEffect::Show => "show",
    }
}

pub(super) fn render_side_effects_summary(side_effects: RenderSideEffects) -> String {
    format!(
        "redraw_draw_cmdline={} redraw_clear_cmdline={} target_cell={} cursor_visibility={} allow_real_cursor_updates={}",
        side_effects.redraw_after_draw_if_cmdline,
        side_effects.redraw_after_clear_if_cmdline,
        target_cell_presentation_name(side_effects.target_cell_presentation),
        cursor_visibility_effect_name(side_effects.cursor_visibility),
        side_effects.allow_real_cursor_updates,
    )
}

fn animation_schedule_summary(schedule: AnimationSchedule) -> String {
    match schedule {
        AnimationSchedule::Idle => "idle".to_string(),
        AnimationSchedule::DefaultDelay => "default_delay".to_string(),
        AnimationSchedule::Deadline(deadline) => format!("deadline({})", deadline.value()),
    }
}

pub(super) fn realization_plan_summary(realization: &RealizationPlan) -> String {
    match realization {
        RealizationPlan::Draw(draw) => format!(
            "draw(max_kept_windows={} allocation_policy={:?})",
            draw.max_kept_windows(),
            draw.allocation_policy(),
        ),
        RealizationPlan::Clear(clear) => {
            format!("clear(max_kept_windows={})", clear.max_kept_windows())
        }
        RealizationPlan::Noop => "noop".to_string(),
        RealizationPlan::Failure(failure) => format!(
            "failure(reason={} divergence={})",
            apply_failure_kind_name(failure.reason()),
            realization_divergence_summary(failure.divergence()),
        ),
    }
}

pub(super) fn proposal_summary(proposal: &InFlightProposal) -> String {
    let realization = proposal.realization();
    format!(
        "proposal_id={} realization={} patch={} cleanup={} animation_schedule={} side_effects=({})",
        proposal.proposal_id().value(),
        realization_plan_summary(&realization),
        scene_patch_summary(proposal.patch()),
        render_cleanup_action_name(proposal.cleanup_action()),
        animation_schedule_summary(proposal.animation_schedule()),
        render_side_effects_summary(proposal.side_effects()),
    )
}

fn planned_render_summary(planned_render: &PlannedRender) -> String {
    let render_revision = planned_render.scene_update().render_revision();
    format!(
        "proposal=({}) render_revision=(motion={} semantics={})",
        proposal_summary(planned_render.proposal()),
        render_revision.motion().value(),
        render_revision.semantics().value(),
    )
}

pub(super) fn render_cleanup_execution_summary(execution: RenderCleanupExecution) -> String {
    match execution {
        RenderCleanupExecution::SoftClear { max_kept_windows } => {
            format!("soft_clear(max_kept_windows={max_kept_windows})")
        }
        RenderCleanupExecution::CompactToBudget {
            target_budget,
            max_prune_per_tick,
        } => format!(
            "compact_to_budget(target_budget={target_budget} max_prune_per_tick={max_prune_per_tick})"
        ),
        RenderCleanupExecution::HardPurge => "hard_purge".to_string(),
    }
}

pub(super) fn apply_report_summary(report: &ApplyReport) -> String {
    match report {
        ApplyReport::AppliedFully {
            proposal_id,
            observed_at,
            visual_change,
        } => format!(
            "applied_fully(proposal_id={} observed_at={} visual_change={})",
            proposal_id.value(),
            millis_summary(*observed_at),
            visual_change,
        ),
        ApplyReport::AppliedDegraded {
            proposal_id,
            divergence,
            observed_at,
            visual_change,
        } => format!(
            "applied_degraded(proposal_id={} observed_at={} visual_change={} divergence={})",
            proposal_id.value(),
            millis_summary(*observed_at),
            visual_change,
            realization_divergence_summary(*divergence),
        ),
        ApplyReport::ApplyFailed {
            proposal_id,
            reason,
            divergence,
            observed_at,
        } => format!(
            "apply_failed(proposal_id={} observed_at={} reason={} divergence={})",
            proposal_id.value(),
            millis_summary(*observed_at),
            apply_failure_kind_name(*reason),
            realization_divergence_summary(*divergence),
        ),
    }
}

pub(super) fn core_state_summary(state: &CoreState) -> String {
    let observation = match (
        state.pending_observation(),
        state.observation(),
        state.retained_observation(),
    ) {
        (Some(pending), None, retained) => format!(
            "pending=({}) retained={}",
            pending_observation_summary(pending),
            retained.map_or_else(
                || "none".to_string(),
                |observation| format!(
                    "({} basis=({}))",
                    active_observation_summary(observation),
                    observation_basis_summary(observation.basis()),
                ),
            ),
        ),
        (None, Some(observation), None) => format!(
            "active=({} basis=({}))",
            active_observation_summary(observation),
            observation_basis_summary(observation.basis()),
        ),
        (None, None, Some(observation)) => format!(
            "retained=({} basis=({}))",
            active_observation_summary(observation),
            observation_basis_summary(observation.basis()),
        ),
        (None, None, None) => "none".to_string(),
        (pending, active, retained) => format!(
            "invalid_slots(pending={} active={} retained={})",
            pending.is_some(),
            active.is_some(),
            retained.is_some(),
        ),
    };
    let proposal = state
        .pending_proposal()
        .map_or_else(|| "none".to_string(), proposal_summary);
    format!(
        "lifecycle={:?} phase={} cleanup={} exact_anchor={} runtime_target=({}) timers={} queue={} observation={} proposal={} realization={}",
        state.lifecycle(),
        protocol_phase_kind_name(state.phase_kind()),
        render_cleanup_state_summary(state.render_cleanup(), &state.runtime().config),
        cursor_position_summary(state.latest_exact_cursor_cell()),
        runtime_target_summary(state.runtime()),
        timer_state_summary(state.timers()),
        demand_queue_summary(state.demand_queue()),
        observation,
        proposal,
        realization_ledger_summary(state.realization()),
    )
}

fn probe_report_summary(payload: &ProbeReportedEvent) -> String {
    match payload {
        ProbeReportedEvent::CursorColorReady {
            observation_id,
            reuse,
            sample,
        } => format!(
            "cursor_color_ready(obs={} request={} reuse={reuse:?} sample_present={})",
            observation_id.value(),
            ProbeKind::CursorColor.request_id(*observation_id).value(),
            sample.is_some(),
        ),
        ProbeReportedEvent::CursorColorFailed {
            observation_id,
            failure,
        } => format!(
            "cursor_color_failed(obs={} request={} failure={failure:?})",
            observation_id.value(),
            ProbeKind::CursorColor.request_id(*observation_id).value(),
        ),
        ProbeReportedEvent::BackgroundReady {
            observation_id,
            reuse,
            batch,
        } => format!(
            "background_ready(obs={} request={} reuse={reuse:?} cells={})",
            observation_id.value(),
            ProbeKind::Background.request_id(*observation_id).value(),
            batch.allowed_mask_len(),
        ),
        ProbeReportedEvent::BackgroundChunkReady {
            observation_id,
            chunk,
            allowed_mask,
        } => format!(
            "background_chunk_ready(obs={} request={} start_index={} cell_count={} packed_bytes={})",
            observation_id.value(),
            ProbeKind::Background.request_id(*observation_id).value(),
            chunk.start_index(),
            chunk.len(),
            allowed_mask.packed_len(),
        ),
        ProbeReportedEvent::BackgroundFailed {
            observation_id,
            failure,
        } => format!(
            "background_failed(obs={} request={} failure={failure:?})",
            observation_id.value(),
            ProbeKind::Background.request_id(*observation_id).value(),
        ),
    }
}

pub(super) fn core_event_summary(event: &CoreEvent) -> String {
    match event {
        CoreEvent::Initialize(payload) => {
            format!("observed_at={}", millis_summary(payload.observed_at))
        }
        CoreEvent::ExternalDemandQueued(payload) => format!(
            "kind={} observed_at={} ingress_cursor_presentation={:?}",
            external_demand_kind_name(payload.kind),
            millis_summary(payload.observed_at),
            payload.ingress_cursor_presentation,
        ),
        CoreEvent::ObservationBaseCollected(payload) => format!(
            "{} basis=({}) scroll_shift={:?}",
            observation_id_summary(payload.observation_id),
            observation_basis_summary(&payload.basis),
            payload.motion.scroll_shift(),
        ),
        CoreEvent::ProbeReported(payload) => probe_report_summary(payload),
        CoreEvent::RenderPlanComputed(RenderPlanComputedEvent {
            planned_render,
            observed_at,
        }) => format!(
            "proposal_id={} observed_at={} {}",
            planned_render.proposal_id().value(),
            millis_summary(*observed_at),
            planned_render_summary(planned_render),
        ),
        CoreEvent::RenderPlanFailed(RenderPlanFailedEvent {
            proposal_id,
            observed_at,
        }) => format!(
            "proposal_id={} observed_at={}",
            proposal_id.value(),
            millis_summary(*observed_at),
        ),
        CoreEvent::ApplyReported(payload) => apply_report_summary(payload),
        CoreEvent::RenderCleanupApplied(payload) => format!(
            "observed_at={} action={:?}",
            millis_summary(payload.observed_at),
            payload.action,
        ),
        CoreEvent::TimerFiredWithToken(payload) => format!(
            "kind={} token={} observed_at={}",
            payload.token.id().name(),
            timer_token_summary(payload.token),
            millis_summary(payload.observed_at),
        ),
        CoreEvent::TimerLostWithToken(payload) => format!(
            "kind={} token={} observed_at={}",
            payload.token.id().name(),
            timer_token_summary(payload.token),
            millis_summary(payload.observed_at),
        ),
        CoreEvent::EffectFailed(payload) => format!(
            "proposal_id={} observed_at={}",
            payload.proposal_id.map_or_else(
                || "none".to_string(),
                |proposal_id| proposal_id.value().to_string()
            ),
            millis_summary(payload.observed_at),
        ),
    }
}

pub(super) fn effect_summary(effect: &Effect) -> String {
    match effect {
        Effect::ScheduleTimer(payload) => format!(
            "kind={} token={} delay_ms={} requested_at={}",
            payload.token.id().name(),
            timer_token_summary(payload.token),
            payload.delay.value(),
            millis_summary(payload.requested_at),
        ),
        Effect::RequestObservationBase(payload) => {
            format!(
                "request=({})",
                pending_observation_summary(&payload.request)
            )
        }
        Effect::RequestProbe(payload) => format!(
            "obs={} request={} kind={:?} smear_to_cmd={} chunk={:?} cursor_color_generations={:?}",
            payload.observation_id.value(),
            payload.probe_request_id().value(),
            payload.kind,
            payload.cursor_position_policy.smear_to_cmd(),
            payload.background_chunk,
            payload.cursor_color_probe_generations,
        ),
        Effect::RequestRenderPlan(payload) => {
            let observation_id = payload
                .planning
                .observation()
                .map(|observation| observation.observation_id().value().to_string())
                .unwrap_or_else(|| "none".to_string());
            format!(
                "proposal_id={} observation_id={} requested_at={} animation_schedule={}",
                payload.proposal_id.value(),
                observation_id,
                millis_summary(payload.requested_at),
                animation_schedule_summary(payload.animation_schedule),
            )
        }
        Effect::ApplyProposal(payload) => {
            format!(
                "requested_at={} {}",
                millis_summary(payload.requested_at),
                proposal_summary(&payload.proposal),
            )
        }
        Effect::ApplyRenderCleanup(payload) => render_cleanup_execution_summary(payload.execution),
        Effect::ApplyIngressCursorPresentation(payload) => match payload {
            IngressCursorPresentationEffect::HideCursor => "hide_cursor".to_string(),
            IngressCursorPresentationEffect::HideCursorAndPrepaint {
                cell,
                shape,
                zindex,
            } => format!(
                "hide_cursor_and_prepaint(row={} col={} shape={shape:?} zindex={zindex})",
                cell.row(),
                cell.col(),
            ),
        },
        Effect::RecordEventLoopMetric(metric) => format!("{metric:?}"),
        Effect::RedrawCmdline => "cmdline_redraw".to_string(),
    }
}

#[cfg(test)]
mod tests;
