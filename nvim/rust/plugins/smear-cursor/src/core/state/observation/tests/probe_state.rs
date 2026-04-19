use super::*;

#[test]
fn unrequested_probe_slots_reject_probe_population() {
    let request = observation_request(ProbeRequestSet::default());
    let viewport = ViewportSnapshot::new(CursorRow(8), CursorCol(16));
    let snapshot = ObservationSnapshot::new(
        request.clone(),
        observation_basis(&request, viewport),
        ObservationMotion::default(),
    );

    assert!(matches!(
        snapshot.probes().cursor_color(),
        ProbeSlot::Unrequested
    ));
    assert!(matches!(
        snapshot.background_probe_state(),
        BackgroundProbeState::Unrequested
    ));
    assert!(snapshot.background_progress().is_none());
    assert!(
        snapshot
            .clone()
            .with_cursor_color_probe(ProbeState::ready(
                ProbeKind::CursorColor.request_id(request.observation_id()),
                request.observation_id(),
                ProbeReuse::Exact,
                Some(CursorColorSample::new(0x00AB_CDEF)),
            ))
            .is_none()
    );
    assert!(
        snapshot
            .with_background_probe_failed(
                ProbeKind::Background.request_id(request.observation_id()),
                ProbeFailure::ShellReadFailed,
            )
            .is_none()
    );
}
