use super::CursorLocation;
use super::CursorShape;
use crate::core::types::StrokeId;
use crate::types::Particle;
use crate::types::ParticleAggregationScratch;
use crate::types::Point;
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
    pub(super) fn new(remaining_steps: u32) -> Option<Self> {
        NonZeroU32::new(remaining_steps).map(|remaining_steps| Self {
            clock: MotionClock::default(),
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

#[derive(Debug, Clone, PartialEq)]
pub(super) struct CursorTarget {
    pub(super) position: Point,
    pub(super) shape: CursorShape,
    pub(super) tracked_location: Option<CursorLocation>,
    pub(super) retarget_epoch: u64,
}

impl Default for CursorTarget {
    fn default() -> Self {
        Self {
            position: Point::ZERO,
            shape: CursorShape::new(false, false),
            tracked_location: None,
            retarget_epoch: 0,
        }
    }
}

impl CursorTarget {
    pub(super) fn corners(&self) -> [Point; 4] {
        self.shape.corners(self.position)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct TrailState {
    pub(super) stroke_id: StrokeId,
    pub(super) origin_corners: [Point; 4],
    pub(super) elapsed_ms: [f64; 4],
}

impl Default for TrailState {
    fn default() -> Self {
        Self {
            stroke_id: StrokeId::INITIAL,
            origin_corners: [Point::ZERO; 4],
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
