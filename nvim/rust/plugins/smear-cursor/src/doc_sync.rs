#[cfg(test)]
mod architecture_enforcement {
    use pretty_assertions::assert_eq;
    use std::fs;
    use std::path::Path;
    use std::path::PathBuf;

    const STATE_OWNERSHIP_DOC: &str = include_str!("../../../docs/state_ownership.md");

    #[derive(Debug, Eq, PartialEq)]
    struct SourceMatch {
        path: String,
        text: String,
    }

    #[derive(Debug, Eq, PartialEq)]
    struct ForbiddenApiMatch {
        api: String,
        path: String,
        text: String,
    }

    fn crate_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    fn source_root() -> PathBuf {
        crate_root().join("src")
    }

    fn source_file(path: &str) -> PathBuf {
        crate_root().join(path)
    }

    fn read_source(path: &Path) -> String {
        fs::read_to_string(path)
            .unwrap_or_else(|err| panic!("failed to read source file {}: {err}", path.display()))
    }

    fn collect_rust_source_files(dir: &Path, files: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir)
            .unwrap_or_else(|err| panic!("failed to read source dir {}: {err}", dir.display()))
        {
            let path = entry
                .unwrap_or_else(|err| panic!("failed to read source dir entry: {err}"))
                .path();
            if path.is_dir() {
                collect_rust_source_files(&path, files);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                files.push(path);
            }
        }
    }

    fn rust_source_files() -> Vec<PathBuf> {
        let mut files = Vec::new();
        collect_rust_source_files(&source_root(), &mut files);
        files.sort();
        files
    }

    fn relative_source_path(path: &Path) -> String {
        path.strip_prefix(crate_root())
            .unwrap_or_else(|err| {
                panic!(
                    "source path {} was outside crate root {}: {err}",
                    path.display(),
                    crate_root().display()
                )
            })
            .to_string_lossy()
            .replace('\\', "/")
    }

    fn source_matches(pattern: &str) -> Vec<SourceMatch> {
        rust_source_files()
            .into_iter()
            .flat_map(|path| {
                let relative_path = relative_source_path(&path);
                read_source(&path)
                    .lines()
                    .filter(|line| line.contains(pattern))
                    .map(|line| SourceMatch {
                        path: relative_path.clone(),
                        text: line.trim().to_string(),
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    fn runtime_cell_lane_names() -> Vec<String> {
        let source = read_source(&source_file("src/events/runtime/cell.rs"));
        let struct_body = source
            .split_once("struct RuntimeCell {")
            .and_then(|(_, tail)| tail.split_once("\n}"))
            .map(|(body, _)| body)
            .expect("RuntimeCell struct body should be parseable");

        struct_body
            .lines()
            .filter_map(|line| line.trim().trim_end_matches(',').split_once(':'))
            .map(|(field, _)| field.trim().to_string())
            .collect()
    }

    fn documented_runtime_lane_fragment(lane_name: &str) -> String {
        format!("`RuntimeCell.{lane_name}`")
    }

    #[test]
    fn only_the_runtime_cell_declares_a_thread_local_root() {
        let thread_local_macro = ["thread", "_local", "!"].concat();

        assert_eq!(
            source_matches(&thread_local_macro),
            vec![SourceMatch {
                path: "src/events/runtime/cell.rs".to_string(),
                text: format!("{thread_local_macro} {{"),
            }]
        );

        let runtime_cell_source = read_source(&source_file("src/events/runtime/cell.rs"));
        assert!(
            runtime_cell_source.contains("static RUNTIME_CELL: RuntimeCell = RuntimeCell::new();")
        );
    }

    #[test]
    fn every_runtime_cell_lane_is_documented() {
        let missing_lanes = runtime_cell_lane_names()
            .into_iter()
            .filter(|lane_name| {
                !STATE_OWNERSHIP_DOC.contains(&documented_runtime_lane_fragment(lane_name))
            })
            .collect::<Vec<_>>();

        assert_eq!(missing_lanes, Vec::<String>::new());
    }

    #[test]
    fn only_the_host_facade_imports_the_neovim_api_module() {
        let host_api_path = ["nvim", "_oxi", "::", "api"].concat();

        assert_eq!(
            source_matches(&host_api_path),
            vec![SourceMatch {
                path: "src/host.rs".to_string(),
                text: format!("pub(crate) use {host_api_path};"),
            }]
        );
    }

    #[test]
    fn removed_broad_engine_access_ports_do_not_reappear() {
        let forbidden_apis = [
            ["read", "_engine", "_state"].concat(),
            ["mutate", "_engine", "_state"].concat(),
            ["with", "_core", "_runtime", "_mutation"].concat(),
        ];

        let matches = forbidden_apis
            .iter()
            .flat_map(|api| {
                source_matches(api)
                    .into_iter()
                    .map(|source_match| ForbiddenApiMatch {
                        api: api.clone(),
                        path: source_match.path,
                        text: source_match.text,
                    })
            })
            .collect::<Vec<_>>();

        assert_eq!(matches, Vec::<ForbiddenApiMatch>::new());
    }
}

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
            "runtime target equality keys must stay derived from the retained target position, shape, and tracked cursor; discrete target cell and retarget surface are not stored owners",
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
            "`RuntimeState.target` owns target `position`, `shape`, `tracked_cursor`, and `retarget_epoch`. `CursorTarget::retarget_key()` derives the reviewable equality surface, including the discrete target cell and retarget surface, so those derived facts are not stored separately. Target corners are derived on demand by `CursorTarget::corners()`.",
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
            "Timer runtime code mutates this lane through `with_timer_bridge` and starts or stops Neovim timers through `HostBridgePort`.",
            "`ShellState.editor_viewport_cache` retains the last live `EditorViewportSnapshot` read through `EditorViewportPort`.",
            "`EditorViewportSnapshot` is also the canonical shell-side owner of command-row math and `ViewportBounds` projection through `command_row()` and `bounds()`.",
            "Current mode, current window, current buffer, and current-handle validity checks used by lifecycle and ingress observation code cross through `CurrentEditorPort`.",
            "`WindowSurfaceSnapshot` is parsed by `src/events/surface.rs` from `getwininfo` and window-buffer host reads that cross through `WindowSurfacePort`.",
            "Cursor observation reads for window cursor position, `screenpos()`, command-line cursor position, conceal probes, and cursor text-context rows cross through `CursorReadPort`.",
            "Draw resource creation, option writes, namespace clears, extmark writes, and orphan-resource scans cross the host boundary through `DrawResourcePort`.",
            "`ShellState.buffer_perf_telemetry_cache` records callback EWMA and probe-pressure signals used to explain or derive future buffer performance policy.",
            "host notification and error output go through `HostLoggingPort` and cannot change reducer events, effects, or state transitions.",
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
            "`RuntimeState.target` owns target `position`, discrete `cell`, `shape`, `retarget_surface`, `tracked_cursor`, and `retarget_epoch`",
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
            "`src/events/runtime/editor_viewport.rs` owns the single command-row formula and `ViewportBounds` projection for shell-side viewport reads, with `FakeEditorViewportPort` covering cache behavior without live Neovim option reads.",
            "`src/host/current_editor.rs`, `src/events/handlers/observation/base.rs`, and cursor-autocmd ingress tests own current-editor host reads through `CurrentEditorPort`, with `FakeCurrentEditorPort` covering mode, current-window, current-buffer, and validity reads without live Neovim calls.",
            "`src/host/cursor_read.rs`, `src/events/cursor/screenpos.rs`, `src/events/cursor/conceal_tests.rs`, and `src/events/handlers/observation/text_context.rs` own cursor host reads through `CursorReadPort`, with `FakeCursorReadPort` covering cursor position, `screenpos()`, command-line cursor, conceal probe, and cursor text-context line reads without live Neovim calls.",
            "`src/events/logging.rs` and `src/draw/context.rs` own diagnostic notification and draw-error routing through `HostLoggingPort`, with `FakeHostLoggingPort` covering host-output behavior without live Neovim notification calls.",
            "`src/draw/floating_windows.rs` owns draw-resource host guard behavior through `DrawResourcePort`, with `FakeDrawResourcePort` covering eventignore suppression without live Neovim option calls.",
            "`src/events/surface.rs` owns `getwininfo` parsing and invalid-host-data rejection for `WindowSurfaceSnapshot`, with `FakeWindowSurfacePort` covering surface reads without live Neovim `getwininfo`, window-buffer, or text-height calls.",
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
