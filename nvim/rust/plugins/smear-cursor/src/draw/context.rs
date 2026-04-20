//! Thread-local draw bookkeeping detached during mutation so shell callbacks stay re-entrant.

use super::prepaint::PrepaintOverlay;
use super::window_pool;
use nvim_oxi::api;
use std::cell::RefCell;
use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::panic::catch_unwind;
use std::panic::resume_unwind;

#[derive(Debug)]
struct DrawContext {
    render_tabs: HashMap<i32, window_pool::TabWindows>,
    prepaint_by_tab: HashMap<i32, PrepaintOverlay>,
}

impl DrawContext {
    fn new() -> Self {
        Self {
            render_tabs: HashMap::with_capacity(4),
            prepaint_by_tab: HashMap::with_capacity(2),
        }
    }
}

thread_local! {
    static DRAW_CONTEXT: RefCell<DrawContext> = RefCell::new(DrawContext::new());
}

pub(crate) fn log_draw_error(context: &str, err: &impl std::fmt::Display) {
    api::err_writeln(&format!("[smear_cursor][draw] {context} failed: {err}"));
}

pub(super) fn take_render_tabs() -> HashMap<i32, window_pool::TabWindows> {
    // Detach the tracked tabs before mutating them so any later shell work runs after the
    // RefCell borrow is released. Re-entrant draw recovery should operate on detached state.
    DRAW_CONTEXT.with(|context| std::mem::take(&mut context.borrow_mut().render_tabs))
}

pub(super) fn restore_render_tabs(render_tabs: HashMap<i32, window_pool::TabWindows>) {
    DRAW_CONTEXT.with(|context| {
        context.borrow_mut().render_tabs = render_tabs;
    });
}

pub(super) fn take_prepaint_by_tab() -> HashMap<i32, PrepaintOverlay> {
    // Detach the tracked overlays before mutating them so any later shell work runs after the
    // RefCell borrow is released. Re-entrant draw recovery should operate on detached state.
    DRAW_CONTEXT.with(|context| std::mem::take(&mut context.borrow_mut().prepaint_by_tab))
}

fn restore_prepaint_by_tab(prepaint_by_tab: HashMap<i32, PrepaintOverlay>) {
    DRAW_CONTEXT.with(|context| {
        context.borrow_mut().prepaint_by_tab = prepaint_by_tab;
    });
}

pub(super) fn with_render_tabs<R>(
    mutator: impl FnOnce(&mut HashMap<i32, window_pool::TabWindows>) -> R,
) -> R {
    let mut render_tabs = take_render_tabs();
    match catch_unwind(AssertUnwindSafe(|| mutator(&mut render_tabs))) {
        Ok(output) => {
            restore_render_tabs(render_tabs);
            output
        }
        Err(panic_payload) => {
            restore_render_tabs(HashMap::with_capacity(4));
            resume_unwind(panic_payload);
        }
    }
}

pub(super) fn with_prepaint_by_tab<R>(
    mutator: impl FnOnce(&mut HashMap<i32, PrepaintOverlay>) -> R,
) -> R {
    // Prepaint overlays follow the same detach-mutate-restore pattern as render tabs so shell
    // callbacks never run while the DRAW_CONTEXT RefCell itself is mutably borrowed.
    let mut prepaint_by_tab = take_prepaint_by_tab();
    match catch_unwind(AssertUnwindSafe(|| mutator(&mut prepaint_by_tab))) {
        Ok(output) => {
            restore_prepaint_by_tab(prepaint_by_tab);
            output
        }
        Err(panic_payload) => {
            restore_prepaint_by_tab(HashMap::with_capacity(2));
            resume_unwind(panic_payload);
        }
    }
}

