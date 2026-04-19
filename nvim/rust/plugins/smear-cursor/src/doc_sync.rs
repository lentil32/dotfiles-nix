#[cfg(test)]
mod state_ownership_doc_sync {
    use pretty_assertions::assert_eq;

    const STATE_OWNERSHIP_DOC: &str = include_str!("../../../docs/state_ownership.md");

    fn normalized_state_ownership_doc() -> String {
        STATE_OWNERSHIP_DOC
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn missing_fragments<'a>(doc: &str, expected_fragments: &'a [&'a str]) -> Vec<&'a str> {
        expected_fragments
            .iter()
            .copied()
            .filter(|fragment| !doc.contains(fragment))
            .collect()
    }

    fn present_forbidden_fragments<'a>(
        doc: &str,
        forbidden_fragments: &'a [&'a str],
    ) -> Vec<&'a str> {
        forbidden_fragments
            .iter()
            .copied()
            .filter(|fragment| doc.contains(fragment))
            .collect()
    }

    #[test]
    fn lists_current_invariant_hooks_and_semantic_surfaces() {
        let doc = normalized_state_ownership_doc();
        let expected_fragments = [
            "`RuntimeState::debug_assert_invariants()`",
            "`ObservationSnapshot::debug_assert_invariants()`",
            "`ProjectionState::debug_assert_invariants()`",
            "`RealizationLedger::debug_assert_invariants()`",
            "`ProtocolState::debug_assert_invariants()`",
            "`CoreState::debug_assert_invariants()`",
            "`apply_runtime_options()` normalizes the user-facing `fps` alias into `RuntimeConfig.time_interval`",
            "`RenderCleanupState::scheduled()` clamps cleanup delays before the scheduler stores them in `ProtocolSharedState.render_cleanup`.",
            "`TimerState::{arm, active_token, clear_matching}` keep timer generation and armed/disarmed ownership in one reducer slot per timer id; stale tokens are rejected instead of becoming a second owner.",
            "`CoreState::{enter_observing_request, activate_observation, replace_active_observation_with_pending, enter_ready, complete_active_observation, restore_retained_observation_to_ready, enter_planning, enter_applying, take_pending_proposal, restore_retained_observation}` are the protocol construction boundaries that reject cross-phase observation or proposal payload injection instead of persisting an invalid workflow shape.",
            "`RuntimeState::semantic_view()` compares authoritative runtime state while ignoring purgeable scratch buffers and rebuildable particle/config caches.",
            "`ProjectionHandle::semantic_view()` compares retained projection witness plus logical raster while ignoring reuse-key and cached realization drift.",
            "`InFlightProposal::semantic_view()` compares authoritative proposal payload through semantic patch-basis views rather than cached projection internals.",
            "`CoreState::semantic_view()` compares authoritative reducer state across protocol, runtime, scene, and realization owners while ignoring runtime scratch buffers, projection reuse caches, and cached shell materialization.",
        ];

        assert_eq!(
            missing_fragments(&doc, &expected_fragments),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn lists_current_runtime_and_protocol_single_owner_facts() {
        let doc = normalized_state_ownership_doc();
        let expected_fragments = [
            "`RuntimeState.config` owns user-configured motion behavior, including render-cleanup retention policy such as `max_kept_windows`. Frame timing is stored canonically as `time_interval`; `fps` remains only a boundary alias that normalizes into that field.",
            "`RuntimeState.config_revision` is the only freshness owner for config-derived runtime views. `DerivedConfigCache` intentionally stores no mirror revision.",
            "`ProtocolSharedState.timers` owns timer-slot generations and armed/disarmed lifecycle state. `TimerToken`s are derived views of the currently armed slots, not a second stored owner.",
            "`ProtocolSharedState.render_cleanup` owns cleanup thermal phase and deadlines only through `RenderCleanupState::{Hot, Cooling, Cold}`. Retention budgets are derived from the current runtime config instead of being copied into scheduler state.",
            "`ProtocolState.phase` is the only workflow owner. There is no separate workflow/slot matrix.",
            "Identity is derived only from the observation root demand sequence: `PendingObservation.demand.seq()` while collecting and `ObservationSnapshot.demand.seq()` once active. `ObservationId` accessors compute from that root; neither the snapshot nor active probe lifecycle state stores a mirrored current-observation id.",
            "Raw retained projection access stays inside `crate::core`; shell and event code cross the boundary through explicit `ProjectionHandle` views instead of borrowing `RetainedProjection` directly.",
        ];

        assert_eq!(
            missing_fragments(&doc, &expected_fragments),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn lists_current_boundary_and_shell_cache_owners() {
        let doc = normalized_state_ownership_doc();
        let expected_fragments = [
            "`ProjectionState.cache` owns the purgeable retained projection reuse state.",
            "`ProjectionHandle` shares one immutable `RetainedProjection` between projection cache and realization ledger instead of cloning snapshot payloads. Cross-module consumers project it through explicit views such as `semantic_view()` or `shell_projection()` rather than generic deref access.",
            "`RealizationLedger::Consistent.acknowledged` owns the shell-trusted projection handle.",
            "`ShellState.probe_cache` owns purgeable cursor-color, cursor-text-context, conceal-region, conceal-delta, and conceal-screen-cell reuse keyed by external witnesses such as `CursorColorProbeWitness`, buffer-local text revisions, and window state.",
            "`ShellState.editor_viewport_cache` retains the last live `EditorViewport` read from Neovim.",
            "`ShellState.buffer_perf_telemetry_cache` records callback EWMA and probe-pressure signals used to explain or derive future buffer performance policy.",
        ];

        assert_eq!(
            missing_fragments(&doc, &expected_fragments),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn omits_removed_duplicate_owner_descriptions() {
        let doc = normalized_state_ownership_doc();
        let forbidden_fragments = [
            "`DerivedConfigCache.source_revision`",
            "`RuntimeState.config_revision` and `DerivedConfigCache.source_revision`",
            "`idle_target_budget`",
            "`max_prune_per_tick`",
            "active timer tokens are the owner",
        ];

        assert_eq!(
            present_forbidden_fragments(&doc, &forbidden_fragments),
            Vec::<&str>::new()
        );
    }
}
