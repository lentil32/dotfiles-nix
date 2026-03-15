// Surprising: transition fingerprinting is staged in before the final trace hook so the reducer
// can keep a stable shape while we wire the last consumer.
#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "transition fingerprint scaffolding is intentionally retained ahead of trace integration"
    )
)]

use crate::core::effect::Effect;
use crate::core::realization::{LogicalRaster, PaletteSpec};
use crate::core::runtime_reducer::{
    CursorVisibilityEffect, RenderAllocationPolicy, RenderCleanupAction, RenderSideEffects,
    TargetCellPresentation,
};
use crate::core::state::{
    ApplyFailureKind, CoreState, DegradedApplyMetrics, ExternalDemand, ExternalDemandKind,
    InFlightProposal, PatchBasis, ProbeRequestSet, ProbeSet, ProbeSlot, ProbeState,
    ProjectionCacheEntry, ProjectionReuseKey, ProjectionSnapshot, ProtocolState, QueuedDemand,
    RealizationDivergence, RealizationLedger, RealizationPlan, ScenePatch, ScenePatchKind,
    SemanticEntity, SemanticEntityId,
};
use crate::core::types::{Lifecycle, TimerId, TimerToken};
use crate::draw::render_plan;
use crate::state::RuntimeState;

fn debug_fingerprint(value: &(impl std::fmt::Debug + ?Sized)) -> u64 {
    format!("{value:?}")
        .bytes()
        .fold(0_u64, |seed, byte| seed.rotate_left(5) ^ u64::from(byte))
}

fn lifecycle_fingerprint(lifecycle: Lifecycle) -> u64 {
    match lifecycle {
        Lifecycle::Idle => 0_u64,
        Lifecycle::Primed => 1_u64,
        Lifecycle::Observing => 2_u64,
        Lifecycle::Ready => 3_u64,
        Lifecycle::Planning => 4_u64,
        Lifecycle::Applying => 5_u64,
        Lifecycle::Recovering => 6_u64,
    }
}

const fn demand_kind_fingerprint(kind: ExternalDemandKind) -> u64 {
    match kind {
        ExternalDemandKind::ExternalCursor => 1_u64,
        ExternalDemandKind::ModeChanged => 2_u64,
        ExternalDemandKind::BufferEntered => 3_u64,
    }
}

const fn render_cleanup_action_fingerprint(action: RenderCleanupAction) -> u64 {
    match action {
        RenderCleanupAction::NoAction => 1_u64,
        RenderCleanupAction::Schedule => 2_u64,
        RenderCleanupAction::Invalidate => 3_u64,
    }
}

const fn render_allocation_policy_fingerprint(policy: RenderAllocationPolicy) -> u64 {
    match policy {
        RenderAllocationPolicy::ReuseOnly => 1_u64,
        RenderAllocationPolicy::BootstrapIfPoolEmpty => 2_u64,
    }
}

const fn cursor_visibility_fingerprint(effect: CursorVisibilityEffect) -> u64 {
    match effect {
        CursorVisibilityEffect::Keep => 1_u64,
        CursorVisibilityEffect::Hide => 2_u64,
        CursorVisibilityEffect::Show => 3_u64,
    }
}

const fn target_cell_presentation_fingerprint(presentation: TargetCellPresentation) -> u64 {
    match presentation {
        TargetCellPresentation::None => 1_u64,
        TargetCellPresentation::OverlayBlockCell => 2_u64,
    }
}

const fn apply_failure_kind_fingerprint(kind: ApplyFailureKind) -> u64 {
    match kind {
        ApplyFailureKind::MissingProjection => 1_u64,
        ApplyFailureKind::MissingRequiredProbe => 2_u64,
        ApplyFailureKind::ShellError => 3_u64,
        ApplyFailureKind::ViewportDrift => 4_u64,
    }
}

const fn scene_patch_kind_fingerprint(kind: ScenePatchKind) -> u64 {
    match kind {
        ScenePatchKind::Noop => 1_u64,
        ScenePatchKind::Clear => 2_u64,
        ScenePatchKind::Replace => 3_u64,
    }
}

fn demand_fingerprint(demand: &ExternalDemand) -> u64 {
    demand.seq().value()
        ^ demand_kind_fingerprint(demand.kind())
        ^ demand.observed_at().value()
        ^ demand
            .requested_target()
            .map_or(0_u64, crate::core::types::CursorPosition::fingerprint)
}

