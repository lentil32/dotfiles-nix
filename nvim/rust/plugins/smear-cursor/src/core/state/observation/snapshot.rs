use super::cursor_context::CursorColorProbeGenerations;
use super::cursor_context::CursorColorProbeWitness;
#[cfg(test)]
use super::cursor_context::CursorTextContextBoundary;
use super::cursor_context::CursorTextContextState;
use super::probe::BackgroundProbeState;
use super::probe::ProbeRequestSet;
use super::probe::ProbeReuse;
use super::probe::ProbeSet;
use super::probe::ProbeSlot;
use crate::core::runtime_reducer::ScrollShift;
use crate::core::state::ExternalDemand;
use crate::core::types::Millis;
use crate::core::types::ObservationId;
use crate::position::CursorObservation;
use crate::position::ObservedCell;
use crate::position::ScreenCell;
use crate::position::SurfaceId;
use crate::position::ViewportBounds;
use crate::position::WindowSurfaceSnapshot;

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct PendingObservation {
    // snapshot: ingress demand retained while collection is in flight.
    demand: ExternalDemand,
    // authoritative: requested probe policy before activation.
    requested_probes: ProbeRequestSet,
}

impl PendingObservation {
    pub(crate) fn new(demand: ExternalDemand, requested_probes: ProbeRequestSet) -> Self {
        Self {
            demand,
            requested_probes,
        }
    }

    pub(crate) const fn observation_id(&self) -> ObservationId {
        ObservationId::from_ingress_seq(self.demand.seq())
    }

    pub(crate) const fn demand(&self) -> &ExternalDemand {
        &self.demand
    }

    pub(crate) const fn requested_probes(&self) -> ProbeRequestSet {
        self.requested_probes
    }

