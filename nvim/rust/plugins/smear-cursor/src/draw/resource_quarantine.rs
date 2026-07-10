//! Exact-handle recovery for staged floating resources whose RAII teardown failed.

use super::floating_windows::EventIgnoreGuard;
use super::floating_windows::close_floating_window_with_autocmds_suppressed;
use super::floating_windows::delete_floating_buffer_with_autocmds_suppressed;
use super::resource_close::TrackedResourceCloseOutcome;
use crate::host::BufferHandle;
use crate::host::DrawResourcePort;
use crate::host::NamespaceId;
use crate::host::NeovimHost;
use std::collections::BTreeSet;

#[derive(Debug, Default)]
pub(crate) struct ResourceQuarantine {
    window_ids: BTreeSet<i32>,
    buffer_handles: BTreeSet<BufferHandle>,
}

impl ResourceQuarantine {
    fn is_empty(&self) -> bool {
        self.window_ids.is_empty() && self.buffer_handles.is_empty()
    }

    pub(crate) fn merge(&mut self, mut other: Self) {
        if self.is_empty() {
            *self = other;
            return;
        }
        self.window_ids.append(&mut other.window_ids);
        self.buffer_handles.append(&mut other.buffer_handles);
    }
}

pub(super) fn quarantine_window(window_id: i32) {
    if window_id > 0 {
        super::context::with_resource_quarantine(|quarantine| {
            quarantine.window_ids.insert(window_id);
        });
    }
}

pub(super) fn quarantine_buffer(buffer_handle: BufferHandle) {
    if buffer_handle.is_valid() {
        super::context::with_resource_quarantine(|quarantine| {
            quarantine.buffer_handles.insert(buffer_handle);
        });
    }
}