fn queued_demand_fingerprint(demand: &QueuedDemand) -> u64 {
    demand_fingerprint(demand.as_demand())
}

fn ingress_policy_fingerprint(last_cursor_autocmd_at: Option<crate::core::types::Millis>) -> u64 {
    last_cursor_autocmd_at.map_or(0_u64, crate::core::types::Millis::value)
}

fn probe_request_set_fingerprint(requests: ProbeRequestSet) -> u64 {
    (if requests.cursor_color() {
        1_u64
    } else {
        0_u64
    }) ^ (if requests.background() { 2_u64 } else { 0_u64 })
}

fn probe_state_fingerprint<T>(probe: &ProbeState<T>, value_fingerprint: impl Fn(&T) -> u64) -> u64 {
    match probe {
        ProbeState::Pending { request_id } => 1_u64 ^ request_id.value().rotate_left(5),
        ProbeState::Ready {
            request_id,
            observed_from,
            reuse,
            value,
        } => {
            2_u64
                ^ request_id.value()
                ^ observed_from.value().rotate_left(7)
                ^ reuse.fingerprint().rotate_left(13)
                ^ value_fingerprint(value).rotate_left(19)
        }
        ProbeState::Failed {
            request_id,
            failure,
        } => 3_u64 ^ request_id.value() ^ failure.fingerprint().rotate_left(11),
    }
}

fn probe_slot_fingerprint<T>(probe: &ProbeSlot<T>, value_fingerprint: impl Fn(&T) -> u64) -> u64 {
    match probe {
        ProbeSlot::Unrequested => 0_u64,
        ProbeSlot::Requested(state) => probe_state_fingerprint(state, value_fingerprint),
    }
}

fn probe_set_fingerprint(probes: &ProbeSet) -> u64 {
    let cursor_color_seed = probe_slot_fingerprint(probes.cursor_color(), |sample| {
        sample
            .as_ref()
            .map_or(0_u64, |color| debug_fingerprint(color.as_str()))
    });
    let background_seed = probe_slot_fingerprint(probes.background(), |batch| {
        let viewport = batch.viewport();
        let allowed_seed = batch
            .allowed_mask_iter()
            .enumerate()
            .filter(|(_, allowed)| *allowed)
            .map(|(index, _)| u64::try_from(index).unwrap_or(u64::MAX).rotate_left(11))
            .fold(0_u64, u64::wrapping_add);
        u64::from(viewport.max_row.value())
            ^ u64::from(viewport.max_col.value()).rotate_left(7)
            ^ allowed_seed.rotate_left(13)
    });

    cursor_color_seed ^ background_seed.rotate_left(5)
}

fn background_progress_fingerprint(
    progress: Option<&crate::core::state::BackgroundProbeProgress>,
) -> u64 {
    let Some(progress) = progress else {
        return 0_u64;
    };
    let viewport = progress.viewport();
    let row_width = usize::try_from(viewport.max_col.value()).unwrap_or(0);
    let mut next_row_index = 0usize;
    let sampled_rows_seed = progress
        .sampled_chunks()
        .flat_map(|chunk| {
            let row_count = if row_width == 0 {
                0
            } else {
                chunk.len() / row_width
            };
            let chunk_start_row = next_row_index;
            next_row_index = next_row_index.saturating_add(row_count);
            (0..row_count).map(move |row_offset| {
                let row_start = row_offset.saturating_mul(row_width);
                let row_end = row_start.saturating_add(row_width).min(chunk.len());
                let row_seed = chunk[row_start..row_end]
                    .iter()
                    .enumerate()
                    .filter(|(_, allowed)| **allowed)
                    .map(|(col, _)| u64::try_from(col).unwrap_or(u64::MAX).rotate_left(11))
                    .fold(0_u64, u64::wrapping_add);
                u64::try_from(chunk_start_row.saturating_add(row_offset))
                    .unwrap_or(u64::MAX)
                    .rotate_left(17)
                    ^ row_seed
            })
        })
        .fold(0_u64, u64::wrapping_add);
    u64::from(viewport.max_row.value())
        ^ u64::from(viewport.max_col.value()).rotate_left(7)
        ^ u64::from(progress.next_row().value()).rotate_left(13)
        ^ sampled_rows_seed.rotate_left(19)
}

