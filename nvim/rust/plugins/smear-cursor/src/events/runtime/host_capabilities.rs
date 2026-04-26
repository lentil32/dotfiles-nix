use std::cell::Cell;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum FlushRedrawCapability {
    #[default]
    Unknown,
    ApiAvailable,
    FallbackOnly,
}

#[derive(Debug, Default)]
pub(super) struct HostCapabilitiesLane {
    flush_redraw: Cell<FlushRedrawCapability>,
}

impl HostCapabilitiesLane {
    pub(super) fn set_flush_redraw_capability(&self, capability: FlushRedrawCapability) {
        self.flush_redraw.set(capability);
    }

    pub(super) fn flush_redraw_capability(&self) -> FlushRedrawCapability {
        self.flush_redraw.get()
    }
}