pub(super) fn has_quarantined_resources() -> bool {
    super::context::with_resource_quarantine(|quarantine| !quarantine.is_empty())
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct QuarantinedResourcePurgeSummary {
    pub(super) attempted_resources: usize,
    pub(super) cleared_resources: usize,
    pub(super) closed_windows: usize,
    pub(super) retained_resources: usize,
    pub(super) pending_resources: usize,
    pub(super) potential_visual_change: bool,
    pub(super) close_stalled: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RetainedPurgePolicy {
    Stop,
    Continue,
}

impl RetainedPurgePolicy {
    const fn stops_after_retained(self) -> bool {
        matches!(self, Self::Stop)
    }
}

pub(super) fn purge_quarantined_resources(
    namespace_id: NamespaceId,
) -> QuarantinedResourcePurgeSummary {
    purge_quarantined_resources_with(&NeovimHost, namespace_id)
}

pub(super) fn purge_quarantined_resources_up_to(
    namespace_id: NamespaceId,
    max_attempts: usize,
) -> QuarantinedResourcePurgeSummary {
    purge_quarantined_resources_with_limit(&NeovimHost, namespace_id, max_attempts)
}

fn purge_quarantined_resources_with(
    host: &impl DrawResourcePort,
    namespace_id: NamespaceId,
) -> QuarantinedResourcePurgeSummary {
    purge_quarantined_resources_with_policy(
        host,
        namespace_id,
        usize::MAX,
        RetainedPurgePolicy::Continue,
    )
}

fn purge_quarantined_resources_with_limit(
    host: &impl DrawResourcePort,
    namespace_id: NamespaceId,
    max_attempts: usize,
) -> QuarantinedResourcePurgeSummary {
    purge_quarantined_resources_with_policy(
        host,
        namespace_id,
        max_attempts,
        RetainedPurgePolicy::Stop,
    )
}

fn purge_quarantined_resources_with_policy(
    host: &impl DrawResourcePort,
    namespace_id: NamespaceId,
    max_attempts: usize,
    retained_policy: RetainedPurgePolicy,
) -> QuarantinedResourcePurgeSummary {
    let mut quarantine = super::context::take_resource_quarantine();
    if quarantine.is_empty() {
        return QuarantinedResourcePurgeSummary::default();
    }
    if max_attempts == 0 {
        let pending_resources = quarantine
            .window_ids
            .len()
            .saturating_add(quarantine.buffer_handles.len());
        super::context::restore_resource_quarantine(quarantine);
        return QuarantinedResourcePurgeSummary {
            pending_resources,
            ..QuarantinedResourcePurgeSummary::default()
        };
    }

    let _event_ignore = EventIgnoreGuard::set_all_with(host);
    let mut retained = ResourceQuarantine::default();
    let mut summary = QuarantinedResourcePurgeSummary::default();
    while summary.attempted_resources < max_attempts {
        let Some(window_id) = quarantine.window_ids.pop_first() else {
            break;
        };
        summary.attempted_resources = summary.attempted_resources.saturating_add(1);
        summary.potential_visual_change = true;
        let Some(window) = host.valid_window_i32(window_id) else {
            summary.cleared_resources = summary.cleared_resources.saturating_add(1);
            continue;
        };
        let outcome = close_floating_window_with_autocmds_suppressed(
            host,
            window,
            "close quarantined floating window",
        );
        match outcome {
            TrackedResourceCloseOutcome::ClosedOrGone => {
                summary.closed_windows = summary.closed_windows.saturating_add(1);
                summary.cleared_resources = summary.cleared_resources.saturating_add(1);
            }
            TrackedResourceCloseOutcome::Retained => {
                retained.window_ids.insert(window_id);
                summary.retained_resources = summary.retained_resources.saturating_add(1);
                summary.close_stalled = true;
                if retained_policy.stops_after_retained() {
                    break;
                }
            }
        }
    }

    while (!summary.close_stalled || !retained_policy.stops_after_retained())
        && summary.attempted_resources < max_attempts
    {
        let Some(buffer_handle) = quarantine.buffer_handles.pop_first() else {
            break;
        };
        summary.attempted_resources = summary.attempted_resources.saturating_add(1);
        summary.potential_visual_change = true;
        let Some(mut buffer) = host.valid_buffer(buffer_handle) else {
            summary.cleared_resources = summary.cleared_resources.saturating_add(1);
            continue;
        };
        if let Err(err) = host.clear_buffer_namespace(&mut buffer, namespace_id) {
            super::context::log_draw_error_with(host, "clear quarantined floating namespace", &err);
        }
        if delete_floating_buffer_with_autocmds_suppressed(
            host,
            buffer,
            "delete quarantined floating buffer",
        )
        .should_retain()
        {
            retained.buffer_handles.insert(buffer_handle);
            summary.retained_resources = summary.retained_resources.saturating_add(1);
            summary.close_stalled = true;
            if retained_policy.stops_after_retained() {
                break;
            }
            continue;
        }
        summary.cleared_resources = summary.cleared_resources.saturating_add(1);
    }

    retained.window_ids.append(&mut quarantine.window_ids);
    retained
        .buffer_handles
        .append(&mut quarantine.buffer_handles);
    summary.pending_resources = retained
        .window_ids
        .len()
        .saturating_add(retained.buffer_handles.len());
    super::context::restore_resource_quarantine(retained);
    summary
}

#[cfg(test)]
pub(super) fn clear_resource_quarantine_for_test() {
    let _ = super::context::take_resource_quarantine();
}

#[cfg(test)]
pub(super) fn resource_quarantine_snapshot_for_test() -> (Vec<i32>, Vec<BufferHandle>) {
    super::context::with_resource_quarantine(|quarantine| {
        (
            quarantine.window_ids.iter().copied().collect(),
            quarantine.buffer_handles.iter().copied().collect(),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::QuarantinedResourcePurgeSummary;
    use super::clear_resource_quarantine_for_test;
    use super::has_quarantined_resources;
    use super::purge_quarantined_resources_with;
    use super::purge_quarantined_resources_with_limit;
    use super::quarantine_buffer;
    use super::quarantine_window;
    use crate::host::BufferHandle;
    use crate::host::DrawResourceCall;
    use crate::host::FakeDrawResourcePort;
    use crate::host::NamespaceId;
    use pretty_assertions::assert_eq;

    #[test]
    fn exact_handle_quarantine_deduplicates_and_purges_only_recorded_resources() {
        clear_resource_quarantine_for_test();
        let host = FakeDrawResourcePort::default();
        host.push_eventignore("previous");
        quarantine_window(/*window_id*/ 7);
        quarantine_window(/*window_id*/ 7);
        quarantine_buffer(BufferHandle::from_raw_for_test(/*value*/ 11));
        quarantine_buffer(BufferHandle::from_raw_for_test(/*value*/ 11));

        let summary = purge_quarantined_resources_with(&host, NamespaceId::new(/*value*/ 13));

        assert_eq!(
            (summary, has_quarantined_resources(), host.calls()),
            (
                QuarantinedResourcePurgeSummary {
                    attempted_resources: 2,
                    cleared_resources: 2,
                    closed_windows: 1,
                    retained_resources: 0,
                    pending_resources: 0,
                    potential_visual_change: true,
                    close_stalled: false,
                },
                false,
                vec![
                    DrawResourceCall::Eventignore,
                    DrawResourceCall::SetEventignore {
                        value: "all".to_owned(),
                    },
                    DrawResourceCall::CloseWindowForce { window_id: 7 },
                    DrawResourceCall::ClearBufferNamespace {
                        buffer: BufferHandle::from_raw_for_test(/*value*/ 11),
                        namespace_id: NamespaceId::new(/*value*/ 13),
                    },
                    DrawResourceCall::DeleteBufferForce {
                        buffer: BufferHandle::from_raw_for_test(/*value*/ 11),
                    },
                    DrawResourceCall::SetEventignore {
                        value: "previous".to_owned(),
                    },
                ],
            ),
        );
    }

    #[test]
    fn full_quarantine_purge_continues_after_failure_and_retries_only_retained_handles() {
        clear_resource_quarantine_for_test();
        let host = FakeDrawResourcePort::default();
        host.push_eventignore("previous");
        host.push_eventignore("previous");
        host.fail_next_window_close();
        quarantine_window(/*window_id*/ 17);
        quarantine_buffer(BufferHandle::from_raw_for_test(/*value*/ 19));

        let first = purge_quarantined_resources_with(&host, NamespaceId::new(/*value*/ 23));
        let retained_after_first = has_quarantined_resources();
        let second = purge_quarantined_resources_with(&host, NamespaceId::new(/*value*/ 23));

        assert_eq!(
            (
                first,
                retained_after_first,
                second,
                has_quarantined_resources(),
            ),
            (
                QuarantinedResourcePurgeSummary {
                    attempted_resources: 2,
                    cleared_resources: 1,
                    closed_windows: 0,
                    retained_resources: 1,
                    pending_resources: 1,
                    potential_visual_change: true,
                    close_stalled: true,
                },
                true,
                QuarantinedResourcePurgeSummary {
                    attempted_resources: 1,
                    cleared_resources: 1,
                    closed_windows: 1,
                    retained_resources: 0,
                    pending_resources: 0,
                    potential_visual_change: true,
                    close_stalled: false,
                },
                false,
            ),
        );
    }

    #[test]
    fn failed_quarantined_buffer_delete_retries_the_exact_handle() {
        clear_resource_quarantine_for_test();
        let host = FakeDrawResourcePort::default();
        host.push_eventignore("previous");
        host.push_eventignore("previous");
        host.fail_next_buffer_delete();
        quarantine_buffer(BufferHandle::from_raw_for_test(/*value*/ 19));

        let first = purge_quarantined_resources_with(&host, NamespaceId::new(/*value*/ 23));
        let retained_after_first = has_quarantined_resources();
        let second = purge_quarantined_resources_with(&host, NamespaceId::new(/*value*/ 23));

        assert_eq!(
            (
                first,
                retained_after_first,
                second,
                has_quarantined_resources(),
            ),
            (
                QuarantinedResourcePurgeSummary {
                    attempted_resources: 1,
                    cleared_resources: 0,
                    closed_windows: 0,
                    retained_resources: 1,
                    pending_resources: 1,
                    potential_visual_change: true,
                    close_stalled: true,
                },
                true,
                QuarantinedResourcePurgeSummary {
                    attempted_resources: 1,
                    cleared_resources: 1,
                    closed_windows: 0,
                    retained_resources: 0,
                    pending_resources: 0,
                    potential_visual_change: true,
                    close_stalled: false,
                },
                false,
            ),
        );
    }

    #[test]
    fn quarantine_purge_respects_the_shared_resource_attempt_budget() {
        clear_resource_quarantine_for_test();
        let host = FakeDrawResourcePort::default();
        host.push_eventignore("previous");
        quarantine_window(/*window_id*/ 7);
        quarantine_buffer(BufferHandle::from_raw_for_test(/*value*/ 11));

        let summary = purge_quarantined_resources_with_limit(
            &host,
            NamespaceId::new(/*value*/ 13),
            /*max_attempts*/ 1,
        );

        assert_eq!(
            (summary, has_quarantined_resources()),
            (
                QuarantinedResourcePurgeSummary {
                    attempted_resources: 1,
                    cleared_resources: 1,
                    closed_windows: 1,
                    retained_resources: 0,
                    pending_resources: 1,
                    potential_visual_change: true,
                    close_stalled: false,
                },
                true,
            ),
        );
    }

    #[test]
    fn bounded_quarantine_purge_stops_after_a_retained_resource() {
        clear_resource_quarantine_for_test();
        let host = FakeDrawResourcePort::default();
        host.push_eventignore("previous");
        host.fail_next_window_close();
        quarantine_window(/*window_id*/ 7);
        quarantine_buffer(BufferHandle::from_raw_for_test(/*value*/ 11));

        let summary = purge_quarantined_resources_with_limit(
            &host,
            NamespaceId::new(/*value*/ 13),
            /*max_attempts*/ 2,
        );

        assert_eq!(
            (summary, has_quarantined_resources()),
            (
                QuarantinedResourcePurgeSummary {
                    attempted_resources: 1,
                    cleared_resources: 0,
                    closed_windows: 0,
                    retained_resources: 1,
                    pending_resources: 2,
                    potential_visual_change: true,
                    close_stalled: true,
                },
                true,
            ),
        );
    }
}
