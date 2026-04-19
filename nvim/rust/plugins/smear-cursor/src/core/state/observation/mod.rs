mod background_probe;
mod background_probe_cells;
mod cursor_context;
mod probe;
mod semantic;
mod snapshot;

#[cfg(test)]
mod tests;

pub(crate) use background_probe::BackgroundProbeBatch;
pub(crate) use background_probe::BackgroundProbeChunk;
pub(crate) use background_probe::BackgroundProbeChunkMask;
#[allow(
    unused_imports,
    reason = "Returned from crate-visible APIs in this module tree."
)]
pub(crate) use background_probe::BackgroundProbePackedMaskIter;
pub(crate) use background_probe::BackgroundProbePlan;
#[cfg(test)]
pub(crate) use background_probe::BackgroundProbeProgress;
#[cfg(test)]
pub(crate) use background_probe::BackgroundProbeUpdate;
pub(crate) use cursor_context::CursorColorProbeGenerations;
pub(crate) use cursor_context::CursorColorProbeWitness;
pub(crate) use cursor_context::CursorTextContext;
pub(crate) use cursor_context::CursorTextContextBoundary;
pub(crate) use cursor_context::CursorTextContextState;
pub(crate) use cursor_context::ObservedTextRow;
pub(crate) use probe::BackgroundProbeState;
pub(crate) use probe::CursorColorSample;
pub(crate) use probe::ProbeFailure;
pub(crate) use probe::ProbeKind;
pub(crate) use probe::ProbeRefreshState;
pub(crate) use probe::ProbeRequestSet;
pub(crate) use probe::ProbeReuse;
#[allow(
    unused_imports,
    reason = "Returned from crate-visible APIs in this module tree."
)]
pub(crate) use probe::ProbeSet;
pub(crate) use probe::ProbeSlot;
pub(crate) use probe::ProbeState;
pub(crate) use semantic::SemanticEvent;
pub(crate) use semantic::classify_semantic_event;
pub(crate) use snapshot::CursorPositionSync;
pub(crate) use snapshot::ObservationBasis;
pub(crate) use snapshot::ObservationMotion;
pub(crate) use snapshot::ObservationSnapshot;
pub(crate) use snapshot::PendingObservation;
pub(crate) use snapshot::derive_cursor_color_probe_witness;