pub(crate) fn with_render_tab<T>(
    tab_handle: i32,
    mutator: impl FnOnce(&mut window_pool::TabWindows) -> T,
) -> T {
    with_render_tabs(|render_tabs| {
        let tab_windows = render_tabs.entry(tab_handle).or_default();
        mutator(tab_windows)
    })
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct RenderPoolDiagnostics {
    pub(crate) total_windows: usize,
    pub(crate) available_windows: usize,
    pub(crate) in_use_windows: usize,
    pub(crate) visible_windows: usize,
    pub(crate) cached_budget: usize,
    pub(crate) peak_total_windows: usize,
    pub(crate) peak_frame_demand: usize,
    pub(crate) peak_requested_capacity: usize,
    pub(crate) capacity_cap_hits: usize,
}

pub(crate) fn render_pool_diagnostics() -> RenderPoolDiagnostics {
    with_render_tabs(|render_tabs| {
        let mut diagnostics = RenderPoolDiagnostics::default();
        for tab_windows in render_tabs.values() {
            let snapshot = window_pool::tab_pool_snapshot_from_tab(tab_windows);
            diagnostics.total_windows = diagnostics
                .total_windows
                .saturating_add(snapshot.total_windows);
            diagnostics.available_windows = diagnostics
                .available_windows
                .saturating_add(snapshot.available_windows);
            diagnostics.in_use_windows = diagnostics
                .in_use_windows
                .saturating_add(snapshot.in_use_windows);
            diagnostics.visible_windows = diagnostics
                .visible_windows
                .saturating_add(window_pool::tab_visible_window_count_from_tab(tab_windows));
            diagnostics.cached_budget = diagnostics
                .cached_budget
                .saturating_add(snapshot.cached_budget);
            diagnostics.peak_total_windows = diagnostics
                .peak_total_windows
                .max(snapshot.peak_total_windows);
            diagnostics.peak_frame_demand = diagnostics
                .peak_frame_demand
                .max(snapshot.peak_frame_demand);
            diagnostics.peak_requested_capacity = diagnostics
                .peak_requested_capacity
                .max(snapshot.peak_requested_capacity);
            diagnostics.capacity_cap_hits = diagnostics
                .capacity_cap_hits
                .saturating_add(snapshot.capacity_cap_hits);
        }
        diagnostics
    })
}

#[cfg(test)]
pub(super) fn render_tab_handles_for_test() -> Vec<i32> {
    DRAW_CONTEXT.with(|context| {
        let context = context.borrow();
        let mut handles = context.render_tabs.keys().copied().collect::<Vec<_>>();
        handles.sort_unstable();
        handles
    })
}

#[cfg(test)]
pub(super) fn take_render_tabs_for_test() -> Vec<(i32, window_pool::TabWindows)> {
    let mut render_tabs = take_render_tabs().into_iter().collect::<Vec<_>>();
    render_tabs.sort_unstable_by_key(|(tab_handle, _)| *tab_handle);
    render_tabs
}

#[cfg(test)]
pub(super) fn clear_draw_context_for_test() {
    restore_render_tabs(HashMap::with_capacity(4));
    restore_prepaint_by_tab(HashMap::with_capacity(2));
}

#[cfg(test)]
mod tests {
    use super::render_pool_diagnostics;
    use super::render_tab_handles_for_test;
    use super::take_render_tabs_for_test;
    use super::with_render_tab;
    use crate::draw::test_support::with_isolated_draw_context;
    use crate::draw::window_pool::WindowBufferHandle;
    use crate::draw::window_pool::WindowPlacement;
    use pretty_assertions::assert_eq;

    #[test]
    fn render_pool_diagnostics_aggregates_window_counts_across_tabs() {
        with_isolated_draw_context(|| {
            let placement_a = WindowPlacement {
                row: 1,
                col: 2,
                width: 1,
                zindex: 40,
            };
            let placement_b = WindowPlacement {
                row: 3,
                col: 4,
                width: 1,
                zindex: 50,
            };

            with_render_tab(11, |tab_windows| {
                tab_windows.push_test_visible_window(
                    WindowBufferHandle {
                        window_id: 101,
                        buffer_id: 201,
                    },
                    placement_a,
                    1,
                );
                tab_windows.push_test_visible_window(
                    WindowBufferHandle {
                        window_id: 102,
                        buffer_id: 202,
                    },
                    placement_b,
                    2,
                );
            });
            with_render_tab(22, |tab_windows| {
                tab_windows.push_test_visible_window(
                    WindowBufferHandle {
                        window_id: 103,
                        buffer_id: 203,
                    },
                    placement_a,
                    3,
                );
            });

            let diagnostics = render_pool_diagnostics();

            assert_eq!(diagnostics.total_windows, 3);
            assert_eq!(diagnostics.available_windows, 3);
            assert_eq!(diagnostics.in_use_windows, 0);
            assert_eq!(diagnostics.visible_windows, 3);
            assert_eq!(diagnostics.cached_budget, 64);
            assert_eq!(diagnostics.peak_total_windows, 2);
            assert_eq!(diagnostics.peak_frame_demand, 0);
            assert_eq!(diagnostics.peak_requested_capacity, 0);
            assert_eq!(diagnostics.capacity_cap_hits, 0);
        });
    }

    #[test]
    fn render_tab_tracking_is_isolated_by_tab_handle() {
        with_isolated_draw_context(|| {
            with_render_tab(11, |tab_windows| tab_windows.cache_payload(91, 111));
            with_render_tab(22, |tab_windows| tab_windows.cache_payload(91, 222));

            assert!(with_render_tab(11, |tab_windows| tab_windows
                .cached_payload_matches(91, 111)));
            assert!(!with_render_tab(11, |tab_windows| tab_windows
                .cached_payload_matches(91, 222)));
            assert!(with_render_tab(22, |tab_windows| tab_windows
                .cached_payload_matches(91, 222)));
            assert_eq!(render_tab_handles_for_test(), vec![11, 22]);
        });
    }

    #[test]
    fn draining_render_tab_tracking_preserves_tab_owned_state_before_registry_clear() {
        with_isolated_draw_context(|| {
            with_render_tab(9, |tab_windows| tab_windows.cache_payload(41, 401));
            with_render_tab(3, |tab_windows| tab_windows.cache_payload(42, 402));

            let drained = take_render_tabs_for_test();
            let drained_handles = drained
                .iter()
                .map(|(tab_handle, _)| *tab_handle)
                .collect::<Vec<_>>();
            assert_eq!(drained_handles, vec![3, 9]);

            let drained_payloads = drained
                .iter()
                .map(|(tab_handle, tab_windows)| {
                    let cached_payload = match *tab_handle {
                        3 => tab_windows.cached_payload_matches(42, 402),
                        9 => tab_windows.cached_payload_matches(41, 401),
                        other => panic!("unexpected tab handle in drained render tabs: {other}"),
                    };
                    (*tab_handle, cached_payload)
                })
                .collect::<Vec<_>>();
            assert_eq!(drained_payloads, vec![(3, true), (9, true)]);
            assert!(render_tab_handles_for_test().is_empty());
        });
    }
}
