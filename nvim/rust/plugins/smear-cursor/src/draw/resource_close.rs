#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TrackedResourceCloseOutcome {
    ClosedOrGone,
    Retained,
}

impl TrackedResourceCloseOutcome {
    pub(crate) fn should_retain(self) -> bool {
        matches!(self, Self::Retained)
    }

    pub(crate) fn merge(self, other: Self) -> Self {
        match (self, other) {
            (Self::Retained, _) | (_, Self::Retained) => Self::Retained,
            (Self::ClosedOrGone, Self::ClosedOrGone) => Self::ClosedOrGone,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TrackedWindowBufferCloseOutcome {
    window: TrackedResourceCloseOutcome,
    buffer: TrackedResourceCloseOutcome,
}

impl TrackedWindowBufferCloseOutcome {
    pub(crate) const fn new(
        window: TrackedResourceCloseOutcome,
        buffer: TrackedResourceCloseOutcome,
    ) -> Self {
        Self { window, buffer }
    }

    pub(crate) const fn closed_or_gone() -> Self {
        Self {
            window: TrackedResourceCloseOutcome::ClosedOrGone,
            buffer: TrackedResourceCloseOutcome::ClosedOrGone,
        }
    }

    pub(crate) fn should_retain(self) -> bool {
        self.window.should_retain() || self.buffer.should_retain()
    }

    pub(crate) fn window_closed_or_gone(self) -> bool {
        matches!(self.window, TrackedResourceCloseOutcome::ClosedOrGone)
    }

    pub(crate) fn aggregate(self) -> TrackedResourceCloseOutcome {
        if self.should_retain() {
            TrackedResourceCloseOutcome::Retained
        } else {
            TrackedResourceCloseOutcome::ClosedOrGone
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct TrackedWindowBufferCloseSummary {
    pub(crate) window_closed_or_gone: usize,
    pub(crate) window_retained: usize,
    pub(crate) buffer_closed_or_gone: usize,
    pub(crate) buffer_retained: usize,
}

impl TrackedWindowBufferCloseSummary {
    pub(crate) fn record(&mut self, outcome: TrackedWindowBufferCloseOutcome) {
        match outcome.window {
            TrackedResourceCloseOutcome::ClosedOrGone => {
                self.window_closed_or_gone = self.window_closed_or_gone.saturating_add(1);
            }
            TrackedResourceCloseOutcome::Retained => {
                self.window_retained = self.window_retained.saturating_add(1);
            }
        }
        match outcome.buffer {
            TrackedResourceCloseOutcome::ClosedOrGone => {
                self.buffer_closed_or_gone = self.buffer_closed_or_gone.saturating_add(1);
            }
            TrackedResourceCloseOutcome::Retained => {
                self.buffer_retained = self.buffer_retained.saturating_add(1);
            }
        }
    }

    pub(crate) fn retained_resources(self) -> usize {
        self.window_retained.saturating_add(self.buffer_retained)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct TrackedResourceCloseSummary {
    pub(crate) closed_or_gone: usize,
    pub(crate) retained: usize,
}

impl TrackedResourceCloseSummary {
    pub(crate) fn record(&mut self, outcome: TrackedResourceCloseOutcome) {
        match outcome {
            TrackedResourceCloseOutcome::ClosedOrGone => {
                self.closed_or_gone = self.closed_or_gone.saturating_add(1);
            }
            TrackedResourceCloseOutcome::Retained => {
                self.retained = self.retained.saturating_add(1);
            }
        }
    }
}
