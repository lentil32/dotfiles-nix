use crate::events::FlushRedrawCapability;
use crate::events::flush_redraw_capability;
use crate::events::set_flush_redraw_capability;
use crate::host::HostFlushRedrawCapability;
use crate::host::NeovimHost;
use crate::host::RedrawCommandPort;
use nvim_oxi::Result;

fn flush_redraw_capability_from_host(
    capability: HostFlushRedrawCapability,
) -> FlushRedrawCapability {
    match capability {
        HostFlushRedrawCapability::ApiAvailable => FlushRedrawCapability::ApiAvailable,
        HostFlushRedrawCapability::FallbackOnly => FlushRedrawCapability::FallbackOnly,
    }
}

pub(crate) fn refresh_redraw_capability() -> Result<()> {
    refresh_redraw_capability_with(&NeovimHost)
}

fn refresh_redraw_capability_with(host: &impl RedrawCommandPort) -> Result<()> {
    let capability = host.probe_flush_redraw_capability()?;
    set_flush_redraw_capability(flush_redraw_capability_from_host(capability));
    Ok(())
}

pub(crate) fn redraw() -> Result<()> {
    redraw_with(&NeovimHost)
}

fn redraw_with(host: &impl RedrawCommandPort) -> Result<()> {
    let capability = match flush_redraw_capability() {
        FlushRedrawCapability::Unknown => {
            refresh_redraw_capability_with(host)?;
            flush_redraw_capability()
        }
        known => known,
    };

    if matches!(capability, FlushRedrawCapability::ApiAvailable) {
        match host.flush_redraw() {
            Ok(()) => return Ok(()),
            Err(_) => set_flush_redraw_capability(FlushRedrawCapability::FallbackOnly),
        }
    }

    host.fallback_redraw()
}

#[cfg(test)]
mod tests {
    use super::FlushRedrawCapability;
    use super::flush_redraw_capability_from_host;
    use super::redraw_with;
    use super::refresh_redraw_capability_with;
    use crate::events::flush_redraw_capability;
    use crate::events::set_flush_redraw_capability;
    use crate::host::FakeRedrawCommandPort;
    use crate::host::HostFlushRedrawCapability;
    use crate::host::RedrawCommandCall;
    use pretty_assertions::assert_eq;

    #[test]
    fn flush_redraw_capability_maps_host_capability_to_runtime_capability() {
        assert_eq!(
            flush_redraw_capability_from_host(HostFlushRedrawCapability::ApiAvailable),
            FlushRedrawCapability::ApiAvailable
        );
        assert_eq!(
            flush_redraw_capability_from_host(HostFlushRedrawCapability::FallbackOnly),
            FlushRedrawCapability::FallbackOnly
        );
    }

    #[test]
    fn refresh_redraw_capability_reads_through_redraw_command_port() {
        let host = FakeRedrawCommandPort::default();
        host.push_probe_flush_redraw_capability(HostFlushRedrawCapability::ApiAvailable);
        set_flush_redraw_capability(FlushRedrawCapability::Unknown);

        refresh_redraw_capability_with(&host).expect("capability refresh should succeed");

        assert_eq!(
            flush_redraw_capability(),
            FlushRedrawCapability::ApiAvailable
        );
        assert_eq!(
            host.calls(),
            vec![RedrawCommandCall::ProbeFlushRedrawCapability]
        );
    }

    #[test]
    fn refresh_redraw_capability_returns_redraw_command_port_failures() {
        let host = FakeRedrawCommandPort::default();
        host.push_probe_error("exists failed");
        set_flush_redraw_capability(FlushRedrawCapability::Unknown);

        let err = refresh_redraw_capability_with(&host)
            .expect_err("capability probe failure should propagate");

        assert!(err.to_string().contains("exists failed"));
        assert_eq!(flush_redraw_capability(), FlushRedrawCapability::Unknown);
        assert_eq!(
            host.calls(),
            vec![RedrawCommandCall::ProbeFlushRedrawCapability]
        );
    }

    #[test]
    fn redraw_probes_unknown_capability_before_flushing() {
        let host = FakeRedrawCommandPort::default();
        host.push_probe_flush_redraw_capability(HostFlushRedrawCapability::ApiAvailable);
        set_flush_redraw_capability(FlushRedrawCapability::Unknown);

        redraw_with(&host).expect("redraw should flush through available API");

        assert_eq!(
            flush_redraw_capability(),
            FlushRedrawCapability::ApiAvailable
        );
        assert_eq!(
            host.calls(),
            vec![
                RedrawCommandCall::ProbeFlushRedrawCapability,
                RedrawCommandCall::FlushRedraw,
            ]
        );
    }

    #[test]
    fn redraw_uses_cached_fallback_without_api_attempt() {
        let host = FakeRedrawCommandPort::default();
        set_flush_redraw_capability(FlushRedrawCapability::FallbackOnly);

        redraw_with(&host).expect("fallback redraw should succeed");

        assert_eq!(host.calls(), vec![RedrawCommandCall::FallbackRedraw]);
    }

    #[test]
    fn redraw_demotes_capability_when_api_flush_fails() {
        let host = FakeRedrawCommandPort::default();
        host.push_flush_error("flush failed");
        set_flush_redraw_capability(FlushRedrawCapability::ApiAvailable);

        redraw_with(&host).expect("redraw should fall back after API flush failure");

        assert_eq!(
            flush_redraw_capability(),
            FlushRedrawCapability::FallbackOnly
        );
        assert_eq!(
            host.calls(),
            vec![
                RedrawCommandCall::FlushRedraw,
                RedrawCommandCall::FallbackRedraw,
            ]
        );
    }

    #[test]
    fn redraw_returns_fallback_failures() {
        let host = FakeRedrawCommandPort::default();
        host.push_fallback_error("fallback failed");
        set_flush_redraw_capability(FlushRedrawCapability::FallbackOnly);

        let err = redraw_with(&host).expect_err("fallback redraw failure should propagate");

        assert!(err.to_string().contains("fallback failed"));
        assert_eq!(host.calls(), vec![RedrawCommandCall::FallbackRedraw]);
    }
}
