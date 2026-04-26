use super::super::LiveTabSnapshot;
use super::super::advance_buffer_text_revision;
use super::super::invalidate_buffer_local_caches;
use super::super::invalidate_buffer_metadata;
use super::super::parse_closed_window_id;
use super::super::should_invalidate_buffer_metadata_for_option;
use super::super::should_invalidate_conceal_probe_cache_for_option;
use super::super::should_refresh_editor_viewport_for_option;
use super::super::stale_tracked_tab_handles;
use super::reset_buffer_local_cache_state;
use crate::core::types::Generation;
use crate::events::cursor::BufferMetadata;
use crate::events::policy::BufferEventPolicy;
use crate::events::runtime::mutate_shell_state;
use crate::events::runtime::read_shell_state;
use crate::host::TabHandle;
use pretty_assertions::assert_eq;

fn tab_handle(value: i32) -> TabHandle {
    TabHandle::from_raw_for_test(value)
}

#[test]
fn buffer_metadata_invalidation_only_tracks_the_buffer_local_policy_inputs() {
    for (option_name, expected) in [
        ("filetype", true),
        ("buftype", true),
        ("buflisted", true),
        ("conceallevel", false),
        ("number", false),
    ] {
        assert_eq!(
            should_invalidate_buffer_metadata_for_option(option_name),
            expected,
            "unexpected invalidation result for {option_name}"
        );
    }
}

#[test]
fn conceal_probe_cache_invalidation_only_tracks_conceal_window_options() {
    for (option_name, expected) in [
        ("conceallevel", true),
        ("concealcursor", true),
        ("filetype", false),
        ("number", false),
    ] {
        assert_eq!(
            should_invalidate_conceal_probe_cache_for_option(option_name),
            expected,
            "unexpected conceal probe invalidation result for {option_name}"
        );
    }
}

#[test]
fn winclosed_payload_uses_match_name_as_window_id() {
    assert_eq!(parse_closed_window_id(Some("42")), Some(42));
    assert_eq!(parse_closed_window_id(Some("0")), None);
    assert_eq!(parse_closed_window_id(Some("not-a-window")), None);
    assert_eq!(parse_closed_window_id(None), None);
}

#[test]
fn tabclosed_mapping_fallback_drops_all_tracked_handles_absent_from_live_handles() {
    let live_tabs = [
        LiveTabSnapshot {
            tab_handle: tab_handle(11),
            tab_number: None,
        },
        LiveTabSnapshot {
            tab_handle: tab_handle(44),
            tab_number: None,
        },
    ];

    assert_eq!(
        stale_tracked_tab_handles(
            [
                tab_handle(44),
                tab_handle(22),
                tab_handle(11),
                tab_handle(33),
                tab_handle(22),
            ],
            &live_tabs,
        ),
        vec![tab_handle(22), tab_handle(33)]
    );
}

#[test]
fn option_set_metadata_invalidation_drops_only_target_buffer_metadata_and_policy() {
    const TARGET_BUFFER_HANDLE: i64 = 11;
    const OTHER_BUFFER_HANDLE: i64 = 29;

    reset_buffer_local_cache_state();

    let target_metadata = BufferMetadata::new_for_test("lua", "", true, 42);
    let other_metadata = BufferMetadata::new_for_test("rust", "terminal", false, 99);
    let target_policy = BufferEventPolicy::from_buffer_metadata("", true, 42, 0.0);
    let other_policy = BufferEventPolicy::from_buffer_metadata("terminal", false, 99, 0.0);
    mutate_shell_state(|state| {
        state
            .buffer_metadata_cache
            .store_for_test(TARGET_BUFFER_HANDLE, target_metadata.clone());
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

    invalidate_buffer_metadata(TARGET_BUFFER_HANDLE).expect("metadata invalidation should succeed");

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
        cached_entries,
        (None, Some(other_metadata), None, Some(other_policy))
    );
}

#[test]
fn buffer_churn_invalidation_clears_all_target_buffer_local_caches() {
    const TARGET_BUFFER_HANDLE: i64 = 13;
    const OTHER_BUFFER_HANDLE: i64 = 31;

    reset_buffer_local_cache_state();

    let target_metadata = BufferMetadata::new_for_test("lua", "", true, 120);
    let other_metadata = BufferMetadata::new_for_test("rust", "terminal", false, 14);
    let target_policy = BufferEventPolicy::from_buffer_metadata("", true, 120, 0.0);
    let other_policy = BufferEventPolicy::from_buffer_metadata("terminal", false, 14, 0.0);
    let (target_telemetry, other_telemetry) = mutate_shell_state(|state| {
        state
            .buffer_metadata_cache
            .store_for_test(TARGET_BUFFER_HANDLE, target_metadata.clone());
        state
            .buffer_metadata_cache
            .store_for_test(OTHER_BUFFER_HANDLE, other_metadata.clone());
        state
            .buffer_perf_policy_cache
            .store_policy(TARGET_BUFFER_HANDLE, target_policy);
        state
            .buffer_perf_policy_cache
            .store_policy(OTHER_BUFFER_HANDLE, other_policy);
        (
            state
                .buffer_perf_telemetry_cache
                .record_conceal_full_scan(TARGET_BUFFER_HANDLE, 1_000.0),
            state
                .buffer_perf_telemetry_cache
                .record_cursor_color_extmark_fallback(OTHER_BUFFER_HANDLE, 1_500.0),
        )
    })
    .expect("runtime access should succeed");

    invalidate_buffer_local_caches(TARGET_BUFFER_HANDLE)
        .expect("buffer-local cache invalidation should succeed");

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
            state
                .buffer_perf_telemetry_cache
                .telemetry(TARGET_BUFFER_HANDLE),
            state
                .buffer_perf_telemetry_cache
                .telemetry(OTHER_BUFFER_HANDLE),
        )
    })
    .expect("runtime access should succeed");

    assert_eq!(
        cached_entries,
        (
            None,
            Some(other_metadata),
            None,
            Some(other_policy),
            None,
            Some(other_telemetry),
        )
    );

    assert_eq!(target_telemetry.callback_duration_estimate_ms(), 0.0);
}

#[test]
fn text_mutation_revision_advances_only_for_the_target_buffer() {
    const TARGET_BUFFER_HANDLE: i64 = 23;
    const OTHER_BUFFER_HANDLE: i64 = 47;

    reset_buffer_local_cache_state();

    mutate_shell_state(|state| {
        state
            .buffer_text_revision_cache
            .advance(OTHER_BUFFER_HANDLE);
    })
    .expect("runtime access should succeed");

    advance_buffer_text_revision(TARGET_BUFFER_HANDLE)
        .expect("text revision advance should succeed");

    let revisions = mutate_shell_state(|state| {
        (
            state
                .buffer_text_revision_cache
                .cached_entry_for_test(TARGET_BUFFER_HANDLE),
            state
                .buffer_text_revision_cache
                .cached_entry_for_test(OTHER_BUFFER_HANDLE),
        )
    })
    .expect("runtime access should succeed");

    assert_eq!(
        revisions,
        (Some(Generation::new(1)), Some(Generation::new(1)))
    );
}

#[test]
fn editor_viewport_refresh_tracks_only_global_viewport_inputs() {
    for (option_name, expected) in [
        ("cmdheight", true),
        ("lines", true),
        ("columns", true),
        ("filetype", false),
        ("number", false),
    ] {
        assert_eq!(
            should_refresh_editor_viewport_for_option(option_name),
            expected,
            "unexpected viewport refresh result for {option_name}"
        );
    }
}
