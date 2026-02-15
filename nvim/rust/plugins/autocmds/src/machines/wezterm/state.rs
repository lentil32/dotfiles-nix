use support::TabTitle;

#[derive(Debug)]
pub(super) struct UpdateSlot<T> {
    pub(super) applied: Option<T>,
    pub(super) in_flight: Option<T>,
    queued: Option<T>,
}

impl<T> UpdateSlot<T> {
    const fn new() -> Self {
        Self {
            applied: None,
            in_flight: None,
            queued: None,
        }
    }

    pub(super) fn clear_pending(&mut self) {
        self.in_flight = None;
        self.queued = None;
    }
}

impl<T> Default for UpdateSlot<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> UpdateSlot<T>
where
    T: Clone + Eq,
{
    pub(super) fn request(&mut self, target: T) -> Option<T> {
        if self.applied.as_ref() == Some(&target)
            || self.in_flight.as_ref() == Some(&target)
            || self.queued.as_ref() == Some(&target)
        {
            return None;
        }
        if self.in_flight.is_some() {
            self.queued = Some(target);
            return None;
        }
        self.in_flight = Some(target.clone());
        Some(target)
    }

    pub(super) fn complete_success(&mut self, target: &T) -> Option<T> {
        if self.in_flight.as_ref() != Some(target) {
            return None;
        }
        self.applied = Some(target.clone());
        self.in_flight = None;
        self.start_queued()
    }

    pub(super) fn complete_failure(&mut self, target: &T) -> Option<T> {
        if self.in_flight.as_ref() != Some(target) {
            return None;
        }
        self.in_flight = None;
        self.start_queued()
    }

    fn start_queued(&mut self) -> Option<T> {
        let queued = self.queued.take()?;
        if self.applied.as_ref() == Some(&queued) {
            return None;
        }
        self.in_flight = Some(queued.clone());
        Some(queued)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FailureChannel {
    Title,
    Cwd,
}

#[derive(Debug, Default)]
pub(super) struct CliWarningGate {
    unavailable_reported: bool,
    title_failed_reported: bool,
    cwd_failed_reported: bool,
}

impl CliWarningGate {
    pub(super) const fn reset_after_success(&mut self, channel: FailureChannel) {
        self.unavailable_reported = false;
        match channel {
            FailureChannel::Title => self.title_failed_reported = false,
            FailureChannel::Cwd => self.cwd_failed_reported = false,
        }
    }

    const fn take_unavailable(&mut self) -> bool {
        if self.unavailable_reported {
            false
        } else {
            self.unavailable_reported = true;
            true
        }
    }

    const fn take_failed(&mut self, channel: FailureChannel) -> bool {
        let failed_reported = match channel {
            FailureChannel::Title => &mut self.title_failed_reported,
            FailureChannel::Cwd => &mut self.cwd_failed_reported,
        };
        if *failed_reported {
            false
        } else {
            *failed_reported = true;
            true
        }
    }
}

#[derive(Debug)]
pub struct WeztermState {
    pub(super) title: UpdateSlot<TabTitle>,
    pub(super) cwd: UpdateSlot<String>,
    pub(super) warning_gate: CliWarningGate,
}

impl WeztermState {
    pub const fn new() -> Self {
        Self {
            title: UpdateSlot::new(),
            cwd: UpdateSlot::new(),
            warning_gate: CliWarningGate {
                unavailable_reported: false,
                title_failed_reported: false,
                cwd_failed_reported: false,
            },
        }
    }

    pub fn clear_pending_updates(&mut self) {
        self.title.clear_pending();
        self.cwd.clear_pending();
    }

    pub const fn take_warn_cli_unavailable(&mut self) -> bool {
        self.warning_gate.take_unavailable()
    }

    pub const fn take_warn_title_failed(&mut self) -> bool {
        self.warning_gate.take_failed(FailureChannel::Title)
    }

    pub const fn take_warn_cwd_failed(&mut self) -> bool {
        self.warning_gate.take_failed(FailureChannel::Cwd)
    }
}

impl Default for WeztermState {
    fn default() -> Self {
        Self::new()
    }
}
