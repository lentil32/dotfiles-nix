use super::super::CursorAutocmdPreflight;
use super::super::build_cursor_autocmd_events;
use super::super::cursor_autocmd_preflight;
use super::super::demand_kind_for_autocmd;
use super::super::should_coalesce_window_follow_up_autocmd;
use super::super::should_drop_unchanged_cursor_autocmd;
use super::autocmd_ingress_strategy;
use super::fast_path_snapshot;
use super::perf_class_strategy;
use super::presentation;
use super::snapshot_with_state;
use crate::core::event::Event as CoreEvent;
use crate::core::event::ExternalDemandQueuedEvent;
use crate::core::event::InitializeEvent;
use crate::core::state::BufferPerfClass;
use crate::core::types::Millis;
use crate::events::ingress::AutocmdIngress;
use crate::position::RenderPoint;
use crate::state::TrackedCursor;
use crate::test_support::proptest::pure_config;
use pretty_assertions::assert_eq;
use proptest::prelude::*;

proptest! {
    #![proptest_config(pure_config())]

    #[test]
    fn prop_cursor_autocmd_builder_matches_routing_and_presentation_rules(
        ingress in autocmd_ingress_strategy(),
        observed_at in any::<u64>(),
        needs_initialize in any::<bool>(),
        buffer_perf_class in prop_oneof![
            Just(BufferPerfClass::Full),
            Just(BufferPerfClass::FastMotion),
            Just(BufferPerfClass::Skip),
        ],
        include_presentation in any::<bool>(),
    ) {
        let observed_at = Millis::new(observed_at);
        let ingress_cursor_presentation = include_presentation.then(presentation);
        let kind = demand_kind_for_autocmd(ingress);
        let mut expected = Vec::new();

        if needs_initialize {
            expected.push(CoreEvent::Initialize(InitializeEvent { observed_at }));
        }
        expected.push(CoreEvent::ExternalDemandQueued(ExternalDemandQueuedEvent {
            kind,
            observed_at,
            buffer_perf_class,
            ingress_cursor_presentation: if kind.is_cursor() {
                ingress_cursor_presentation
            } else {
                None
            },
            ingress_observation_surface: None,
        }));

        prop_assert_eq!(
            build_cursor_autocmd_events(
                ingress,
                observed_at,
                needs_initialize,
                buffer_perf_class,
                ingress_cursor_presentation,
                None,
            ),
            expected
        );
    }

    #[test]
    fn prop_cursor_autocmd_preflight_matches_enable_validity_and_perf_class_gates(
        enabled in any::<bool>(),
        window_valid in any::<bool>(),
        buffer_valid in any::<bool>(),
        buffer_perf_class in perf_class_strategy(),
    ) {
        let snapshot = snapshot_with_state(enabled, buffer_perf_class, None);
        let expected = if !enabled || !window_valid || !buffer_valid {
            CursorAutocmdPreflight::Dropped
        } else {
            match buffer_perf_class {
                Some(BufferPerfClass::Skip) => CursorAutocmdPreflight::Dropped,
                Some(buffer_perf_class) => CursorAutocmdPreflight::Continue { buffer_perf_class },
                None => CursorAutocmdPreflight::MissingPerfClass,
            }
        };

        prop_assert_eq!(
            cursor_autocmd_preflight(&snapshot, window_valid, buffer_valid),
            expected
        );
    }

    #[test]
    fn prop_window_follow_up_coalescing_depends_only_on_buf_enter_and_window_change(
        ingress in autocmd_ingress_strategy(),
        tracked_window_handle in 1_i64..=64_i64,
        current_window_handle in 1_i64..=64_i64,
        tracked_cursor_present in any::<bool>(),
    ) {
        let tracked_cursor = tracked_cursor_present
            .then(|| TrackedCursor::fixture(tracked_window_handle, 22, 3, 4));
        let snapshot = snapshot_with_state(
            true,
            Some(BufferPerfClass::Full),
            tracked_cursor,
        );
        let expected = ingress == AutocmdIngress::BufEnter
            && tracked_cursor_present
            && tracked_window_handle != current_window_handle;

        prop_assert_eq!(
            should_coalesce_window_follow_up_autocmd(
                ingress,
                &snapshot,
                current_window_handle,
            ),
            expected
        );
    }
}

