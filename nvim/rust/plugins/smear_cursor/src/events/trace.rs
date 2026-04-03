use crate::core::effect::{
    Effect, IngressCursorPresentationEffect, RenderCleanupExecution, TimerKind,
};
use crate::core::event::{
    ApplyReport, Event as CoreEvent, ProbeReportedEvent, RenderPlanComputedEvent,
    RenderPlanFailedEvent,
};
use crate::core::runtime_reducer::{
    CursorVisibilityEffect, RenderCleanupAction, RenderSideEffects, TargetCellPresentation,
};
use crate::core::state::{
    AnimationSchedule, ApplyFailureKind, CoreState, DemandQueue, ExternalDemand,
    ExternalDemandKind, InFlightProposal, ObservationBasis, ObservationRequest, PatchBasis,
    PlannedRender, ProjectionSnapshot, QueuedDemand, RealizationDivergence, RealizationLedger,
    RealizationPlan, RenderCleanupState, RenderThermalState, ScenePatch, ScenePatchKind,
};
use crate::core::types::{CursorPosition, Millis, TimerId, TimerToken, ViewportSnapshot};
use crate::state::CursorLocation;

pub(super) fn timer_kind_name(kind: TimerKind) -> &'static str {
    match kind {
        TimerKind::Animation => "animation",
        TimerKind::Ingress => "ingress",
        TimerKind::Recovery => "recovery",
        TimerKind::Cleanup => "cleanup",
    }
}

fn timer_id_name(timer_id: TimerId) -> &'static str {
    match timer_id {
        TimerId::Animation => "animation",
        TimerId::Ingress => "ingress",
        TimerId::Recovery => "recovery",
        TimerId::Cleanup => "cleanup",
    }
}

pub(super) fn timer_token_summary(token: TimerToken) -> String {
    format!(
        "{}@{}",
        timer_id_name(token.id()),
        token.generation().value()
    )
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

fn cursor_position_summary(position: Option<CursorPosition>) -> String {
    position.map_or_else(
        || "none".to_string(),
        |position| format!("{}:{}", position.row.value(), position.col.value()),
    )
}

fn cursor_location_summary(location: &CursorLocation) -> String {
    format!(
        "win={} buf={} top={} line={}",
        location.window_handle, location.buffer_handle, location.top_row, location.line
    )
}

fn viewport_summary(viewport: ViewportSnapshot) -> String {
    format!("{}x{}", viewport.max_row.value(), viewport.max_col.value())
}

fn external_demand_kind_name(kind: ExternalDemandKind) -> &'static str {
    match kind {
        ExternalDemandKind::ExternalCursor => "external_cursor",
        ExternalDemandKind::ModeChanged => "mode_changed",
        ExternalDemandKind::BufferEntered => "buffer_entered",
    }
}

