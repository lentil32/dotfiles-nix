use super::Transition;
use super::reduce;
use crate::core::effect::ApplyRenderCleanupEffect;
use crate::core::effect::CursorColorFallback;
use crate::core::effect::CursorPositionReadPolicy;
use crate::core::effect::Effect;
use crate::core::effect::EventLoopMetricEffect;
use crate::core::effect::IngressCursorPresentationEffect;
use crate::core::effect::IngressCursorPresentationRequest;
use crate::core::effect::ObservationRuntimeContext;
use crate::core::effect::ProbePolicy;
use crate::core::effect::ProbeQuality;
use crate::core::effect::RenderCleanupExecution;
use crate::core::effect::RequestObservationBaseEffect;
use crate::core::effect::RequestProbeEffect;
use crate::core::effect::ScheduleTimerEffect;
use crate::core::event::ApplyReport;
use crate::core::event::EffectFailedEvent;
use crate::core::event::Event;
use crate::core::event::ExternalDemandQueuedEvent;
use crate::core::event::InitializeEvent;
use crate::core::event::ObservationBaseCollectedEvent;
use crate::core::event::ProbeReportedEvent;
use crate::core::event::RenderCleanupAppliedAction;
use crate::core::event::RenderCleanupAppliedEvent;
use crate::core::event::RenderPlanComputedEvent;
use crate::core::event::TimerFiredWithTokenEvent;
use crate::core::event::TimerLostWithTokenEvent;
use crate::core::runtime_reducer::RenderAction;
use crate::core::runtime_reducer::RenderCleanupAction;
use crate::core::runtime_reducer::RenderSideEffects;
use crate::core::runtime_reducer::render_cleanup_delay_ms;
use crate::core::runtime_reducer::render_hard_cleanup_delay_ms;
use crate::core::state::BackgroundProbeBatch;
use crate::core::state::BackgroundProbeChunk;
use crate::core::state::BackgroundProbeChunkMask;
use crate::core::state::BackgroundProbePlan;
use crate::core::state::BufferPerfClass;
use crate::core::state::CoreState;
use crate::core::state::CursorColorSample;
use crate::core::state::CursorPositionSync;
use crate::core::state::CursorTextContext;
use crate::core::state::DegradedApplyMetrics;
use crate::core::state::DemandQueue;
use crate::core::state::ExternalDemand;
use crate::core::state::ExternalDemandKind;
use crate::core::state::InFlightProposal;
use crate::core::state::ObservationBasis;
use crate::core::state::ObservationMotion;
use crate::core::state::ObservationRequest;
use crate::core::state::ObservationSnapshot;
use crate::core::state::ObservedTextRow;
use crate::core::state::PatchBasis;
use crate::core::state::ProbeFailure;
use crate::core::state::ProbeKind;
use crate::core::state::ProbeRequestSet;
use crate::core::state::ProbeReuse;
use crate::core::state::ProbeSlot;
use crate::core::state::ProbeState;
use crate::core::state::RealizationClear;
use crate::core::state::RealizationDivergence;
use crate::core::state::RealizationLedger;
use crate::core::state::RealizationPlan;
use crate::core::state::RecoveryPolicyState;
use crate::core::state::RenderThermalState;
use crate::core::state::ScenePatch;
use crate::core::state::SemanticEntityId;
use crate::core::types::CursorCol;
use crate::core::types::CursorPosition;
use crate::core::types::CursorRow;
use crate::core::types::DelayBudgetMs;
use crate::core::types::IngressSeq;
use crate::core::types::Lifecycle;
use crate::core::types::Millis;
use crate::core::types::ProposalId;
use crate::core::types::TimerId;
use crate::core::types::TimerToken;
use crate::core::types::ViewportSnapshot;
use crate::state::CursorLocation;
use crate::state::CursorShape;
use crate::state::RuntimeState;
use crate::test_support::cursor;
use crate::test_support::cursor_color_probe_witness;
use crate::test_support::sparse_probe_cells;
use crate::types::Point;
use crate::types::ScreenCell;
use pretty_assertions::assert_eq as pretty_assert_eq;

