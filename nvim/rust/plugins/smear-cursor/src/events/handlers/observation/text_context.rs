use super::base::CurrentEditorSnapshot;
use crate::core::effect::TrackedBufferPosition;
use crate::core::state::CursorTextContext;
use crate::core::state::CursorTextContextBoundary;
use crate::core::state::CursorTextContextState;
use crate::core::state::ObservedTextRow;
use crate::events::probe_cache::CursorTextContextCacheKey;
use crate::events::probe_cache::CursorTextContextCacheLookup;
use crate::events::runtime::cached_cursor_text_context;
use crate::events::runtime::store_cursor_text_context;
use nvim_oxi::Result;
use nvim_oxi::api;
use std::sync::Arc;

fn observed_text_rows(buffer: &api::Buffer, center_line: i64) -> Result<Arc<[ObservedTextRow]>> {
    if center_line < 1 {
        return Ok(Arc::default());
    }

    let start_line = center_line.saturating_sub(1).max(1);
    let end_line = center_line.saturating_add(1);
    let start_index = usize::try_from(start_line.saturating_sub(1)).ok();
    let end_index = usize::try_from(end_line).ok();
    let (Some(start_index), Some(end_index)) = (start_index, end_index) else {
        return Ok(Arc::default());
    };

    let rows = buffer
        .get_lines(start_index..end_index, false)?
        .map(|line| Ok(ObservedTextRow::new(line.to_string_lossy().into_owned())))
        .collect::<Result<Vec<_>>>()?;
    Ok(rows.into())
}

fn tracked_cursor_text_context_line(
    buffer_handle: i64,
    tracked_buffer_position: Option<TrackedBufferPosition>,
) -> Option<i64> {
    tracked_buffer_position.and_then(|position| {
        (position.buffer_handle() == buffer_handle).then_some(position.buffer_line().value())
    })
}

fn should_skip_cursor_text_context_sampling(
    retained_boundary: Option<CursorTextContextBoundary>,
    buffer_handle: i64,
    text_revision: u64,
) -> bool {
    retained_boundary.is_some_and(|boundary| boundary.matches(buffer_handle, text_revision))
}

pub(super) fn current_cursor_text_context(
    editor: &CurrentEditorSnapshot,
    cursor_line: i64,
    tracked_buffer_position: Option<TrackedBufferPosition>,
    retained_boundary: Option<CursorTextContextBoundary>,
) -> Result<CursorTextContextState> {
    if cursor_line < 1 {
        return Ok(CursorTextContextState::Unavailable);
    }

    let Some(buffer) = editor.buffer() else {
        return Ok(CursorTextContextState::Unavailable);
    };

    let buffer_handle = i64::from(buffer.handle());
    let text_revision = editor.current_text_revision()?.value();
    let boundary = Some(CursorTextContextBoundary::new(buffer_handle, text_revision));
    let tracked_line = tracked_cursor_text_context_line(buffer_handle, tracked_buffer_position);
    let cache_key =
        CursorTextContextCacheKey::new(buffer_handle, text_revision, cursor_line, tracked_line);
    match cached_cursor_text_context(&cache_key) {
        Ok(CursorTextContextCacheLookup::Hit(context)) => {
            return Ok(CursorTextContextState::from_parts(context, boundary));
        }
        Ok(CursorTextContextCacheLookup::Miss) => {}
        Err(err) => {
            crate::events::logging::warn(&format!("cursor text context cache read failed: {err}"))
        }
    }
    if should_skip_cursor_text_context_sampling(retained_boundary, buffer_handle, text_revision) {
        return Ok(CursorTextContextState::from_parts(None, boundary));
    }

    // Surprising: embedded Neovim does not expose Neovide's redraw grid here, so semantic
    // mutation detection uses narrow buffer-line snapshots plus a shell-local text revision
    // instead of UI cells.
    let nearby_rows = observed_text_rows(buffer, cursor_line)?;
    let context = if nearby_rows.is_empty() {
        None
    } else {
        let tracked_nearby_rows = match tracked_line {
            Some(tracked_line_number) if tracked_line_number != cursor_line => {
                // Surprising: edits above the cursor renumber absolute lines, so we also sample
                // the previously tracked cursor footprint and compare by relative row order.
                let tracked_rows = observed_text_rows(buffer, tracked_line_number)?;
                if tracked_rows.is_empty() {
                    None
                } else {
                    Some(tracked_rows)
                }
            }
            Some(_) => Some(Arc::clone(&nearby_rows)),
            None => None,
        };

        Some(CursorTextContext::from_shared(
            buffer_handle,
            text_revision,
            cursor_line,
            nearby_rows,
            tracked_nearby_rows,
        ))
    };
    if let Err(err) = store_cursor_text_context(cache_key, context.clone()) {
        crate::events::logging::warn(&format!("cursor text context cache write failed: {err}"));
    }
    Ok(CursorTextContextState::from_parts(context, boundary))
}

#[cfg(test)]
mod tests {
    use super::should_skip_cursor_text_context_sampling;
    use crate::core::state::CursorTextContextBoundary;

    #[test]
    fn sampling_skip_only_matches_the_retained_buffer_changedtick_boundary() {
        let retained = Some(CursorTextContextBoundary::new(22, 14));

        assert!(should_skip_cursor_text_context_sampling(retained, 22, 14));
        assert!(!should_skip_cursor_text_context_sampling(retained, 23, 14));
        assert!(!should_skip_cursor_text_context_sampling(retained, 22, 15));
        assert!(!should_skip_cursor_text_context_sampling(None, 22, 14));
    }
}
