use super::super::AutocmdDispatchContext;
use super::super::DeferredTeardownEffect;
use super::super::on_teardown_autocmd_ingress;
use super::reset_buffer_local_cache_state;
use crate::events::cursor::BufferMetadata;
use crate::events::ingress::TeardownAutocmdIngress;
use crate::events::policy::BufferEventPolicy;
use crate::events::runtime::mutate_shell_state;
use crate::events::runtime::read_shell_state;
use crate::host::HostTabSnapshot;
use crate::host::TabHandle;
use pretty_assertions::assert_eq;

fn tab_snapshot(tab_number: u32, tab_handle: i32) -> HostTabSnapshot {
    HostTabSnapshot {
        tab_handle: TabHandle::from_raw_for_test(tab_handle),
        tab_number: Some(tab_number),
    }
}

#[test]
fn tabclosed_shell_phase_returns_deferred_effect_from_registry_without_host_cleanup() {
    mutate_shell_state(|state| {
        state.tab_page_registry.record_snapshot(tab_snapshot(1, 11));
        state.tab_page_registry.record_snapshot(tab_snapshot(2, 22));
    })
    .expect("runtime access should succeed");

    let dispatch = on_teardown_autocmd_ingress(
        TeardownAutocmdIngress::TabClosed,
        AutocmdDispatchContext {
            file_name: Some("2"),
            ..AutocmdDispatchContext::default()
        },
    )
    .expect("teardown dispatch should parse cached tab");

    assert_eq!(
        dispatch.deferred_effect(),
        Some(DeferredTeardownEffect::ClosedTab {
            tab_handle: TabHandle::from_raw_for_test(22),
        })
    );
}

#[test]
fn winclosed_shell_phase_returns_deferred_effect_from_match_name() {
    let dispatch = on_teardown_autocmd_ingress(
        TeardownAutocmdIngress::WinClosed,
        AutocmdDispatchContext {
            match_name: Some("81"),
            ..AutocmdDispatchContext::default()
        },
    )
    .expect("teardown dispatch should parse closed window id");

    assert_eq!(
        dispatch.deferred_effect(),
        Some(DeferredTeardownEffect::ClosedWindow { window_id: 81 })
    );
}

#[test]
fn bufwipeout_shell_phase_invalidates_buffer_caches_without_deferred_cleanup() {
    const TARGET_BUFFER_HANDLE: i64 = 13;
    const OTHER_BUFFER_HANDLE: i64 = 31;

    reset_buffer_local_cache_state();
    let target_metadata = BufferMetadata::new_for_test("lua", "", true, 120);
    let other_metadata = BufferMetadata::new_for_test("rust", "terminal", false, 14);
    let target_policy = BufferEventPolicy::from_buffer_metadata("", true, 120, 0.0);
    let other_policy = BufferEventPolicy::from_buffer_metadata("terminal", false, 14, 0.0);
    mutate_shell_state(|state| {
        state
            .buffer_metadata_cache
            .store_for_test(TARGET_BUFFER_HANDLE, target_metadata);
        state
            .buffer_metadata_cache
            .store_for_test(OTHER_BUFFER_HANDLE, other_metadata.clone());
        state
            .buffer_perf_policy_cache
            .store_policy(TARGET_BUFFER_HANDLE, target_policy);
        state
            .buffer_perf_policy_cache
            .store_policy(OTHER_BUFFER_HANDLE, other_policy);
    })
    .expect("runtime access should succeed");

    let dispatch = on_teardown_autocmd_ingress(
        TeardownAutocmdIngress::BufWipeout,
        AutocmdDispatchContext {
            buffer_handle: crate::host::BufferHandle::new(TARGET_BUFFER_HANDLE),
            ..AutocmdDispatchContext::default()
        },
    )
    .expect("bufwipeout shell phase should invalidate caches");

    let cached_entries = read_shell_state(|state| {
        (
            state
                .buffer_metadata_cache
                .cached_entry_for_test(TARGET_BUFFER_HANDLE),
            state
                .buffer_metadata_cache
                .cached_entry_for_test(OTHER_BUFFER_HANDLE),
            state
                .buffer_perf_policy_cache
                .cached_policy(TARGET_BUFFER_HANDLE),
            state
                .buffer_perf_policy_cache
                .cached_policy(OTHER_BUFFER_HANDLE),
        )
    })
    .expect("runtime access should succeed");

    assert_eq!(
        (dispatch.deferred_effect(), cached_entries),
        (None, (None, Some(other_metadata), None, Some(other_policy)))
    );
}

#[test]
fn malformed_teardown_payload_does_not_schedule_deferred_cleanup() {
    let tab_dispatch = on_teardown_autocmd_ingress(
        TeardownAutocmdIngress::TabClosed,
        AutocmdDispatchContext {
            file_name: Some("not-a-tab"),
            ..AutocmdDispatchContext::default()
        },
    )
    .expect("malformed tab payload should be dropped");
    let win_dispatch = on_teardown_autocmd_ingress(
        TeardownAutocmdIngress::WinClosed,
        AutocmdDispatchContext {
            match_name: Some("not-a-window"),
            ..AutocmdDispatchContext::default()
        },
    )
    .expect("malformed window payload should be dropped");

    assert_eq!(
        (
            tab_dispatch.deferred_effect(),
            win_dispatch.deferred_effect()
        ),
        (None, None)
    );
}

#[test]
fn teardown_shell_phase_module_has_no_direct_host_cleanup_calls() {
    let source = include_str!("../teardown_autocmd.rs");
    let forbidden_fragments = [
        "ensure_namespace_id",
        "prune_closed_window_resources",
        "prune_stale_tab_resources",
        "schedule_guarded",
        "DrawResourcePort",
        "NeovimHost",
        "list_tabpages",
        "list_wins",
        "list_bufs",
        "window_is_valid",
        "buffer_is_valid",
        "close_window_force",
        "delete_buffer_force",
        "set_eventignore",
    ];

    let violations = forbidden_fragments
        .iter()
        .copied()
        .filter(|fragment| source.contains(fragment))
        .collect::<Vec<_>>();

    assert_eq!(violations, Vec::<&str>::new());
}
