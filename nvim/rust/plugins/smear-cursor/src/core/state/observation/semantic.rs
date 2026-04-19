use super::super::ExternalDemandKind;
use super::cursor_context::CursorTextContext;
use super::cursor_context::ObservedTextRow;
use super::snapshot::ObservationBasis;
use super::snapshot::ObservationSnapshot;

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub(crate) enum SemanticEvent {
    #[default]
    FrameCommitted,
    ModeChanged,
    CursorMovedWithoutTextMutation,
    TextMutatedAtCursorContext,
    ViewportOrWindowMoved,
}

fn cursor_motion_detected(previous: &ObservationBasis, current: &ObservationBasis) -> bool {
    previous.cursor_position() != current.cursor_position()
        || previous.cursor().buffer_line() != current.cursor().buffer_line()
}

fn viewport_or_window_moved(previous: &ObservationBasis, current: &ObservationBasis) -> bool {
    let previous_surface = previous.surface();
    let current_surface = current.surface();
    previous_surface.id() != current_surface.id()
        || previous_surface.top_buffer_line() != current_surface.top_buffer_line()
        || previous_surface.left_col0() != current_surface.left_col0()
        || previous_surface.text_offset0() != current_surface.text_offset0()
        || previous_surface.window_origin() != current_surface.window_origin()
        || previous_surface.window_size() != current_surface.window_size()
        || previous.viewport() != current.viewport()
}

fn text_mutated_at_cursor_context(
    previous: Option<&CursorTextContext>,
    current: Option<&CursorTextContext>,
) -> bool {
    let (Some(previous), Some(current)) = (previous, current) else {
        return false;
    };
    if previous.buffer_handle() != current.buffer_handle()
        || previous.changedtick() == current.changedtick()
    {
        return false;
    }

    // Surprising: line numbers drift after insertions and deletions above the cursor, so text
    // mutation detection compares the last committed footprint against a footprint sampled around
    // the runtime's previously tracked cursor line instead of trusting absolute line numbers.
    current.tracked_nearby_rows().is_some_and(|tracked_rows| {
        rows_differ_by_relative_offset(previous.nearby_rows(), tracked_rows)
    }) || (previous.cursor_line() == current.cursor_line()
        && rows_differ_by_relative_offset(previous.nearby_rows(), current.nearby_rows()))
}

fn rows_differ_by_relative_offset(
    previous_rows: &[ObservedTextRow],
    current_rows: &[ObservedTextRow],
) -> bool {
    previous_rows.len() != current_rows.len()
        || previous_rows
            .iter()
            .zip(current_rows)
            .any(|(previous_row, current_row)| previous_row.text() != current_row.text())
}

pub(crate) fn classify_semantic_event(
    previous: Option<&ObservationSnapshot>,
    current: &ObservationSnapshot,
) -> SemanticEvent {
    let Some(previous) = previous else {
        return SemanticEvent::FrameCommitted;
    };

    let previous_basis = previous.basis();
    let current_basis = current.basis();
    if current.demand().kind() == ExternalDemandKind::ModeChanged
        || previous_basis.mode() != current_basis.mode()
    {
        return SemanticEvent::ModeChanged;
    }
    if text_mutated_at_cursor_context(
        previous_basis.cursor_text_context_state().sampled(),
        current_basis.cursor_text_context_state().sampled(),
    ) {
        return SemanticEvent::TextMutatedAtCursorContext;
    }
    if viewport_or_window_moved(previous_basis, current_basis) {
        return SemanticEvent::ViewportOrWindowMoved;
    }
    if cursor_motion_detected(previous_basis, current_basis) {
        return SemanticEvent::CursorMovedWithoutTextMutation;
    }

    SemanticEvent::FrameCommitted
}
