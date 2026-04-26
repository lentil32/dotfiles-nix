use super::super::cursor::current_mode;
use super::super::host_bridge::ensure_namespace_id;
use super::super::logging::log_slow_callback;
use super::super::logging::should_log_slow_callback;
use super::super::logging::warn;
use super::super::runtime::cursor_callback_duration_estimate_ms;
use super::super::runtime::now_ms;
use super::super::runtime::record_cursor_callback_duration;
use super::super::runtime::record_degraded_draw_application;
use super::super::runtime::to_core_millis;
use super::super::trace::apply_report_summary;
use super::super::trace::proposal_summary;
use super::render_apply::ApplyRenderActionError;
use super::render_apply::apply_render_action;
use crate::core::effect::ApplyProposalEffect;
use crate::core::event::ApplyReport;
use crate::core::event::EffectFailedEvent;
use crate::core::event::Event as CoreEvent;
use crate::core::state::ApplyFailureKind;
use crate::core::state::RealizationDivergence;

pub(crate) fn execute_core_apply_proposal_effect(payload: ApplyProposalEffect) -> Vec<CoreEvent> {
    let observed_at = to_core_millis(now_ms());
    let proposal = payload.proposal;
    let buffer_handle = payload.buffer_handle;
    let proposal_id = proposal.proposal_id();
    let proposal_trace = proposal_summary(&proposal);
    super::super::logging::trace_lazy(|| {
        format!(
            "apply_proposal_start requested_at={} {}",
            payload.requested_at.value(),
            proposal_trace
        )
    });
    let apply_outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let apply_started_ms = now_ms();
        let namespace_id = match ensure_namespace_id() {
            Ok(namespace_id) => namespace_id,
            Err(err) => {
                warn(&format!(
                    "runtime lane re-entered while resolving render namespace; applying typed failure: {err}"
                ));
                return (
                    CoreEvent::ApplyReported(ApplyReport::ApplyFailed {
                        proposal_id,
                        reason: ApplyFailureKind::ShellError,
                        divergence: RealizationDivergence::ShellStateUnknown,
                        observed_at,
                    }),
                    0,
                );
            }
        };

        if let Some(failure) = proposal.failure_reason() {
            return (
                CoreEvent::ApplyReported(ApplyReport::ApplyFailed {
                    proposal_id,
                    reason: failure.reason(),
                    divergence: failure.divergence(),
                    observed_at,
                }),
                0,
            );
        }

        let apply_result = apply_render_action(namespace_id, &proposal);

        let apply_duration_ms = (now_ms() - apply_started_ms).max(0.0);
        record_cursor_callback_duration(buffer_handle, apply_duration_ms);
        let apply_duration_estimate_ms = cursor_callback_duration_estimate_ms(buffer_handle);
        let should_log_apply_perf = should_log_slow_callback(apply_duration_ms);

        match apply_result {
            Ok(metrics) if metrics.is_degraded_apply() => {
                record_degraded_draw_application();
                if should_log_apply_perf {
                    let mode = current_mode();
                    let details = metrics.perf_details();
                    log_slow_callback(
                        "core_render_apply",
                        &mode,
                        apply_duration_ms,
                        apply_duration_estimate_ms,
                        &details,
                    );
                }
                (
                    CoreEvent::ApplyReported(ApplyReport::AppliedDegraded {
                        proposal_id,
                        divergence: RealizationDivergence::ApplyMetrics(
                            metrics.degraded_apply_metrics(),
                        ),
                        observed_at,
                        visual_change: metrics.had_visual_change(),
                    }),
                    metrics.retained_cleanup_resources,
                )
            }
            Ok(metrics) => {
                if should_log_apply_perf {
                    let mode = current_mode();
                    let details = metrics.perf_details();
                    log_slow_callback(
                        "core_render_apply",
                        &mode,
                        apply_duration_ms,
                        apply_duration_estimate_ms,
                        &details,
                    );
                }
                (
                    CoreEvent::ApplyReported(ApplyReport::AppliedFully {
                        proposal_id,
                        observed_at,
                        visual_change: metrics.had_visual_change(),
                    }),
                    metrics.retained_cleanup_resources,
                )
            }
            Err(err) => {
                let (reason, divergence) = match err {
                    ApplyRenderActionError::ViewportDrift => (
                        ApplyFailureKind::ViewportDrift,
                        RealizationDivergence::ShellStateUnknown,
                    ),
                    ApplyRenderActionError::DrawProposalMissingTarget => {
                        warn(
                            "draw proposal reached render shell apply without a target projection",
                        );
                        (
                            ApplyFailureKind::MissingProjection,
                            RealizationDivergence::ShellStateUnknown,
                        )
                    }
                    ApplyRenderActionError::Shell(error) => {
                        warn(&format!("core render apply failed: {error}"));
                        (
                            ApplyFailureKind::ShellError,
                            RealizationDivergence::ShellStateUnknown,
                        )
                    }
                    ApplyRenderActionError::FailureProposalReachedShell(failure) => {
                        warn(
                            "failure proposal reached render shell apply; preserving typed failure",
                        );
                        (failure.reason(), failure.divergence())
                    }
                };
                (
                    CoreEvent::ApplyReported(ApplyReport::ApplyFailed {
                        proposal_id,
                        reason,
                        divergence,
                        observed_at,
                    }),
                    0,
                )
            }
        }
    }));

    let (follow_up, retained_resources) = apply_outcome.unwrap_or_else(|_| {
        // Surprising: apply effect panicked after the reducer committed the proposal.
        // Emit a typed failure so recovery can preserve divergence from the acknowledged basis.
        warn("core render apply panicked");
        (
            CoreEvent::EffectFailed(EffectFailedEvent {
                proposal_id: Some(proposal_id),
                observed_at,
            }),
            0,
        )
    });
    super::super::logging::trace_lazy(|| {
        let report_summary = match &follow_up {
            CoreEvent::ApplyReported(report) => apply_report_summary(report),
            CoreEvent::EffectFailed(payload) => format!(
                "effect_failed(proposal_id={} observed_at={})",
                payload.proposal_id.map_or_else(
                    || "none".to_string(),
                    |proposal_id| proposal_id.value().to_string()
                ),
                payload.observed_at.value(),
            ),
            _ => "unexpected_follow_up".to_string(),
        };
        format!("apply_proposal_result {proposal_trace} {report_summary}")
    });

    let mut follow_ups = vec![follow_up];
    if let Some(event) =
        super::retained_resource_cleanup_retry_event(retained_resources, observed_at)
    {
        follow_ups.push(event);
    }
    follow_ups
}