mod support;

pub(in crate::core::reducer::tests) use self::support::*;

struct ObservationScenario {
    request: ObservationRequest,
    basis: ObservationBasis,
    based: Transition,
}

impl ObservationScenario {
    fn new(ready: CoreState) -> Self {
        let observing =
            observing_state_from_demand(&ready, ExternalDemandKind::ExternalCursor, 25, None);
        let request = active_request(&observing);
        let basis = observation_basis(&request, Some(cursor(7, 8)), 26);
        let based =
            collect_observation_base(&observing, &request, basis.clone(), observation_motion());
        Self {
            request,
            basis,
            based,
        }
    }

    fn with_background_plan(ready: CoreState, plan_cells: Vec<ScreenCell>) -> Self {
        let observing =
            observing_state_from_demand(&ready, ExternalDemandKind::ExternalCursor, 25, None);
        let request = active_request(&observing);
        let basis = observation_basis(&request, Some(cursor(7, 8)), 26);
        let observation =
            ObservationSnapshot::new(request.clone(), basis.clone(), observation_motion())
                .with_background_probe_plan(BackgroundProbePlan::from_cells(plan_cells));
        let next = observing
            .clone()
            .with_last_cursor(Some(cursor(7, 8)))
            .with_active_observation(Some(observation.clone()))
            .expect("observation should stay active");
        let cursor_color_fallback = retained_cursor_color_fallback(&observing);
        let effect = if ready.runtime().config.requires_cursor_color_sampling() {
            Effect::RequestProbe(RequestProbeEffect {
                observation_basis: Box::new(basis.clone()),
                probe_request_id: ProbeKind::CursorColor.request_id(request.observation_id()),
                kind: ProbeKind::CursorColor,
                cursor_position_policy: cursor_position_policy(&observing),
                buffer_perf_class: request.demand().buffer_perf_class(),
                probe_policy: expected_probe_policy(
                    request.demand().kind(),
                    request.demand().buffer_perf_class(),
                    cursor_color_fallback.as_ref(),
                ),
                background_chunk: None,
                cursor_color_fallback,
            })
        } else {
            Effect::RequestProbe(RequestProbeEffect {
                observation_basis: Box::new(basis.clone()),
                probe_request_id: ProbeKind::Background.request_id(request.observation_id()),
                kind: ProbeKind::Background,
                cursor_position_policy: cursor_position_policy(&observing),
                buffer_perf_class: request.demand().buffer_perf_class(),
                probe_policy: expected_probe_policy(
                    request.demand().kind(),
                    request.demand().buffer_perf_class(),
                    cursor_color_fallback.as_ref(),
                ),
                background_chunk: observation
                    .background_progress()
                    .and_then(crate::core::state::BackgroundProbeProgress::next_chunk),
                cursor_color_fallback: None,
            })
        };

        Self {
            request,
            basis,
            based: Transition::new(next, vec![effect]),
        }
    }

    fn with_background_probe_cell_count(ready: CoreState, cell_count: usize) -> Self {
        let observing =
            observing_state_from_demand(&ready, ExternalDemandKind::ExternalCursor, 25, None);
        let request = active_request(&observing);
        let basis = observation_basis(&request, Some(cursor(7, 8)), 26);
        Self::with_background_plan(ready, sparse_probe_cells(basis.viewport(), cell_count))
    }
}

mod animation_timer_draw_state;
mod apply_completion;
mod apply_completion_resume;
mod cleanup_lifecycle;
mod delayed_cursor_demand_queue;
mod failure_handling;
mod ingress_and_animation_planning;
mod initialization;
mod observation_base_collection;
mod observation_completion;
mod observation_completion_planning;
mod observation_request_planning;
mod observing_cursor_demand_queue;
mod probe_completion_sequence;
mod probe_failure_retention;
mod probe_refresh_retry_budget;
mod probe_retry;
mod protocol_shared_state_constructors;
