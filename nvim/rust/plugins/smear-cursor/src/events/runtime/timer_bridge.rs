use super::super::logging::warn;
use super::super::timer_protocol::FiredHostTimer;
use super::super::timer_protocol::HostCallbackId;
use super::super::timer_protocol::HostTimerId;
use crate::core::types::TimerId;
use crate::core::types::TimerSlots;
use crate::core::types::TimerToken;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct CoreTimerHandle {
    pub(crate) host_callback_id: HostCallbackId,
    pub(crate) host_timer_id: HostTimerId,
    pub(crate) token: TimerToken,
}

#[derive(Debug, Default)]
struct CoreTimerHandles {
    // One active host callback per reducer timer kind. The reducer token stays
    // authoritative; the callback id resolves the fired callback and the host
    // timer id remains the cancellation witness.
    slots: TimerSlots<Option<CoreTimerHandle>>,
}

impl CoreTimerHandles {
    fn slot(&self, timer_id: TimerId) -> Option<CoreTimerHandle> {
        self.slots.copied(timer_id)
    }

    fn replace(&mut self, handle: CoreTimerHandle) -> Option<CoreTimerHandle> {
        self.slots.replace(handle.token.id(), Some(handle))
    }

    #[cfg(test)]
    fn has_timer_id(&self, timer_id: TimerId) -> bool {
        self.slot(timer_id).is_some()
    }

    fn clear_all(&mut self) -> Vec<CoreTimerHandle> {
        self.slots.take_all()
    }

    fn all(&self) -> Vec<CoreTimerHandle> {
        TimerId::ALL
            .into_iter()
            .filter_map(|timer_id| self.slot(timer_id))
            .collect()
    }

    #[cfg(test)]
    fn take_by_host_timer_id(&mut self, host_timer_id: HostTimerId) -> Option<CoreTimerHandle> {
        for slot in self.slots.iter_mut() {
            if slot
                .as_ref()
                .is_some_and(|handle| handle.host_timer_id == host_timer_id)
            {
                return slot.take();
            }
        }

        None
    }

    fn resolve_fired(&mut self, fired_timer: FiredHostTimer) -> FiredCoreTimerLookup {
        for timer_id in TimerId::ALL {
            let Some(handle) = self.slot(timer_id) else {
                continue;
            };
            if handle.host_callback_id != fired_timer.host_callback_id() {
                continue;
            }

            if handle.host_timer_id != fired_timer.host_timer_id() {
                return FiredCoreTimerLookup::MismatchedHostTimerId {
                    timer_id,
                    expected: handle.host_timer_id,
                };
            }

            let Some(handle) = self.slots.take(timer_id) else {
                warn("matched host timer slot unexpectedly lost its timer handle");
                return FiredCoreTimerLookup::MissingHandle;
            };
            return FiredCoreTimerLookup::Matched(handle);
        }

        FiredCoreTimerLookup::MissingHandle
    }

    #[cfg(test)]
    fn take_fired(&mut self, fired_timer: FiredHostTimer) -> Option<CoreTimerHandle> {
        match self.resolve_fired(fired_timer) {
            FiredCoreTimerLookup::Matched(handle) => Some(handle),
            FiredCoreTimerLookup::MismatchedHostTimerId { .. }
            | FiredCoreTimerLookup::MissingHandle => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum FiredCoreTimerLookup {
    Matched(CoreTimerHandle),
    MismatchedHostTimerId {
        timer_id: TimerId,
        expected: HostTimerId,
    },
    MissingHandle,
}

#[derive(Debug, Default)]
struct PendingCoreTimerRetries {
    retries: Vec<FiredHostTimer>,
}

impl PendingCoreTimerRetries {
    fn insert(&mut self, retry: FiredHostTimer) -> bool {
        if self.retries.contains(&retry) {
            return false;
        }

        self.retries.push(retry);
        true
    }

    fn remove(&mut self, retry: FiredHostTimer) {
        if let Some(index) = self.retries.iter().position(|pending| *pending == retry) {
            let _ = self.retries.swap_remove(index);
        }
    }

    fn clear(&mut self) {
        self.retries.clear();
    }

    #[cfg(test)]
    pub(super) fn contains(&self, retry: FiredHostTimer) -> bool {
        self.retries.contains(&retry)
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.retries.len()
    }
}

#[derive(Debug, Default)]
pub(super) struct TimerBridge {
    // The reducer owns timer liveness and generations. This bridge owns only
    // shell-side witnesses needed to allocate, cancel, and retry host callbacks.
    handles: CoreTimerHandles,
    pending_retries: PendingCoreTimerRetries,
    next_host_callback_id: u64,
}

impl TimerBridge {
    pub(super) fn allocate_host_callback_id(&mut self) -> HostCallbackId {
        HostCallbackId::next(&mut self.next_host_callback_id)
    }

    pub(crate) fn replace_handle(&mut self, handle: CoreTimerHandle) -> Option<CoreTimerHandle> {
        self.handles.replace(handle)
    }

    pub(crate) fn clear_handles(&mut self) -> Vec<CoreTimerHandle> {
        self.handles.clear_all()
    }

    pub(super) fn resolve_fired(&mut self, fired_timer: FiredHostTimer) -> FiredCoreTimerLookup {
        self.handles.resolve_fired(fired_timer)
    }

    pub(super) fn stage_retry(&mut self, retry: FiredHostTimer) -> TimerRetryTransition {
        if self.pending_retries.insert(retry) {
            TimerRetryTransition::Staged
        } else {
            TimerRetryTransition::Coalesced
        }
    }

    pub(super) fn release_retry(&mut self, retry: FiredHostTimer) {
        self.pending_retries.remove(retry);
    }

    pub(super) fn clear_pending_retries(&mut self) {
        self.pending_retries.clear();
    }

    pub(super) fn reset_transient(&mut self) -> Vec<CoreTimerHandle> {
        let handles = self.clear_handles();
        self.clear_pending_retries();
        self.next_host_callback_id = 0;
        handles
    }

    pub(super) fn clear_recovered_transient(&mut self) {
        let _ = self.clear_handles();
        self.clear_pending_retries();
        self.next_host_callback_id = 0;
    }

    pub(super) fn recovery_state(&self) -> TimerBridgeRecoveryState {
        TimerBridgeRecoveryState {
            core_timer_handles: self.handles.all(),
        }
    }

    #[cfg(test)]
    pub(crate) fn has_timer_id(&self, timer_id: TimerId) -> bool {
        self.handles.has_timer_id(timer_id)
    }

    #[cfg(test)]
    pub(crate) fn take_by_host_timer_id(
        &mut self,
        host_timer_id: HostTimerId,
    ) -> Option<CoreTimerHandle> {
        self.handles.take_by_host_timer_id(host_timer_id)
    }

    #[cfg(test)]
    pub(crate) fn take_fired(&mut self, fired_timer: FiredHostTimer) -> Option<CoreTimerHandle> {
        self.handles.take_fired(fired_timer)
    }

    #[cfg(test)]
    pub(crate) fn pending_retry_contains(&self, retry: FiredHostTimer) -> bool {
        self.pending_retries.contains(retry)
    }

    #[cfg(test)]
    pub(crate) fn pending_retry_len(&self) -> usize {
        self.pending_retries.len()
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum TimerRetryTransition {
    Staged,
    Coalesced,
}

#[derive(Debug, Clone, Default)]
pub(super) struct TimerBridgeRecoveryState {
    pub(super) core_timer_handles: Vec<CoreTimerHandle>,
}