fn cursor_color_witness_fingerprint(
    witness: Option<&crate::core::state::CursorColorProbeWitness>,
) -> u64 {
    let Some(witness) = witness else {
        return 0_u64;
    };

    witness.buffer_handle().unsigned_abs()
        ^ witness.changedtick().rotate_left(7)
        ^ debug_fingerprint(witness.mode()).rotate_left(13)
        ^ witness
            .cursor_position()
            .map_or(0_u64, crate::core::types::CursorPosition::fingerprint)
            .rotate_left(19)
        ^ witness.colorscheme_generation().value().rotate_left(23)
}

fn observed_text_rows_fingerprint(rows: &[crate::core::state::ObservedTextRow]) -> u64 {
    rows.iter().fold(0_u64, |seed, row| {
        seed ^ u64::from_ne_bytes(row.line().to_ne_bytes()).rotate_left(5)
            ^ debug_fingerprint(row.text()).rotate_left(11)
    })
}

fn cursor_text_context_fingerprint(context: Option<&crate::core::state::CursorTextContext>) -> u64 {
    let Some(context) = context else {
        return 0_u64;
    };

    let tracked_rows_seed = context
        .tracked_nearby_rows()
        .map_or(0_u64, observed_text_rows_fingerprint);

    u64::from_ne_bytes(context.buffer_handle().to_ne_bytes())
        ^ context.changedtick().rotate_left(7)
        ^ u64::from_ne_bytes(context.cursor_line().to_ne_bytes()).rotate_left(13)
        ^ observed_text_rows_fingerprint(context.nearby_rows()).rotate_left(17)
        ^ context
            .tracked_cursor_line()
            .map_or(0_u64, |line| u64::from_ne_bytes(line.to_ne_bytes()))
            .rotate_left(23)
        ^ tracked_rows_seed.rotate_left(29)
}

fn observation_snapshot_fingerprint(observation: &crate::core::state::ObservationSnapshot) -> u64 {
    let basis = observation.basis();
    observation.request().observation_id().value()
        ^ basis.observed_at().value()
        ^ basis
            .cursor_position()
            .map_or(0_u64, crate::core::types::CursorPosition::fingerprint)
        ^ u64::from(basis.viewport().max_row.value())
        ^ u64::from(basis.viewport().max_col.value())
        ^ cursor_color_witness_fingerprint(basis.cursor_color_witness()).rotate_left(11)
        ^ cursor_text_context_fingerprint(basis.cursor_text_context()).rotate_left(17)
        ^ probe_set_fingerprint(observation.probes()).rotate_left(23)
        ^ background_progress_fingerprint(observation.background_progress()).rotate_left(29)
}

fn semantic_entity_fingerprint(entity: &SemanticEntity) -> u64 {
    match entity {
        SemanticEntity::CursorTrail(trail) => {
            let base = debug_fingerprint(trail.geometry());
            SemanticEntityId::CursorTrail.fingerprint()
                ^ base
                ^ target_cell_presentation_fingerprint(trail.target_cell_presentation())
                    .rotate_left(7)
        }
    }
}

fn projection_snapshot_fingerprint(snapshot: &ProjectionSnapshot) -> u64 {
    let witness = snapshot.witness();
    witness.scene_revision().value()
        ^ witness.observation_id().value()
        ^ u64::from(witness.viewport().max_row.value())
        ^ u64::from(witness.viewport().max_col.value())
        ^ witness.projector_revision().value()
        ^ logical_raster_fingerprint(snapshot.logical_raster())
}

fn projection_reuse_key_fingerprint(reuse_key: &ProjectionReuseKey) -> u64 {
    reuse_key.signature().unwrap_or(0_u64)
        ^ reuse_key
            .planner_clock()
            .map_or(0_u64, |clock| debug_fingerprint(&clock))
            .rotate_left(3)
        ^ target_cell_presentation_fingerprint(reuse_key.target_cell_presentation()).rotate_left(5)
        ^ debug_fingerprint(reuse_key.policy()).rotate_left(11)
}

fn projection_cache_entry_fingerprint(entry: &ProjectionCacheEntry) -> u64 {
    projection_snapshot_fingerprint(entry.snapshot())
        ^ projection_reuse_key_fingerprint(entry.reuse_key()).rotate_left(7)
}

fn cell_op_fingerprint(cell: &render_plan::CellOp) -> u64 {
    let glyph_seed = cell
        .glyph
        .as_str()
        .bytes()
        .fold(0_u64, |seed, byte| seed ^ u64::from(byte));
    let highlight_seed = match cell.highlight {
        render_plan::HighlightRef::Normal(level) => 1_u64 ^ u64::from(level.value()),
    };
    u64::from_ne_bytes(cell.row.to_ne_bytes())
        ^ u64::from_ne_bytes(cell.col.to_ne_bytes())
        ^ u64::from(cell.zindex)
        ^ glyph_seed.rotate_left(7)
        ^ highlight_seed.rotate_left(13)
}

