use super::BackgroundProbeBatch;
use super::BackgroundProbeChunk;
use super::BackgroundProbeChunkMask;
use super::BackgroundProbePlan;
use super::BackgroundProbeProgress;
use super::BackgroundProbeState;
use super::BackgroundProbeUpdate;
use super::CursorColorSample;
use super::CursorTextContext;
use super::CursorTextContextState;
use super::ObservationBasis;
use super::ObservationMotion;
use super::ObservationSnapshot;
use super::ObservedTextRow;
use super::PendingObservation;
use super::ProbeFailure;
use super::ProbeRequestSet;
use super::ProbeReuse;
use super::ProbeSlot;
use super::ProbeState;
use super::SemanticEvent;
use super::classify_semantic_event;
use crate::core::state::BufferPerfClass;
use crate::core::state::ExternalDemand;
use crate::core::state::ExternalDemandKind;
use crate::core::types::CursorCol;
use crate::core::types::CursorPosition;
use crate::core::types::CursorRow;
use crate::core::types::IngressSeq;
use crate::core::types::Millis;
use crate::core::types::ViewportSnapshot;
use crate::state::CursorLocation;
use crate::types::ScreenCell;

mod background_probe;
mod probe_state;
mod semantic;

fn observation_request(probes: ProbeRequestSet) -> PendingObservation {
    PendingObservation::new(
        ExternalDemand::new(
            IngressSeq::new(1),
            ExternalDemandKind::ExternalCursor,
            Millis::new(10),
            None,
            BufferPerfClass::Full,
        ),
        probes,
    )
}

fn observation_basis(viewport: ViewportSnapshot) -> ObservationBasis {
    ObservationBasis::new(
        Millis::new(10),
        "n".to_string(),
        Some(CursorPosition {
            row: CursorRow(4),
            col: CursorCol(5),
        }),
        CursorLocation::new(1, 1, 1, 1),
        viewport,
    )
}

fn with_background_probe_plan(
    mut snapshot: ObservationSnapshot,
    plan: BackgroundProbePlan,
) -> ObservationSnapshot {
    *snapshot.probes_mut().background_mut() = BackgroundProbeState::from_plan(plan);
    snapshot
}

fn with_background_probe_failed(
    mut snapshot: ObservationSnapshot,
    failure: ProbeFailure,
) -> Option<ObservationSnapshot> {
    snapshot
        .probes_mut()
        .background_mut()
        .set_failed(failure)
        .then_some(snapshot)
}

fn next_background_chunk(snapshot: &ObservationSnapshot) -> Option<BackgroundProbeChunk> {
    snapshot.probes().background().next_chunk()
}

fn observed_rows(rows: &[&str]) -> Vec<ObservedTextRow> {
    rows.iter()
        .map(|text| ObservedTextRow::new((*text).to_string()))
        .collect()
}

fn text_context(
    changedtick: u64,
    line: i64,
    rows: &[&str],
    tracked_rows: Option<&[&str]>,
) -> CursorTextContext {
    CursorTextContext::new(
        99,
        changedtick,
        line,
        observed_rows(rows),
        tracked_rows.map(observed_rows),
    )
}

fn assert_text_mutation_classification(
    previous_position: CursorPosition,
    previous_line: i64,
    previous_rows: &[&str],
    current_position: CursorPosition,
    current_line: i64,
    current_rows: &[&str],
    current_tracked_rows: Option<&[&str]>,
) {
    let request = observation_request(ProbeRequestSet::default());
    let viewport = ViewportSnapshot::new(CursorRow(40), CursorCol(120));
    let previous =
        ObservationSnapshot::new(
            request.clone(),
            ObservationBasis::new(
                Millis::new(10),
                "n".to_string(),
                Some(previous_position),
                CursorLocation::new(1, 1, 1, previous_line),
                viewport,
            )
            .with_cursor_text_context_state(CursorTextContextState::Sampled(
                text_context(4, previous_line, previous_rows, None),
            )),
            ObservationMotion::default(),
        );
    let current =
        ObservationSnapshot::new(
            request,
            ObservationBasis::new(
                Millis::new(11),
                "n".to_string(),
                Some(current_position),
                CursorLocation::new(1, 1, 1, current_line),
                viewport,
            )
            .with_cursor_text_context_state(CursorTextContextState::Sampled(
                text_context(5, current_line, current_rows, current_tracked_rows),
            )),
            ObservationMotion::default(),
        );

    pretty_assertions::assert_eq!(
        classify_semantic_event(Some(&previous), &current),
        SemanticEvent::TextMutatedAtCursorContext
    );
}