    #[cfg(test)]
    pub(crate) const fn probes(&self) -> ProbeRequestSet {
        self.requested_probes()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ObservationBasis {
    // authoritative: collected observation facts and reuse witnesses.
    observed_at: Millis,
    mode: String,
    surface: WindowSurfaceSnapshot,
    // authoritative: projected display-space cursor truth retained by reducer
    // state; raw host probe details stay in event-layer readers and diagnostics.
    cursor: CursorObservation,
    viewport: ViewportBounds,
    buffer_revision: Option<u64>,
    cursor_text_context_state: CursorTextContextState,
}

impl ObservationBasis {
    pub(crate) fn new(
        observed_at: Millis,
        mode: String,
        surface: WindowSurfaceSnapshot,
        cursor: CursorObservation,
        viewport: ViewportBounds,
    ) -> Self {
        Self {
            observed_at,
            mode,
            surface,
            cursor,
            viewport,
            buffer_revision: None,
            cursor_text_context_state: CursorTextContextState::Unavailable,
        }
    }

    pub(crate) const fn observed_at(&self) -> Millis {
        self.observed_at
    }

    pub(crate) fn mode(&self) -> &str {
        &self.mode
    }

    pub(crate) const fn surface(&self) -> WindowSurfaceSnapshot {
        self.surface
    }

    pub(crate) const fn cursor(&self) -> CursorObservation {
        self.cursor
    }

    pub(crate) const fn cursor_position(&self) -> Option<ScreenCell> {
        self.cursor.screen_cell()
    }

    pub(crate) const fn exact_cursor_position(&self) -> Option<ScreenCell> {
        self.cursor.exact_screen_cell()
    }

    pub(crate) const fn requires_exact_cursor_position_refresh(&self) -> bool {
        self.cursor.requires_exact_refresh()
    }

    pub(crate) const fn viewport(&self) -> ViewportBounds {
        self.viewport
    }

    pub(crate) const fn buffer_revision(&self) -> Option<u64> {
        self.buffer_revision
    }

    #[cfg(test)]
    pub(crate) const fn cursor_text_context_boundary(&self) -> Option<CursorTextContextBoundary> {
        self.cursor_text_context_state.boundary()
    }

    pub(crate) const fn cursor_text_context_state(&self) -> &CursorTextContextState {
        &self.cursor_text_context_state
    }

    pub(crate) fn with_buffer_revision(mut self, buffer_revision: Option<u64>) -> Self {
        self.buffer_revision = buffer_revision;
        self
    }

    pub(crate) fn with_cursor_text_context_state(
        mut self,
        cursor_text_context_state: CursorTextContextState,
    ) -> Self {
        self.cursor_text_context_state = cursor_text_context_state;
        self
    }

    #[cfg(debug_assertions)]
    fn debug_assert_invariants(&self) {
        let surface = self.surface();
        let cursor = self.cursor();
        // Observation storage owns one projected display-space cursor fact. The
        // exact/deferred split tracks freshness only; reducer state does not
        // distinguish raw versus projected cursor coordinates.
        debug_assert!(
            SurfaceId::new(surface.id().window_handle(), surface.id().buffer_handle()).is_some(),
            "observation surfaces must retain positive window and buffer handles"
        );
        match cursor.cell() {
            ObservedCell::Unavailable => {
                debug_assert_eq!(self.cursor_position(), None);
                debug_assert_eq!(self.exact_cursor_position(), None);
                debug_assert!(!self.requires_exact_cursor_position_refresh());
            }
            ObservedCell::Exact(cell) => {
                debug_assert_eq!(self.cursor_position(), Some(cell));
                debug_assert_eq!(self.exact_cursor_position(), Some(cell));
                debug_assert!(!self.requires_exact_cursor_position_refresh());
            }
            ObservedCell::Deferred(cell) => {
                debug_assert_eq!(self.cursor_position(), Some(cell));
                debug_assert_eq!(self.exact_cursor_position(), None);
                debug_assert!(self.requires_exact_cursor_position_refresh());
            }
        }
    }

    #[cfg(not(debug_assertions))]
    fn debug_assert_invariants(&self) {}
}

pub(crate) fn derive_cursor_color_probe_witness(
    basis: &ObservationBasis,
    generations: CursorColorProbeGenerations,
) -> Option<CursorColorProbeWitness> {
    let buffer_revision = basis.buffer_revision()?;
    let surface_id = basis.surface().id();
    Some(CursorColorProbeWitness::new(
        surface_id.window_handle(),
        surface_id.buffer_handle(),
        buffer_revision,
        basis.mode().to_owned(),
        basis.cursor_position(),
        generations.colorscheme_generation(),
        generations.cache_generation(),
    ))
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct ObservationMotion {
    scroll_shift: Option<ScrollShift>,
}

impl ObservationMotion {
    pub(crate) const fn new(scroll_shift: Option<ScrollShift>) -> Self {
        Self { scroll_shift }
    }

    pub(crate) const fn scroll_shift(&self) -> Option<ScrollShift> {
        self.scroll_shift
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ObservationSnapshot {
    // snapshot: ingress demand that produced this observation.
    demand: ExternalDemand,
    // authoritative: active observation facts and probe lifecycle state.
    basis: ObservationBasis,
    probes: ProbeSet,
    // snapshot: shell-side generation witness used to derive cursor-color reuse keys.
    cursor_color_probe_generations: Option<CursorColorProbeGenerations>,
    // authoritative: observation-scoped motion metadata.
    motion: ObservationMotion,
}

impl ObservationSnapshot {
    pub(crate) fn new(
        pending: PendingObservation,
        basis: ObservationBasis,
        motion: ObservationMotion,
    ) -> Self {
        let requested_probes = pending.requested_probes();
        Self {
            demand: pending.demand,
            probes: ProbeSet::new(
                if requested_probes.cursor_color() {
                    ProbeSlot::pending()
                } else {
                    ProbeSlot::unrequested()
                },
                BackgroundProbeState::unrequested(),
            ),
            cursor_color_probe_generations: None,
            basis,
            motion,
        }
    }

    pub(crate) const fn demand(&self) -> &ExternalDemand {
        &self.demand
    }

    pub(crate) const fn observation_id(&self) -> ObservationId {
        ObservationId::from_ingress_seq(self.demand.seq())
    }

    pub(crate) const fn basis(&self) -> &ObservationBasis {
        &self.basis
    }

    pub(crate) const fn probes(&self) -> &ProbeSet {
        &self.probes
    }

    pub(crate) fn probes_mut(&mut self) -> &mut ProbeSet {
        &mut self.probes
    }

    pub(crate) fn cursor_color_probe_witness(&self) -> Option<CursorColorProbeWitness> {
        self.cursor_color_probe_generations
            .and_then(|generations| derive_cursor_color_probe_witness(&self.basis, generations))
    }

    pub(crate) const fn cursor_color_probe_generations(
        &self,
    ) -> Option<CursorColorProbeGenerations> {
        self.cursor_color_probe_generations
    }

    pub(crate) const fn motion(&self) -> ObservationMotion {
        self.motion
    }

    #[cfg(debug_assertions)]
    pub(crate) fn debug_assert_invariants(&self) {
        self.basis.debug_assert_invariants();
        let cursor = self.basis.cursor();
        match cursor.cell() {
            ObservedCell::Unavailable => {
                debug_assert_eq!(self.exact_cursor_position(), None);
                debug_assert!(!self.requires_exact_cursor_position_refresh());
            }
            ObservedCell::Exact(cell) => {
                debug_assert_eq!(self.exact_cursor_position(), Some(cell));
                debug_assert!(!self.requires_exact_cursor_position_refresh());
            }
            ObservedCell::Deferred(cell) => {
                debug_assert_eq!(self.basis.cursor_position(), Some(cell));
                debug_assert_eq!(self.exact_cursor_position(), None);
                debug_assert!(self.requires_exact_cursor_position_refresh());
            }
        }

        if let BackgroundProbeState::Ready { batch, .. } = self.probes.background() {
            debug_assert!(
                batch.is_valid_for_viewport(self.basis.viewport()),
                "background probe batches must match the observation viewport"
            );
        }
    }

    #[cfg(not(debug_assertions))]
    pub(crate) fn debug_assert_invariants(&self) {}

    pub(crate) fn with_cursor_color_probe_generations(
        mut self,
        cursor_color_probe_generations: Option<CursorColorProbeGenerations>,
    ) -> Self {
        self.cursor_color_probe_generations = cursor_color_probe_generations;
        self
    }

    pub(crate) fn cursor_color(&self) -> Option<u32> {
        self.probes.sampled_cursor_color()
    }

    pub(crate) const fn scroll_shift(&self) -> Option<ScrollShift> {
        self.motion().scroll_shift()
    }

    pub(crate) const fn exact_cursor_position(&self) -> Option<ScreenCell> {
        self.basis.exact_cursor_position()
    }

    pub(crate) const fn requires_exact_cursor_color_refresh(&self) -> bool {
        matches!(
            self.probes.cursor_color().reuse(),
            Some(ProbeReuse::Compatible)
        )
    }

    pub(crate) const fn requires_exact_cursor_position_refresh(&self) -> bool {
        self.basis.requires_exact_cursor_position_refresh()
    }
}