fn logical_raster_fingerprint(raster: &LogicalRaster) -> u64 {
    let clear_seed = raster.clear().map_or(0_u64, |clear| {
        u64::try_from(clear.max_kept_windows).unwrap_or(u64::MAX)
    });
    let cell_seed = raster
        .cells()
        .iter()
        .map(cell_op_fingerprint)
        .fold(0_u64, u64::wrapping_add);
    clear_seed ^ cell_seed.rotate_left(13)
}

fn patch_basis_fingerprint(basis: &PatchBasis) -> u64 {
    let acknowledged_seed = basis
        .acknowledged()
        .map_or(0_u64, projection_snapshot_fingerprint);
    let target_seed = basis
        .target()
        .map_or(0_u64, projection_snapshot_fingerprint);
    acknowledged_seed ^ target_seed.rotate_left(7)
}

fn scene_patch_fingerprint(patch: &ScenePatch) -> u64 {
    patch_basis_fingerprint(patch.basis()) ^ scene_patch_kind_fingerprint(patch.kind())
}

fn palette_spec_fingerprint(spec: &PaletteSpec) -> u64 {
    let mode_seed = spec
        .mode()
        .bytes()
        .fold(0_u64, |seed, byte| seed.rotate_left(5) ^ u64::from(byte));
    let cursor_color_seed = spec.cursor_color().map_or(0_u64, debug_fingerprint);
    let insert_cursor_seed = spec
        .cursor_color_insert_mode()
        .map_or(0_u64, debug_fingerprint);
    let normal_bg_seed = spec.normal_bg().map_or(0_u64, debug_fingerprint);
    let transparent_seed = debug_fingerprint(&spec.transparent_bg_fallback_color());
    let cterm_cursor_seed = spec.cterm_cursor_colors().map_or(0_u64, |colors| {
        colors
            .iter()
            .copied()
            .map(u64::from)
            .fold(0_u64, u64::wrapping_add)
    });
    let color_at_cursor_seed = spec.color_at_cursor().map_or(0_u64, debug_fingerprint);

    mode_seed
        ^ cursor_color_seed.rotate_left(3)
        ^ insert_cursor_seed.rotate_left(7)
        ^ normal_bg_seed.rotate_left(11)
        ^ transparent_seed.rotate_left(13)
        ^ cterm_cursor_seed.rotate_left(17)
        ^ spec.cterm_bg().map_or(0_u64, u64::from)
        ^ u64::from(spec.color_levels()).rotate_left(19)
        ^ spec.gamma_bits().rotate_left(23)
        ^ color_at_cursor_seed.rotate_left(29)
}

fn render_side_effects_fingerprint(side_effects: RenderSideEffects) -> u64 {
    let redraw_draw_seed = if side_effects.redraw_after_draw_if_cmdline {
        1_u64
    } else {
        0_u64
    };
    let redraw_clear_seed = if side_effects.redraw_after_clear_if_cmdline {
        1_u64
    } else {
        0_u64
    };
    let real_cursor_seed = if side_effects.allow_real_cursor_updates {
        1_u64
    } else {
        0_u64
    };
    redraw_draw_seed
        ^ redraw_clear_seed.rotate_left(5)
        ^ target_cell_presentation_fingerprint(side_effects.target_cell_presentation)
            .rotate_left(11)
        ^ cursor_visibility_fingerprint(side_effects.cursor_visibility)
        ^ real_cursor_seed.rotate_left(17)
}

fn realization_plan_fingerprint(plan: &RealizationPlan) -> u64 {
    match plan {
        RealizationPlan::Draw(draw) => {
            1_u64
                ^ palette_spec_fingerprint(draw.palette()).rotate_left(7)
                ^ render_allocation_policy_fingerprint(draw.allocation_policy()).rotate_left(13)
        }
        RealizationPlan::Clear(clear) => {
            2_u64 ^ u64::try_from(clear.max_kept_windows()).unwrap_or(u64::MAX)
        }
        RealizationPlan::Noop => 3_u64,
        RealizationPlan::Failure(failure) => {
            4_u64
                ^ apply_failure_kind_fingerprint(failure.reason())
                ^ realization_divergence_fingerprint(&failure.divergence()).rotate_left(11)
        }
    }
}

