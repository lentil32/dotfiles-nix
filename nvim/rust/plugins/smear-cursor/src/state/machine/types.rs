use super::CursorShape;
use super::TrackedCursor;
use crate::core::types::StrokeId;
use crate::host::BufferHandle;
use crate::position::RenderPoint;
use crate::position::ScreenCell;
use crate::types::Particle;
use crate::types::ParticleAggregationScratch;
use crate::types::RenderStepSample;
use crate::types::SharedAggregatedParticleCells;
use crate::types::SharedParticleScreenCells;
use std::num::NonZeroU32;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum PluginState {
    Enabled,
    Disabled,
}

impl PluginState {
    pub(super) fn from_enabled(enabled: bool) -> Self {
        if enabled {
            Self::Enabled
        } else {
            Self::Disabled
        }
    }

    pub(super) fn is_enabled(self) -> bool {
        matches!(self, Self::Enabled)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum AnimationClockSample {
    Advance { elapsed_ms: f64 },
    Discontinuity,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) struct MotionClock {
    pub(super) last_tick_ms: Option<f64>,
    pub(super) next_frame_at_ms: Option<f64>,
    pub(super) simulation_accumulator_ms: f64,
}

impl MotionClock {
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SettlingWindow {
    pub(crate) stable_since_ms: f64,
    pub(crate) settle_deadline_ms: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct SettlingPhase {
    pub(super) settling_window: SettlingWindow,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) struct RunningPhase {
    pub(super) clock: MotionClock,
    pub(super) settle_hold_counter: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct DrainingPhase {
    pub(super) clock: MotionClock,
    pub(super) remaining_steps: NonZeroU32,
}

impl DrainingPhase {
    pub(super) fn new(remaining_steps: u32, last_tick_ms: Option<f64>) -> Option<Self> {
        NonZeroU32::new(remaining_steps).map(|remaining_steps| Self {
            clock: MotionClock {
                last_tick_ms,
                ..MotionClock::default()
            },
            remaining_steps,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum AnimationPhase {
    Uninitialized,
    Idle,
    Settling(SettlingPhase),
    Running(RunningPhase),
    Draining(DrainingPhase),
}

impl AnimationPhase {
    pub(super) fn is_initialized(&self) -> bool {
        !matches!(self, Self::Uninitialized)
    }

    pub(super) fn is_animating(&self) -> bool {
        matches!(self, Self::Running(_))
    }

    pub(super) fn is_settling(&self) -> bool {
        matches!(self, Self::Settling(_))
    }

    pub(super) fn is_draining(&self) -> bool {
        matches!(self, Self::Draining(_))
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) struct RetargetSurfaceKey {
    window_handle: i64,
    buffer_handle: BufferHandle,
    window_width: i64,
    window_height: i64,
}

impl RetargetSurfaceKey {
    pub(super) fn from_tracked_cursor(tracked_cursor: &TrackedCursor) -> Self {
        let surface = tracked_cursor.surface();
        let surface_id = surface.id();
        let window_size = surface.window_size();
        Self {
            window_handle: surface_id.window_handle(),
            buffer_handle: surface_id.buffer_handle(),
            window_width: window_size.max_col(),
            window_height: window_size.max_row(),
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct RuntimeTargetRetargetKey {
    cell: Option<ScreenCell>,
    shape: CursorShape,
    surface: Option<RetargetSurfaceKey>,
}

impl RuntimeTargetRetargetKey {
    const fn new(
        cell: Option<ScreenCell>,
        shape: CursorShape,
        surface: Option<RetargetSurfaceKey>,
    ) -> Self {
        Self {
            cell,
            shape,
            surface,
        }
    }

    pub(crate) fn from_snapshot(
        position: RenderPoint,
        shape: CursorShape,
        tracked_cursor: Option<&TrackedCursor>,
    ) -> Self {
        Self::new(
            ScreenCell::from_rounded_point(position),
            shape,
            tracked_cursor.map(RetargetSurfaceKey::from_tracked_cursor),
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RuntimeTargetSnapshot {
    position: RenderPoint,
    shape: CursorShape,
    tracked_cursor: Option<TrackedCursor>,
}

impl RuntimeTargetSnapshot {
    fn new(
        position: RenderPoint,
        shape: CursorShape,
        tracked_cursor: Option<TrackedCursor>,
    ) -> Self {
        Self {
            position,
            shape,
            tracked_cursor,
        }
    }

    pub(crate) fn tracked(
        position: RenderPoint,
        shape: CursorShape,
        tracked_cursor: &TrackedCursor,
    ) -> Self {
        Self::new(position, shape, Some(tracked_cursor.clone()))
    }

    #[cfg(test)]
    pub(crate) fn preserving_tracking(
        position: RenderPoint,
        shape: CursorShape,
        tracked_cursor: Option<&TrackedCursor>,
    ) -> Self {
        Self::new(position, shape, tracked_cursor.cloned())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct CursorTarget {
    pub(super) position: RenderPoint,
    pub(super) shape: CursorShape,
    pub(super) tracked_cursor: Option<TrackedCursor>,
    pub(super) retarget_epoch: u64,
}

impl Default for CursorTarget {
    fn default() -> Self {
        Self {
            position: RenderPoint::ZERO,
            shape: CursorShape::block(),
            tracked_cursor: None,
            retarget_epoch: 0,
        }
    }
}

impl CursorTarget {
    pub(super) fn retarget_key(&self) -> RuntimeTargetRetargetKey {
        RuntimeTargetRetargetKey::from_snapshot(
            self.position,
            self.shape,
            self.tracked_cursor.as_ref(),
        )
    }

    pub(super) fn apply_snapshot(&mut self, snapshot: RuntimeTargetSnapshot) -> bool {
        let retarget_key = RuntimeTargetRetargetKey::from_snapshot(
            snapshot.position,
            snapshot.shape,
            snapshot.tracked_cursor.as_ref(),
        );
        let RuntimeTargetSnapshot {
            position,
            shape,
            tracked_cursor,
        } = snapshot;
        let geometry_changed = self.position != position || self.shape != shape;
        if self.retarget_key() != retarget_key {
            self.retarget_epoch = self.retarget_epoch.wrapping_add(1);
        }

        self.position = position;
        self.shape = shape;
        self.tracked_cursor = tracked_cursor;

        geometry_changed
    }

    pub(super) fn corners(&self) -> [RenderPoint; 4] {
        self.shape.corners(self.position)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct TrailState {
    pub(super) stroke_id: StrokeId,
    pub(super) origin_corners: [RenderPoint; 4],
    pub(super) elapsed_ms: [f64; 4],
}

impl Default for TrailState {
    fn default() -> Self {
        Self {
            stroke_id: StrokeId::INITIAL,
            origin_corners: [RenderPoint::ZERO; 4],
            elapsed_ms: [0.0; 4],
        }
    }
}

// Short-lived runtime facts still live in reducer truth. They may reset at
// lifecycle boundaries, but they are not purgeable caches.
#[derive(Debug, Clone, PartialEq, Default)]
pub(super) struct TransientRuntimeState {
    pub(super) last_observed_mode: LastObservedMode,
    pub(super) color_at_cursor: Option<u32>,
}

impl TransientRuntimeState {
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub(super) enum LastObservedMode {
    #[default]
    Unknown,
    Cmdline,
    NonCmdline,
}

impl LastObservedMode {
    pub(super) const fn from_cmdline(is_cmdline: bool) -> Self {
        if is_cmdline {
            Self::Cmdline
        } else {
            Self::NonCmdline
        }
    }

    pub(super) const fn crossed_cmdline_boundary(self, current: Self) -> bool {
        matches!(
            (self, current),
            (Self::Cmdline, Self::NonCmdline) | (Self::NonCmdline, Self::Cmdline)
        )
    }
}

// Purgeable buffers reused across reducer and preview work. Dropping them must not
// change runtime semantics.
#[derive(Debug, Default)]
pub(super) struct RuntimeScratchBuffers {
    pub(super) preview_particles: Vec<Particle>,
    pub(super) render_step_samples: Vec<RenderStepSample>,
    pub(super) particle_aggregation: ParticleAggregationScratch,
}

// Lazy particle-derived render artifacts. These mirror authoritative particle state
// and can always be rebuilt from the live runtime inputs.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct CachedParticleArtifacts {
    pub(super) aggregated_particle_cells: SharedAggregatedParticleCells,
    pub(super) particle_screen_cells: Option<SharedParticleScreenCells>,
}

// `None` means the current authoritative particles have not been materialized into
// cache form yet. This avoids a parallel cache-owned freshness bit.
#[derive(Debug, Clone, PartialEq, Default)]
pub(super) struct RuntimeParticleArtifactsCache {
    pub(super) cached: Option<CachedParticleArtifacts>,
}

// Explicitly groups all purgeable runtime storage so semantic runtime state stays
// separate from scratch buffers and rebuildable caches.
#[derive(Debug, Default)]
pub(super) struct RuntimeCaches {
    pub(super) scratch_buffers: RuntimeScratchBuffers,
    pub(super) particle_artifacts: RuntimeParticleArtifactsCache,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum CursorTransitionPolicy {
    Initialize { seed: u32 },
    JumpPreservingMotion,
    JumpAndStopAnimation,
    SyncToCurrentCursor,
}

#[cfg(test)]
mod tests {
    use super::CursorShape;
    use super::CursorTarget;
    use super::RetargetSurfaceKey;
    use super::RuntimeTargetRetargetKey;
    use super::RuntimeTargetSnapshot;
    use crate::host::BufferHandle;
    use crate::position::BufferLine;
    use crate::position::RenderPoint;
    use crate::position::ScreenCell;
    use crate::position::SurfaceId;
    use crate::position::ViewportBounds;
    use crate::position::WindowSurfaceSnapshot;
    use crate::state::TrackedCursor;
    use pretty_assertions::assert_eq;

    fn tracked_cursor(
        top_row: i64,
        line: i64,
        viewport_columns: (i64, i64),
        window_origin: (i64, i64),
        window_dimensions: (i64, i64),
    ) -> TrackedCursor {
        let (left_col, text_offset) = viewport_columns;
        let (window_row, window_col) = window_origin;
        let (window_width, window_height) = window_dimensions;

        TrackedCursor::new(
            WindowSurfaceSnapshot::new(
                SurfaceId::new(11, 17).expect("positive handles"),
                BufferLine::new(top_row).expect("positive top buffer line"),
                u32::try_from(left_col).expect("non-negative left column"),
                u32::try_from(text_offset).expect("non-negative text offset"),
                ScreenCell::new(window_row, window_col).expect("one-based window origin"),
                ViewportBounds::new(window_height, window_width).expect("positive window size"),
            ),
            BufferLine::new(line).expect("positive cursor buffer line"),
        )
    }

    #[test]
    fn retarget_surface_key_uses_tracked_cursor_surface_identity_and_dimensions() {
        assert_eq!(
            RetargetSurfaceKey::from_tracked_cursor(&tracked_cursor(
                23,
                29,
                (5, 2),
                (7, 13),
                (80, 24),
            )),
            RetargetSurfaceKey {
                window_handle: 11,
                buffer_handle: BufferHandle::from_raw_for_test(/*value*/ 17),
                window_width: 80,
                window_height: 24,
            }
        );
    }

    #[test]
    fn runtime_target_key_omits_surface_when_tracking_is_absent() {
        let position = RenderPoint {
            row: 7.0,
            col: 19.0,
        };
        let shape = CursorShape::block();
        assert_eq!(
            RuntimeTargetRetargetKey::from_snapshot(position, shape, None),
            RuntimeTargetRetargetKey::new(
                Some(ScreenCell::new(7, 19).expect("rounded render point should stay one-based")),
                shape,
                None,
            )
        );
    }

    #[test]
    fn cursor_target_retarget_epoch_ignores_surface_translations_outside_the_retarget_key() {
        let position = RenderPoint {
            row: 7.0,
            col: 19.0,
        };
        let shape = CursorShape::block();
        let base_tracked_cursor = tracked_cursor(23, 29, (5, 2), (7, 13), (80, 24));
        let translated_tracked_cursor = tracked_cursor(41, 29, (11, 4), (9, 21), (80, 24));
        let mut target = CursorTarget::default();

        assert!(target.apply_snapshot(RuntimeTargetSnapshot::tracked(
            position,
            shape,
            &base_tracked_cursor,
        )));
        let baseline_epoch = target.retarget_epoch;

        assert!(!target.apply_snapshot(RuntimeTargetSnapshot::tracked(
            position,
            shape,
            &translated_tracked_cursor,
        )));
        assert_eq!(target.retarget_epoch, baseline_epoch);
        assert_eq!(target.tracked_cursor, Some(translated_tracked_cursor));
    }

    #[test]
    fn cursor_target_retarget_epoch_advances_when_the_retarget_key_changes() {
        let position = RenderPoint {
            row: 7.0,
            col: 19.0,
        };
        let base_tracked_cursor = tracked_cursor(23, 29, (5, 2), (7, 13), (80, 24));
        let resized_tracked_cursor = tracked_cursor(23, 29, (5, 2), (7, 13), (100, 24));
        let mut target = CursorTarget::default();

        target.apply_snapshot(RuntimeTargetSnapshot::tracked(
            position,
            CursorShape::block(),
            &base_tracked_cursor,
        ));
        let baseline_epoch = target.retarget_epoch;

        assert!(target.apply_snapshot(RuntimeTargetSnapshot::tracked(
            position,
            CursorShape::vertical_bar(),
            &base_tracked_cursor,
        )));
        assert_eq!(target.retarget_epoch, baseline_epoch.wrapping_add(1));

        let after_shape_epoch = target.retarget_epoch;
        assert!(target.apply_snapshot(RuntimeTargetSnapshot::tracked(
            RenderPoint {
                row: 8.0,
                col: 19.0,
            },
            CursorShape::vertical_bar(),
            &base_tracked_cursor,
        )));
        assert_eq!(target.retarget_epoch, after_shape_epoch.wrapping_add(1));

        let after_cell_epoch = target.retarget_epoch;
        assert!(!target.apply_snapshot(RuntimeTargetSnapshot::tracked(
            RenderPoint {
                row: 8.0,
                col: 19.0,
            },
            CursorShape::vertical_bar(),
            &resized_tracked_cursor,
        )));
        assert_eq!(target.retarget_epoch, after_cell_epoch.wrapping_add(1));
    }
}
