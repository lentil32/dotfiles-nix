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
            "runtime target cell and retarget-surface facts must stay derived from the retained target position and tracked cursor",
            "exact observation samples must refresh `latest_exact_cursor_cell`, while deferred and unavailable samples preserve the retained exact-anchor cache",
            "`apply_runtime_options()` validates and normalizes `time_interval` before `RuntimeState.config` is mutated, so frame timing has one accepted boundary key and one retained owner.",
            "`apply_runtime_options()` validates and normalizes `color_levels` before `RuntimeState.config` is mutated, so palette quantization stays bounded at the config boundary instead of inflating draw-time highlight state.",
            "`RenderCleanupState::scheduled()` clamps cleanup delays before the scheduler stores them in `ProtocolSharedState.render_cleanup`.",
            "`TimerState::{arm, active_token, clear_matching}` keep timer generation and armed/disarmed ownership in one reducer slot per timer id; stale tokens are rejected instead of becoming a second owner.",
            "`CoreState::{enter_observing_request, activate_observation, replace_active_observation_with_pending, enter_ready, complete_active_observation, restore_retained_observation_to_ready, enter_planning, enter_applying, take_pending_proposal, restore_retained_observation}` are the protocol construction boundaries that reject cross-phase observation or proposal payload injection instead of persisting an invalid workflow shape.",
            "`RuntimeState::semantic_view()` compares authoritative runtime state while ignoring purgeable scratch buffers and rebuildable particle/config caches.",
            "`ProjectionHandle::semantic_view()` compares retained projection witness plus logical raster while ignoring reuse-key and cached realization drift.",
            "`InFlightProposal::semantic_view()` compares authoritative proposal payload through semantic patch-basis views rather than cached projection internals.",
            "`CoreState::semantic_view()` compares authoritative reducer state across protocol, runtime, scene, and realization owners while ignoring runtime scratch buffers, projection reuse caches, and cached shell materialization.",
            "observation-owned cursor cells stay in projected display space; raw host quirks such as conceal and `screenpos()` remain event-layer concerns",
            "requested probe policy may allow deferred projection, but it may not switch reducer-owned cursor truth out of projected display space",
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
            "`RuntimeState.config` owns user-configured motion behavior, including render-cleanup retention policy such as `max_kept_windows`. Frame timing is stored canonically as `time_interval`, which is also the only accepted runtime option key for that fact.",
            "`RuntimeState.config_revision` is the only freshness owner for config-derived runtime views. `DerivedConfigCache` intentionally stores no mirror revision.",
            "`RuntimeState.target` owns target `position`, discrete `cell`, `shape`, `retarget_surface`, `tracked_cursor`, and `retarget_epoch`. `CursorTarget::retarget_key()` derives the reviewable equality surface, and target corners are derived on demand by `CursorTarget::corners()` instead of being stored separately.",
            "`ProtocolSharedState.demand` owns at most one pending `ExternalDemand` per `ExternalDemandKind`; same-kind ingress coalesces in place while dequeue order is still derived from the occupied demand sequences.",
            "`ProtocolSharedState.timers` owns timer-slot generations and armed/disarmed lifecycle state. `TimerToken`s are derived views of the currently armed slots, not a second stored owner.",
            "`ProtocolSharedState.render_cleanup` owns cleanup thermal phase and deadlines only through `RenderCleanupState::{Hot, Cooling, Cold}`. Retention budgets are derived from the current runtime config instead of being copied into scheduler state.",
            "`ProtocolState.phase` is the only workflow owner. There is no separate workflow/slot matrix.",
            "Identity is derived only from the observation root demand sequence: `PendingObservation.demand.seq()` while collecting and `ObservationSnapshot.demand.seq()` once active. `ObservationId` accessors compute from that root; neither the snapshot nor active probe lifecycle state stores a mirrored current-observation id.",
            "`PendingObservation.requested_probes` is the only owner of probe policy before activation. `ObservationSnapshot::new()` consumes it to initialize active probe lifecycle state. The policy chooses freshness, reuse, and fallback cost only; it does not choose between raw and projected cursor coordinate systems.",
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
            "`ShellState.editor_viewport_cache` retains the last live `EditorViewportSnapshot` read from Neovim.",
            "`EditorViewportSnapshot` is also the canonical shell-side owner of command-row math and `ViewportBounds` projection through `command_row()` and `bounds()`.",
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

