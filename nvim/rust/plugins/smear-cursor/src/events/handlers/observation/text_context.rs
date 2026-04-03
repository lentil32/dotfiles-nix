use super::base::current_buffer_changedtick;
use crate::core::state::CursorTextContext;
use crate::core::state::ObservedTextRow;
use crate::events::probe_cache::CursorTextContextCacheKey;
use crate::events::probe_cache::CursorTextContextCacheLookup;
use crate::events::runtime::cached_cursor_text_context;
use crate::events::runtime::store_cursor_text_context;
use crate::state::CursorLocation;
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
        .enumerate()
        .map(|(offset, line)| {
            let relative_line = match i64::try_from(offset) {
                Ok(offset_line) => start_line.saturating_add(offset_line),
                Err(_) => start_line,
            };
            Ok(ObservedTextRow::new(
                relative_line,
                line.to_string_lossy().into_owned(),
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(rows.into())
}

fn tracked_cursor_text_context_line(
    buffer_handle: i64,
    tracked_location: Option<&CursorLocation>,
) -> Option<i64> {
    tracked_location.and_then(|location| {
        (location.buffer_handle == buffer_handle && location.line >= 1).then_some(location.line)
    })
}

pub(super) fn current_cursor_text_context(
    cursor_line: i64,
    tracked_location: Option<&CursorLocation>,
) -> Result<Option<CursorTextContext>> {
    if cursor_line < 1 {
        return Ok(None);
    }

    let buffer = api::get_current_buf();
    if !buffer.is_valid() {
        return Ok(None);
    }

    let buffer_handle = i64::from(buffer.handle());
    let changedtick = current_buffer_changedtick(buffer_handle)?;
    let tracked_cursor_line = tracked_cursor_text_context_line(buffer_handle, tracked_location);
    let cache_key = CursorTextContextCacheKey::new(
        buffer_handle,
        changedtick,
        cursor_line,
        tracked_cursor_line,
    );
    match cached_cursor_text_context(&cache_key) {
        Ok(CursorTextContextCacheLookup::Hit(context)) => return Ok(context),
        Ok(CursorTextContextCacheLookup::Miss) => {}
        Err(err) => {
            crate::events::logging::warn(&format!("cursor text context cache read failed: {err}"))
        }
    }

    // Surprising: embedded Neovim does not expose Neovide's redraw grid here, so semantic
    // mutation detection uses narrow buffer-line snapshots plus changedtick instead of UI cells.
    let nearby_rows = observed_text_rows(&buffer, cursor_line)?;
    let context = if nearby_rows.is_empty() {
        None
    } else {
        let (tracked_cursor_line, tracked_nearby_rows) = match tracked_cursor_line {
            Some(tracked_cursor_line) if tracked_cursor_line != cursor_line => {
                // Surprising: edits above the cursor renumber absolute lines, so we also sample
                // the previously tracked cursor footprint and compare by relative row order.
                let tracked_rows = observed_text_rows(&buffer, tracked_cursor_line)?;
                if tracked_rows.is_empty() {
                    (None, None)
                } else {
                    (Some(tracked_cursor_line), Some(tracked_rows))
                }
            }
            Some(_) => (Some(cursor_line), Some(Arc::clone(&nearby_rows))),
            None => (None, None),
        };

        Some(CursorTextContext::from_shared(
            buffer_handle,
            changedtick,
            cursor_line,
            nearby_rows,
            tracked_cursor_line,
            tracked_nearby_rows,
        ))
    };
    if let Err(err) = store_cursor_text_context(cache_key, context.clone()) {
        crate::events::logging::warn(&format!("cursor text context cache write failed: {err}"));
    }
    Ok(context)
}
