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

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn host_capabilities_lane_tracks_flush_redraw_capability() {
        let lane = HostCapabilitiesLane::default();

        let initial = lane.flush_redraw_capability();
        lane.set_flush_redraw_capability(FlushRedrawCapability::ApiAvailable);
        let after_api_available = lane.flush_redraw_capability();
        lane.set_flush_redraw_capability(FlushRedrawCapability::FallbackOnly);
        let after_fallback_only = lane.flush_redraw_capability();

        assert_eq!(
            [initial, after_api_available, after_fallback_only],
            [
                FlushRedrawCapability::Unknown,
                FlushRedrawCapability::ApiAvailable,
                FlushRedrawCapability::FallbackOnly
            ]
        );
    }
}