#[cfg(test)]
mod testing_taxonomy_doc_sync {
    use pretty_assertions::assert_eq;

    const TESTING_TAXONOMY_DOC: &str =
        include_str!("../../../docs/smear-cursor-testing-taxonomy.md");

    fn normalized_testing_taxonomy_doc() -> String {
        TESTING_TAXONOMY_DOC
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

    #[test]
    fn lists_current_position_refactor_test_owners() {
        let doc = normalized_testing_taxonomy_doc();
        let expected_fragments = [
            "`src/position/tests.rs` owns shared position primitives such as `ScreenCell`, `ViewportBounds`, `RenderPoint`, and `ObservedCell`, including positivity, one-based indexing, and canonical conversions.",
            "`src/events/runtime/editor_viewport.rs` owns the single command-row formula and `ViewportBounds` projection for shell-side viewport reads.",
            "`src/events/surface.rs` owns `getwininfo` parsing and invalid-host-data rejection for `WindowSurfaceSnapshot`.",
            "`src/core/state/observation/tests/` together with `src/core/reducer/tests/observation_base_collection.rs` own exact, deferred, and unavailable cursor-sample behavior, including exact-anchor retention.",
            "`src/events/probe_cache/tests/` owns probe-witness reuse and invalidation boundaries for conceal and text-context facts that now depend on the shared surface and cursor vocabulary.",
            "`src/state/machine/types.rs` owns `CursorTarget` retarget-key composition and the `retarget_epoch` bump/no-bump rules for cell, shape, and retarget-surface changes.",
            "`src/core/runtime_reducer/tests/retargeting_while_animating.rs`, `viewport_scroll_translation.rs`, and `window_resize_reflow.rs` keep boundary smokes for animated retarget application, scroll-translation stability, resize classification, and render-facing `retarget_epoch` propagation.",
            "Snapshot tests stay limited to user-visible trace or diagnostic output when renamed runtime fields reshape formatted output.",
        ];

        assert_eq!(
            missing_fragments(&doc, &expected_fragments),
            Vec::<&str>::new()
        );
    }
}

#[cfg(test)]
mod position_spec_doc_sync {
    use pretty_assertions::assert_eq;

    const POSITION_SPEC_DOC: &str = include_str!("../../../docs/position-spec.md");

    fn normalized_position_spec_doc() -> String {
        POSITION_SPEC_DOC
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
    fn lists_current_projected_cursor_contract() {
        let doc = normalized_position_spec_doc();
        let expected_fragments = [
            "`ObservedCell::Exact` and `ObservedCell::Deferred` both carry projected display-space `ScreenCell` values. `Deferred` means freshness is lower and an exact refresh is still owed; it does not mean the cell is still in raw host coordinates.",
            "cached or deferred projection during fast motion, followed by exact refresh when needed",
            "Probe policy may choose exact projection versus deferred-allowed reads, but it may not change the coordinate space of the returned observation.",
            "exact and deferred observed cells both retain projected display-space cursor cells",
            "probe policy chooses freshness/cost only; it does not switch between raw and projected coordinate systems",
        ];

        assert_eq!(
            missing_fragments(&doc, &expected_fragments),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn omits_removed_raw_fallback_wording() {
        let doc = normalized_position_spec_doc();
        let forbidden_fragments = ["raw fallback during fast motion"];

        assert_eq!(
            present_forbidden_fragments(&doc, &forbidden_fragments),
            Vec::<&str>::new()
        );
    }
}
