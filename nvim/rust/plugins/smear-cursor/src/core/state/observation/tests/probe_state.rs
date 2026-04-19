use super::*;
use crate::core::types::Generation;
use pretty_assertions::assert_eq;

#[test]
fn unrequested_probe_slots_reject_probe_population() {
    let request = observation_request(ProbeRequestSet::default());
    let viewport = viewport_bounds(8, 16);
    let mut snapshot = ObservationSnapshot::new(
        request,
        observation_basis(viewport),
        ObservationMotion::default(),
    );

    assert!(matches!(
        snapshot.probes().cursor_color(),
        ProbeSlot::Unrequested
    ));
    assert!(matches!(
        snapshot.probes().background(),
        BackgroundProbeState::Unrequested
    ));
    assert!(snapshot.probes().background().next_chunk().is_none());
    assert!(
        !snapshot
            .probes_mut()
            .set_cursor_color_state(ProbeState::ready(
                ProbeReuse::Exact,
                Some(CursorColorSample::new(0x00AB_CDEF)),
            ))
    );
    assert!(
        !snapshot
            .probes_mut()
            .background_mut()
            .set_failed(ProbeFailure::ShellReadFailed)
    );
}

#[test]
fn observation_id_is_derived_only_from_the_root_demand_sequence() {
    let demand = ExternalDemand::new(
        IngressSeq::new(41),
        ExternalDemandKind::ExternalCursor,
        Millis::new(10),
        BufferPerfClass::Full,
    );
    let request = PendingObservation::new(
        demand.clone(),
        ProbeRequestSet::only(crate::core::state::ProbeKind::CursorColor),
    );
    let snapshot = ObservationSnapshot::new(
        request.clone(),
        observation_basis(viewport_bounds(8, 16)),
        ObservationMotion::default(),
    );

    assert_eq!(
        request.observation_id(),
        crate::core::types::ObservationId::from_ingress_seq(demand.seq())
    );
    assert_eq!(snapshot.observation_id(), request.observation_id());
}

#[test]
fn activated_cursor_color_requestedness_lives_in_the_probe_slot() {
    let request = observation_request(ProbeRequestSet::only(
        crate::core::state::ProbeKind::CursorColor,
    ));
    let viewport = viewport_bounds(8, 16);
    let snapshot = ObservationSnapshot::new(
        request,
        observation_basis(viewport),
        ObservationMotion::default(),
    );

    assert!(snapshot.probes().cursor_color().is_requested());
    assert!(matches!(
        snapshot.probes().cursor_color(),
        ProbeSlot::Requested(ProbeState::Pending)
    ));
    assert!(!snapshot.probes().background().is_requested());
}

#[test]
fn cursor_color_probe_witness_is_derived_from_boundary_and_generations() {
    let request = observation_request(ProbeRequestSet::only(
        crate::core::state::ProbeKind::CursorColor,
    ));
    let viewport = viewport_bounds(8, 16);
    let basis = observation_basis(viewport).with_buffer_revision(Some(14));
    let generations = crate::core::state::CursorColorProbeGenerations::new(
        Generation::new(3),
        Generation::new(5),
    );
    let snapshot = ObservationSnapshot::new(request, basis, ObservationMotion::default())
        .with_cursor_color_probe_generations(Some(generations));

    assert_eq!(
        snapshot.cursor_color_probe_witness(),
        Some(crate::core::state::CursorColorProbeWitness::new(
            1,
            1,
            14,
            "n".to_string(),
            Some(screen_cell(4, 5)),
            Generation::new(3),
            Generation::new(5),
        )),
    );
}

#[test]
fn cursor_color_probe_witness_requires_a_buffer_revision() {
    let request = observation_request(ProbeRequestSet::only(
        crate::core::state::ProbeKind::CursorColor,
    ));
    let viewport = viewport_bounds(8, 16);
    let snapshot = ObservationSnapshot::new(
        request,
        observation_basis(viewport),
        ObservationMotion::default(),
    )
    .with_cursor_color_probe_generations(Some(
        crate::core::state::CursorColorProbeGenerations::new(
            Generation::new(3),
            Generation::new(5),
        ),
    ));

    assert_eq!(snapshot.cursor_color_probe_witness(), None);
}