#[test]
fn unchanged_cursor_fast_path_requires_matching_surface_and_target() {
    let tracked_cursor = TrackedCursor::fixture(10, 20, 4, 12)
        .with_viewport_columns(3, 1)
        .with_window_origin(5, 7)
        .with_window_dimensions(80, 24);
    let matching_target = RenderPoint {
        row: 11.0,
        col: 22.0,
    };
    let matching_snapshot =
        fast_path_snapshot(true, false, Some(tracked_cursor.clone()), matching_target);

    for (label, ingress, snapshot, current_location, current_target, expected) in [
        (
            "cursor moved always stays live",
            AutocmdIngress::CursorMoved,
            matching_snapshot.clone(),
            Some(tracked_cursor.clone()),
            Some(matching_target),
            false,
        ),
        (
            "insert repeat always stays live",
            AutocmdIngress::CursorMovedInsert,
            matching_snapshot.clone(),
            Some(tracked_cursor.clone()),
            Some(matching_target),
            false,
        ),
        (
            "window scrolled repeat",
            AutocmdIngress::WinScrolled,
            matching_snapshot.clone(),
            Some(tracked_cursor.clone()),
            Some(matching_target),
            true,
        ),
        (
            "window enter repeat",
            AutocmdIngress::WinEnter,
            matching_snapshot.clone(),
            Some(tracked_cursor.clone()),
            Some(matching_target),
            true,
        ),
        (
            "buffer enter repeat",
            AutocmdIngress::BufEnter,
            matching_snapshot.clone(),
            Some(tracked_cursor.clone()),
            Some(matching_target),
            true,
        ),
        (
            "mode changes still require full path",
            AutocmdIngress::ModeChanged,
            matching_snapshot.clone(),
            Some(tracked_cursor.clone()),
            Some(matching_target),
            false,
        ),
        (
            "surface changes stay live",
            AutocmdIngress::CursorMoved,
            matching_snapshot.clone(),
            Some(TrackedCursor::fixture(10, 20, 4, 13)),
            Some(matching_target),
            false,
        ),
        (
            "target changes stay live",
            AutocmdIngress::CursorMoved,
            matching_snapshot.clone(),
            Some(tracked_cursor.clone()),
            Some(RenderPoint {
                row: 12.0,
                col: 22.0,
            }),
            false,
        ),
        (
            "missing live position disables the fast path",
            AutocmdIngress::CursorMoved,
            matching_snapshot,
            Some(tracked_cursor.clone()),
            None,
            false,
        ),
        (
            "uninitialized runtime disables the fast path",
            AutocmdIngress::CursorMoved,
            fast_path_snapshot(true, true, Some(tracked_cursor.clone()), matching_target),
            Some(tracked_cursor.clone()),
            Some(matching_target),
            false,
        ),
        (
            "disabled runtime disables the fast path",
            AutocmdIngress::CursorMoved,
            fast_path_snapshot(false, false, Some(tracked_cursor.clone()), matching_target),
            Some(tracked_cursor.clone()),
            Some(matching_target),
            false,
        ),
        (
            "missing tracked cursor disables the fast path",
            AutocmdIngress::CursorMoved,
            fast_path_snapshot(true, false, None, matching_target),
            Some(tracked_cursor),
            Some(matching_target),
            false,
        ),
    ] {
        assert_eq!(
            should_drop_unchanged_cursor_autocmd(
                ingress,
                &snapshot,
                current_location.as_ref(),
                current_target,
            ),
            expected,
            "{label}"
        );
    }
}
