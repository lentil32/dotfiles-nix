use super::BackgroundProbeBatch;
use super::BackgroundProbeChunk;
use super::BackgroundProbeChunkMask;
use super::BackgroundProbePlan;
use super::BackgroundProbeProgress;
use super::BackgroundProbeState;
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
use super::SemanticEvent;
use super::classify_semantic_event;
use crate::core::state::BufferPerfClass;
use crate::core::state::ExternalDemand;
use crate::core::state::ExternalDemandKind;
use crate::core::types::IngressSeq;
use crate::core::types::Millis;
use crate::position::BufferLine;
use crate::position::CursorObservation;
use crate::position::ObservedCell;
use crate::position::ScreenCell;
use crate::position::SurfaceId;
use crate::position::ViewportBounds;
use crate::position::WindowSurfaceSnapshot;

mod background_probe;
mod probe_state;
mod semantic;

fn screen_cell(row: i64, col: i64) -> ScreenCell {
    ScreenCell::new(row, col).expect("positive cursor position")
}

fn viewport_bounds(max_row: i64, max_col: i64) -> ViewportBounds {
    ViewportBounds::new(max_row, max_col).expect("positive viewport bounds")
}

fn observation_request(probes: ProbeRequestSet) -> PendingObservation {
    PendingObservation::new(
        ExternalDemand::new(
            IngressSeq::new(1),
            ExternalDemandKind::ExternalCursor,
            Millis::new(10),
            BufferPerfClass::Full,
        ),
        probes,
    )
}

fn observation_basis(viewport: ViewportBounds) -> ObservationBasis {
    ObservationBasis::new(
        Millis::new(10),
        "n".to_string(),
        WindowSurfaceSnapshot::new(
            SurfaceId::new(1, 1).expect("positive handles"),
            BufferLine::new(1).expect("positive top buffer line"),
            0,
            0,
            ScreenCell::new(1, 1).expect("one-based window origin"),
            viewport,
        ),
        CursorObservation::new(
            BufferLine::new(1).expect("positive buffer line"),
            ObservedCell::Exact(ScreenCell::new(4, 5).expect("positive cursor position")),
        ),
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
    previous_position: ScreenCell,
    previous_line: i64,
    previous_rows: &[&str],
    current_position: ScreenCell,
    current_line: i64,
    current_rows: &[&str],
    current_tracked_rows: Option<&[&str]>,
) {
    let request = observation_request(ProbeRequestSet::default());
    let viewport = ViewportBounds::new(40, 120).expect("positive viewport bounds");
    let previous =
        ObservationSnapshot::new(
            request.clone(),
            ObservationBasis::new(
                Millis::new(10),
                "n".to_string(),
                WindowSurfaceSnapshot::new(
                    SurfaceId::new(1, 1).expect("positive handles"),
                    BufferLine::new(1).expect("positive top buffer line"),
                    0,
                    0,
                    ScreenCell::new(1, 1).expect("one-based window origin"),
                    viewport,
                ),
                CursorObservation::new(
                    BufferLine::new(previous_line).expect("positive buffer line"),
                    ObservedCell::Exact(previous_position),
                ),
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
                WindowSurfaceSnapshot::new(
                    SurfaceId::new(1, 1).expect("positive handles"),
                    BufferLine::new(1).expect("positive top buffer line"),
                    0,
                    0,
                    ScreenCell::new(1, 1).expect("one-based window origin"),
                    viewport,
                ),
                CursorObservation::new(
                    BufferLine::new(current_line).expect("positive buffer line"),
                    ObservedCell::Exact(current_position),
                ),
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
