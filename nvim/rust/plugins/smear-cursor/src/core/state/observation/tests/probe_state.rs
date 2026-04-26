use super::*;
use pretty_assertions::assert_eq;

use crate::core::types::Generation;

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