fn in_flight_proposal_fingerprint(proposal: &InFlightProposal) -> u64 {
    let animation_seed = match proposal.animation_schedule() {
        crate::core::state::AnimationSchedule::Idle => 0_u64,
        crate::core::state::AnimationSchedule::DefaultDelay => 1_u64,
        crate::core::state::AnimationSchedule::Deadline(deadline) => {
            2_u64 ^ deadline.value().rotate_left(13)
        }
    };
    let realization = proposal.realization();
    proposal.proposal_id().value()
        ^ scene_patch_fingerprint(proposal.patch())
        ^ realization_plan_fingerprint(&realization)
        ^ render_cleanup_action_fingerprint(proposal.cleanup_action()).rotate_left(5)
        ^ render_side_effects_fingerprint(proposal.side_effects()).rotate_left(9)
        ^ animation_seed.rotate_left(17)
}

fn degraded_apply_metrics_fingerprint(metrics: DegradedApplyMetrics) -> u64 {
    u64::try_from(metrics.planned_ops()).unwrap_or(u64::MAX)
        ^ u64::try_from(metrics.applied_ops()).unwrap_or(u64::MAX)
        ^ u64::try_from(metrics.skipped_ops_capacity()).unwrap_or(u64::MAX)
        ^ u64::try_from(metrics.reuse_failed_missing_window()).unwrap_or(u64::MAX)
        ^ u64::try_from(metrics.reuse_failed_reconfigure()).unwrap_or(u64::MAX)
        ^ u64::try_from(metrics.reuse_failed_missing_buffer()).unwrap_or(u64::MAX)
        ^ u64::try_from(metrics.windows_recovered()).unwrap_or(u64::MAX)
}

fn realization_divergence_fingerprint(divergence: &RealizationDivergence) -> u64 {
    match divergence {
        RealizationDivergence::ApplyMetrics(metrics) => {
            1_u64 ^ degraded_apply_metrics_fingerprint(*metrics)
        }
        RealizationDivergence::ShellStateUnknown => 2_u64,
    }
}

fn realization_ledger_fingerprint(ledger: &RealizationLedger) -> u64 {
    match ledger {
        RealizationLedger::Cleared => 1_u64,
        RealizationLedger::Consistent { acknowledged } => {
            2_u64 ^ projection_snapshot_fingerprint(acknowledged)
        }
        RealizationLedger::Diverged {
            last_consistent,
            divergence,
        } => {
            3_u64
                ^ last_consistent
                    .as_ref()
                    .map_or(0_u64, projection_snapshot_fingerprint)
                ^ realization_divergence_fingerprint(divergence)
        }
    }
}

fn runtime_state_fingerprint(runtime: &RuntimeState) -> u64 {
    debug_fingerprint(runtime)
}