fn external_demand_summary(demand: &ExternalDemand) -> String {
    format!(
        "kind={} seq={} observed_at={} target={}",
        external_demand_kind_name(demand.kind()),
        demand.seq().value(),
        millis_summary(demand.observed_at()),
        cursor_position_summary(demand.requested_target()),
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

fn observation_request_summary(request: &ObservationRequest) -> String {
    let probes = request.probes();
    format!(
        "obs={} demand=({}) probes=cursor_color:{} background:{}",
        request.observation_id().value(),
        external_demand_summary(request.demand()),
        probes.cursor_color(),
        probes.background(),
    )
}

fn observation_basis_summary(basis: &ObservationBasis) -> String {
    let cursor_color_witness = basis.cursor_color_witness().map_or_else(
        || "none".to_string(),
        |witness| {
            format!(
                "buf={} tick={} mode={} cursor={} colorscheme_gen={}",
                witness.buffer_handle(),
                witness.changedtick(),
                witness.mode(),
                cursor_position_summary(witness.cursor_position()),
                witness.colorscheme_generation().value(),
            )
        },
    );
    format!(
        "obs={} observed_at={} mode={} cursor={} location=({}) viewport={} cursor_color_witness={}",
        basis.observation_id().value(),
        millis_summary(basis.observed_at()),
        basis.mode(),
        cursor_position_summary(basis.cursor_position()),
        cursor_location_summary(&basis.cursor_location()),
        viewport_summary(basis.viewport()),
        cursor_color_witness,
    )
}

fn projection_summary(snapshot: &ProjectionSnapshot) -> String {
    let witness = snapshot.witness();
    format!(
        "scene_rev={} obs={} projector_rev={} viewport={}",
        witness.scene_revision().value(),
        witness.observation_id().value(),
        witness.projector_revision().value(),
        viewport_summary(witness.viewport()),
    )
}

fn patch_basis_summary(basis: &PatchBasis) -> String {
    let acknowledged = basis
        .acknowledged()
        .map_or_else(|| "none".to_string(), projection_summary);
    let target = basis
        .target()
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

fn render_cleanup_state_summary(cleanup: RenderCleanupState) -> String {
    match cleanup.thermal() {
        RenderThermalState::Hot => format!(
            "hot(next_compaction_due_at={} hard_purge_due_at={} max_kept_windows={} idle_target_budget={} max_prune_per_tick={})",
            optional_millis_summary(cleanup.next_compaction_due_at()),
            optional_millis_summary(cleanup.hard_purge_due_at()),
            cleanup.max_kept_windows(),
            cleanup.idle_target_budget(),
            cleanup.max_prune_per_tick(),
        ),
        RenderThermalState::Cooling => format!(
            "cooling(entered_cooling_at={} hard_purge_due_at={} max_kept_windows={} idle_target_budget={} max_prune_per_tick={})",
            optional_millis_summary(cleanup.entered_cooling_at()),
            optional_millis_summary(cleanup.hard_purge_due_at()),
            cleanup.max_kept_windows(),
            cleanup.idle_target_budget(),
            cleanup.max_prune_per_tick(),
        ),
        RenderThermalState::Cold => format!(
            "cold(idle_target_budget={} max_prune_per_tick={})",
            cleanup.idle_target_budget(),
            cleanup.max_prune_per_tick(),
        ),
    }
}

fn timer_state_summary(state: crate::core::state::TimerState) -> String {
    let tokens = [
        state.active_token(TimerId::Animation),
        state.active_token(TimerId::Ingress),
        state.active_token(TimerId::Recovery),
        state.active_token(TimerId::Cleanup),
    ]
    .into_iter()
    .flatten()
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
        TargetCellPresentation::OverlayBlockCell => "overlay_block_cell",
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
    format!(
        "proposal=({}) scene_revision={}",
        proposal_summary(planned_render.proposal()),
        planned_render.next_scene().revision().value(),
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
    let observation = state.observation().map_or_else(
        || {
            state
                .active_observation_request()
                .map_or_else(|| "none".to_string(), observation_request_summary)
        },
        |observation| {
            format!(
                "{} basis=({})",
                observation_request_summary(observation.request()),
                observation_basis_summary(observation.basis()),
            )
        },
    );
    let proposal = state
        .pending_proposal()
        .map_or_else(|| "none".to_string(), proposal_summary);
    format!(
        "lifecycle={:?} cleanup={} timers={} queue={} observation={} proposal={} realization={}",
        state.lifecycle(),
        render_cleanup_state_summary(state.render_cleanup()),
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
            probe_request_id,
            reuse,
            sample,
        } => format!(
            "cursor_color_ready(obs={} request={} reuse={reuse:?} sample_present={})",
            observation_id.value(),
            probe_request_id.value(),
            sample.is_some(),
        ),
        ProbeReportedEvent::CursorColorFailed {
            observation_id,
            probe_request_id,
            failure,
        } => format!(
            "cursor_color_failed(obs={} request={} failure={failure:?})",
            observation_id.value(),
            probe_request_id.value(),
        ),
        ProbeReportedEvent::BackgroundReady {
            observation_id,
            probe_request_id,
            reuse,
            batch,
        } => format!(
            "background_ready(obs={} request={} reuse={reuse:?} cells={} viewport={})",
            observation_id.value(),
            probe_request_id.value(),
            batch.allowed_mask_len(),
            viewport_summary(batch.viewport()),
        ),
        ProbeReportedEvent::BackgroundChunkReady {
            observation_id,
            probe_request_id,
            chunk,
            allowed_mask,
        } => format!(
            "background_chunk_ready(obs={} request={} start_index={} cell_count={} packed_bytes={})",
            observation_id.value(),
            probe_request_id.value(),
            chunk.start_index(),
            chunk.len(),
            allowed_mask.packed_len(),
        ),
        ProbeReportedEvent::BackgroundFailed {
            observation_id,
            probe_request_id,
            failure,
        } => format!(
            "background_failed(obs={} request={} failure={failure:?})",
            observation_id.value(),
            probe_request_id.value(),
        ),
    }
}

pub(super) fn core_event_summary(event: &CoreEvent) -> String {
    match event {
        CoreEvent::Initialize(payload) => {
            format!("observed_at={}", millis_summary(payload.observed_at))
        }
        CoreEvent::ExternalDemandQueued(payload) => format!(
            "kind={} observed_at={} target={} ingress_cursor_presentation={:?}",
            external_demand_kind_name(payload.kind),
            millis_summary(payload.observed_at),
            cursor_position_summary(payload.requested_target),
            payload.ingress_cursor_presentation,
        ),
        CoreEvent::ObservationBaseCollected(payload) => format!(
            "request=({}) basis=({}) scroll_shift={:?}",
            observation_request_summary(&payload.request),
            observation_basis_summary(&payload.basis),
            payload.motion.scroll_shift(),
        ),
        CoreEvent::ProbeReported(payload) => probe_report_summary(payload),
        CoreEvent::RenderPlanComputed(RenderPlanComputedEvent {
            proposal_id,
            planned_render,
            observed_at,
        }) => format!(
            "proposal_id={} observed_at={} {}",
            proposal_id.value(),
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
            timer_id_name(payload.token.id()),
            timer_token_summary(payload.token),
            millis_summary(payload.observed_at),
        ),
        CoreEvent::TimerLostWithToken(payload) => format!(
            "kind={} token={} observed_at={}",
            timer_id_name(payload.token.id()),
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
            timer_id_name(payload.token.id()),
            timer_token_summary(payload.token),
            payload.delay.value(),
            millis_summary(payload.requested_at),
        ),
        Effect::RequestObservationBase(payload) => {
            format!(
                "request=({})",
                observation_request_summary(&payload.request)
            )
        }
        Effect::RequestProbe(payload) => format!(
            "obs={} request={} kind={:?} smear_to_cmd={} chunk={:?} cursor_color_witness={:?}",
            payload.observation_basis.observation_id().value(),
            payload.probe_request_id.value(),
            payload.kind,
            payload.cursor_position_policy.smear_to_cmd(),
            payload.background_chunk,
            payload.observation_basis.cursor_color_witness(),
        ),
        Effect::RequestRenderPlan(payload) => format!(
            "proposal_id={} observation_id={} requested_at={} animation_schedule={}",
            payload.proposal_id.value(),
            payload.observation.basis().observation_id().value(),
            millis_summary(payload.requested_at),
            animation_schedule_summary(payload.animation_schedule),
        ),
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
            IngressCursorPresentationEffect::HideCursorAndPrepaint { cell, zindex } => format!(
                "hide_cursor_and_prepaint(row={} col={} zindex={zindex})",
                cell.row(),
                cell.col(),
            ),
        },
        Effect::RecordEventLoopMetric(metric) => format!("{metric:?}"),
        Effect::RedrawCmdline => "cmdline_redraw".to_string(),
    }
}
