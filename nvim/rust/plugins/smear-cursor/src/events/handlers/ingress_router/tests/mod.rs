mod cursor_autocmd;
mod non_cursor_autocmd;

use super::CursorAutocmdFastPathSnapshot;
use crate::config::BufferPerfMode;
use crate::core::effect::IngressCursorPresentationRequest;
use crate::core::state::BufferPerfClass;
use crate::events::ingress::AutocmdIngress;
use crate::events::runtime::mutate_engine_state;
use crate::events::runtime::IngressReadSnapshot;
use crate::events::runtime::IngressReadSnapshotTestInput;
use crate::position::RenderPoint;
use crate::state::TrackedCursor;
use crate::types::CursorCellShape;
use proptest::prelude::*;

pub(super) fn presentation() -> IngressCursorPresentationRequest {
    IngressCursorPresentationRequest::new(true, true, None, CursorCellShape::Block)
}

pub(super) fn autocmd_ingress_strategy() -> BoxedStrategy<AutocmdIngress> {
    prop_oneof![
        Just(AutocmdIngress::CmdlineChanged),
        Just(AutocmdIngress::CursorMoved),
        Just(AutocmdIngress::CursorMovedInsert),
        Just(AutocmdIngress::ModeChanged),
        Just(AutocmdIngress::WinEnter),
        Just(AutocmdIngress::WinScrolled),
        Just(AutocmdIngress::BufEnter),
    ]
    .boxed()
}

pub(super) fn perf_class_strategy() -> BoxedStrategy<Option<BufferPerfClass>> {
    prop_oneof![
        Just(None),
        Just(Some(BufferPerfClass::Full)),
        Just(Some(BufferPerfClass::FastMotion)),
        Just(Some(BufferPerfClass::Skip)),
    ]
    .boxed()
}

pub(super) fn snapshot_with_state(
    enabled: bool,
    buffer_perf_class: Option<BufferPerfClass>,
    tracked_cursor: Option<TrackedCursor>,
) -> IngressReadSnapshot {
    IngressReadSnapshot::new_for_test(IngressReadSnapshotTestInput {
        enabled,
        needs_initialize: false,
        current_corners: [RenderPoint::ZERO; 4],
        target_corners: [RenderPoint::ZERO; 4],
        target_position: RenderPoint::ZERO,
        tracked_cursor,
        mode_flags: [true, true, true, true],
        buffer_perf_mode: BufferPerfMode::Auto,
        callback_duration_estimate_ms: 0.0,
        current_buffer_perf_class: buffer_perf_class,
        filetypes_disabled: Vec::new(),
    })
}

pub(super) fn fast_path_snapshot(
    enabled: bool,
    needs_initialize: bool,
    tracked_cursor: Option<TrackedCursor>,
    target_position: RenderPoint,
) -> CursorAutocmdFastPathSnapshot {
    CursorAutocmdFastPathSnapshot {
        enabled,
        needs_initialize,
        tracked_cursor,
        target_position,
        smear_to_cmd: true,
    }
}

pub(super) fn reset_buffer_local_cache_state() {
    mutate_engine_state(|state| {
        state.shell.reset_transient_caches();
    })
    .expect("engine state access should succeed");
}