fn protocol_fingerprint(protocol: &ProtocolState) -> u64 {
    let demand_seed = protocol
        .demand()
        .ordered()
        .values()
        .map(queued_demand_fingerprint)
        .fold(0_u64, u64::wrapping_add)
        ^ protocol
            .demand()
            .latest_cursor()
            .map_or(0_u64, queued_demand_fingerprint);

    match protocol {
        ProtocolState::Idle { .. } => 101_u64 ^ demand_seed,
        ProtocolState::Primed { .. } => 102_u64 ^ demand_seed,
        ProtocolState::ObservingRequest {
            request,
            probe_refresh,
            ..
        } => {
            103_u64
                ^ demand_seed
                ^ request.observation_id().value()
                ^ demand_fingerprint(request.demand())
                ^ probe_request_set_fingerprint(request.probes()).rotate_left(11)
                ^ protocol
                    .retained_observation()
                    .map_or(0_u64, observation_snapshot_fingerprint)
                    .rotate_left(17)
                ^ u64::from(probe_refresh.retry_count(crate::core::state::ProbeKind::CursorColor))
                    .rotate_left(23)
                ^ u64::from(probe_refresh.retry_count(crate::core::state::ProbeKind::Background))
                    .rotate_left(29)
        }
        ProtocolState::ObservingActive {
            request,
            observation,
            probe_refresh,
            ..
        } => {
            104_u64
                ^ demand_seed
                ^ request.observation_id().value()
                ^ demand_fingerprint(request.demand())
                ^ probe_request_set_fingerprint(request.probes()).rotate_left(11)
                ^ observation_snapshot_fingerprint(observation).rotate_left(17)
                ^ u64::from(probe_refresh.retry_count(crate::core::state::ProbeKind::CursorColor))
                    .rotate_left(23)
                ^ u64::from(probe_refresh.retry_count(crate::core::state::ProbeKind::Background))
                    .rotate_left(29)
        }
        ProtocolState::Ready { observation, .. } => {
            107_u64 ^ demand_seed ^ observation_snapshot_fingerprint(observation).rotate_left(5)
        }
        ProtocolState::Planning {
            observation,
            proposal_id,
            ..
        } => {
            109_u64
                ^ demand_seed
                ^ observation_snapshot_fingerprint(observation).rotate_left(5)
                ^ proposal_id.value().rotate_left(11)
        }
        ProtocolState::Applying {
            observation,
            proposal,
            ..
        } => {
            113_u64
                ^ demand_seed
                ^ observation_snapshot_fingerprint(observation).rotate_left(5)
                ^ in_flight_proposal_fingerprint(proposal)
        }
        ProtocolState::Recovering { observation, .. } => {
            127_u64
                ^ demand_seed
                ^ observation
                    .as_ref()
                    .map_or(0_u64, observation_snapshot_fingerprint)
                    .rotate_left(7)
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Transition {
    pub(crate) next: CoreState,
    pub(crate) effects: Vec<Effect>,
}

impl Transition {
    pub(super) fn new(next: CoreState, effects: Vec<Effect>) -> Self {
        Self { next, effects }
    }

    pub(super) fn stay(state: &CoreState) -> Self {
        Self::new(state.clone(), Vec::new())
    }

    pub(super) fn fingerprint(&self) -> u64 {
        let lifecycle = lifecycle_fingerprint(self.next.lifecycle());
        let cursor = self
            .next
            .last_cursor()
            .map_or(0_u64, crate::core::types::CursorPosition::fingerprint);
        let effects = self
            .effects
            .iter()
            .map(Effect::fingerprint)
            .fold(0_u64, u64::wrapping_add);

        let timers = self.next.timers();
        let timer_seed = timers.generation(TimerId::Animation).value()
            ^ timers.generation(TimerId::Ingress).value()
            ^ timers.generation(TimerId::Recovery).value()
            ^ timers.generation(TimerId::Cleanup).value()
            ^ timers
                .active_token(TimerId::Animation)
                .map_or(0_u64, TimerToken::fingerprint)
            ^ timers
                .active_token(TimerId::Ingress)
                .map_or(0_u64, TimerToken::fingerprint)
            ^ timers
                .active_token(TimerId::Recovery)
                .map_or(0_u64, TimerToken::fingerprint)
            ^ timers
                .active_token(TimerId::Cleanup)
                .map_or(0_u64, TimerToken::fingerprint);

        let recovery_policy_seed = u64::from(self.next.recovery_policy().retry_attempt());
        let ingress_policy_seed =
            ingress_policy_fingerprint(self.next.ingress_policy().last_cursor_autocmd_at());
        let cleanup_seed = debug_fingerprint(&self.next.render_cleanup());
        let entropy = self.next.entropy();
        let entropy_seed = entropy.next_proposal_id().value() ^ entropy.next_ingress_seq().value();
        let observation_seed = self
            .next
            .observation()
            .map_or(0_u64, observation_snapshot_fingerprint);
        let scene = self.next.scene();
        let semantic_seed = scene
            .semantics()
            .entities()
            .values()
            .map(semantic_entity_fingerprint)
            .fold(0_u64, u64::wrapping_add);
        let dirty_seed = scene
            .dirty()
            .entities()
            .iter()
            .copied()
            .map(SemanticEntityId::fingerprint)
            .fold(0_u64, u64::wrapping_add);
        let projection_seed = scene
            .projection_entry()
            .map_or(0_u64, projection_cache_entry_fingerprint);
        let scene_seed = scene.revision().value() ^ semantic_seed ^ dirty_seed ^ projection_seed;
        let realization_seed = realization_ledger_fingerprint(self.next.realization());
        let runtime_seed = runtime_state_fingerprint(self.next.runtime());

        lifecycle
            ^ cursor
            ^ self.next.generation().value()
            ^ effects
            ^ timer_seed
            ^ recovery_policy_seed
            ^ ingress_policy_seed
            ^ cleanup_seed
            ^ entropy_seed
            ^ observation_seed
            ^ scene_seed
            ^ realization_seed
            ^ runtime_seed
            ^ protocol_fingerprint(self.next.protocol())
    }
}
