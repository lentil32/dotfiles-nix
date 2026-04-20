use crate::core::realization::PaletteSpec;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HighlightPaletteKey {
    pub(super) cursor_color: u32,
    pub(super) normal_background: Option<u32>,
    pub(super) transparent_fallback: u32,
    pub(super) non_inverted_blend: u8,
    pub(super) color_levels: u32,
    pub(super) gamma_bits: u64,
    pub(super) cterm_cursor_colors: Option<Vec<u16>>,
    pub(super) cterm_bg: Option<u16>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct RawPaletteInputKey {
    pub(super) fingerprint: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct PendingPaletteRefresh {
    pub(super) raw_input_key: RawPaletteInputKey,
    pub(super) spec: PaletteSpec,
}

#[derive(Clone, Debug, PartialEq)]
enum DeferredPaletteRefreshPhase {
    Idle,
    // Active covers both a queued callback and a callback currently draining.
    Active {
        pending: Option<PendingPaletteRefresh>,
    },
}

#[derive(Clone, Debug, PartialEq)]
struct DeferredPaletteRefreshState {
    phase: DeferredPaletteRefreshPhase,
    epoch: u64,
}

impl DeferredPaletteRefreshState {
    fn new(epoch: u64) -> Self {
        Self {
            phase: DeferredPaletteRefreshPhase::Idle,
            epoch,
        }
    }

    fn invalidate(&mut self) {
        self.phase = DeferredPaletteRefreshPhase::Idle;
        self.epoch = self.epoch.wrapping_add(1);
    }

    fn stage(
        &mut self,
        spec: &PaletteSpec,
        raw_input_key: RawPaletteInputKey,
    ) -> PaletteRefreshDisposition {
        match &mut self.phase {
            DeferredPaletteRefreshPhase::Idle => {
                self.phase = DeferredPaletteRefreshPhase::Active {
                    pending: Some(PendingPaletteRefresh {
                        raw_input_key,
                        spec: spec.clone(),
                    }),
                };
                PaletteRefreshDisposition::ScheduleDeferred { epoch: self.epoch }
            }
            DeferredPaletteRefreshPhase::Active { pending } => {
                if pending
                    .as_ref()
                    .is_none_or(|pending| pending.raw_input_key != raw_input_key)
                {
                    *pending = Some(PendingPaletteRefresh {
                        raw_input_key,
                        spec: spec.clone(),
                    });
                }

                PaletteRefreshDisposition::DeferredAlreadyScheduled
            }
        }
    }

    fn poll(&mut self, expected_epoch: u64) -> DeferredPaletteRefreshPoll {
        if self.epoch != expected_epoch {
            return DeferredPaletteRefreshPoll::StaleEpoch;
        }

        match &mut self.phase {
            DeferredPaletteRefreshPhase::Idle => DeferredPaletteRefreshPoll::Idle,
            DeferredPaletteRefreshPhase::Active { pending } => match pending.take() {
                Some(pending_refresh) => DeferredPaletteRefreshPoll::Run(pending_refresh),
                None => {
                    self.phase = DeferredPaletteRefreshPhase::Idle;
                    DeferredPaletteRefreshPoll::Idle
                }
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct PaletteCoreState {
    raw_input_key: Option<RawPaletteInputKey>,
    palette_key: Option<HighlightPaletteKey>,
    deferred_refresh: DeferredPaletteRefreshState,
}

impl PaletteCoreState {
    pub(super) fn new() -> Self {
        Self::new_with_epoch(0)
    }

    pub(super) fn new_with_epoch(deferred_refresh_epoch: u64) -> Self {
        Self {
            raw_input_key: None,
            palette_key: None,
            deferred_refresh: DeferredPaletteRefreshState::new(deferred_refresh_epoch),
        }
    }

    pub(super) fn epoch(&self) -> u64 {
        self.deferred_refresh.epoch
    }

    pub(super) fn clear(&mut self) -> Option<u32> {
        let previous_levels = self
            .palette_key
            .as_ref()
            .map(|palette| palette.color_levels);
        self.raw_input_key = None;
        self.palette_key = None;
        self.deferred_refresh.invalidate();
        previous_levels
    }

    pub(super) fn stage_refresh(
        &mut self,
        spec: &PaletteSpec,
        raw_input_key: RawPaletteInputKey,
    ) -> PaletteRefreshDisposition {
        if self.raw_input_key == Some(raw_input_key) {
            return PaletteRefreshDisposition::Ready;
        }

        if self.palette_key.is_some() {
            self.deferred_refresh.stage(spec, raw_input_key)
        } else {
            PaletteRefreshDisposition::BootstrapSynchronously
        }
    }

    pub(super) fn prepare_refresh(
        &mut self,
        raw_input_key: RawPaletteInputKey,
        resolved_palette: &HighlightPaletteKey,
    ) -> PaletteRefreshPlan {
        if self.raw_input_key == Some(raw_input_key) {
            return PaletteRefreshPlan::ReuseCommitted;
        }

        match &self.palette_key {
            Some(palette_key) if palette_key == resolved_palette => {
                self.raw_input_key = Some(raw_input_key);
                PaletteRefreshPlan::ReuseCommitted
            }
            Some(palette_key) => PaletteRefreshPlan::Apply {
                previous_levels: Some(palette_key.color_levels),
            },
            None => PaletteRefreshPlan::Apply {
                previous_levels: None,
            },
        }
    }

    pub(super) fn commit_refresh(
        &mut self,
        raw_input_key: RawPaletteInputKey,
        palette_key: HighlightPaletteKey,
    ) {
        self.raw_input_key = Some(raw_input_key);
        self.palette_key = Some(palette_key);
    }

    pub(super) fn poll_deferred_refresh(
        &mut self,
        expected_epoch: u64,
    ) -> DeferredPaletteRefreshPoll {
        self.deferred_refresh.poll(expected_epoch)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PaletteRefreshDisposition {
    Ready,
    DeferredAlreadyScheduled,
    BootstrapSynchronously,
    ScheduleDeferred { epoch: u64 },
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum DeferredPaletteRefreshPoll {
    Run(PendingPaletteRefresh),
    Idle,
    StaleEpoch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PaletteRefreshPlan {
    ReuseCommitted,
    Apply { previous_levels: Option<u32> },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::StrokeId;
    use crate::position::RenderPoint;
    use crate::types::ModeClass;
    use crate::types::RenderFrame;
    use crate::types::StaticRenderConfig;
    use pretty_assertions::assert_eq;
    use std::sync::Arc;

    fn test_palette_spec() -> PaletteSpec {
        PaletteSpec::from_frame(&RenderFrame {
            mode: ModeClass::NormalLike,
            corners: [RenderPoint::ZERO; 4],
            step_samples: Vec::new().into(),
            planner_idle_steps: 0,
            target: RenderPoint::ZERO,
            target_corners: [RenderPoint::ZERO; 4],
            vertical_bar: false,
            trail_stroke_id: StrokeId::INITIAL,
            retarget_epoch: 0,
            particle_count: 0,
            aggregated_particle_cells: Arc::default(),
            particle_screen_cells: Arc::default(),
            color_at_cursor: Some(0x00FF_FFFF),
            projection_policy_revision: crate::core::types::ProjectionPolicyRevision::INITIAL,
            static_config: Arc::new(StaticRenderConfig {
                cursor_color: Some("#112233".to_string()),
                cursor_color_insert_mode: Some("none".to_string()),
                normal_bg: Some("#202020".to_string()),
                transparent_bg_fallback_color: "#303030".to_string(),
                cterm_cursor_colors: Some(vec![17_u16, 42_u16]),
                cterm_bg: Some(235_u16),
                hide_target_hack: false,
                max_kept_windows: 32,
                never_draw_over_target: false,
                particle_max_lifetime: 250.0,
                particle_switch_octant_braille: 0.5,
                particles_over_text: true,
                color_levels: 16,
                gamma: 2.2,
                block_aspect_ratio: 0.5,
                tail_duration_ms: 120.0,
                simulation_hz: 120.0,
                trail_thickness: 1.0,
                trail_thickness_x: 1.0,
                spatial_coherence_weight: 0.0,
                temporal_stability_weight: 0.0,
                top_k_per_cell: 4,
                windows_zindex: 50,
            }),
        })
    }

    fn palette_key() -> HighlightPaletteKey {
        HighlightPaletteKey {
            cursor_color: 0x112233,
            normal_background: Some(0x202020),
            transparent_fallback: 0x303030,
            non_inverted_blend: 0,
            color_levels: 16,
            gamma_bits: 2.2_f64.to_bits(),
            cterm_cursor_colors: Some(vec![17_u16, 42_u16]),
            cterm_bg: Some(235_u16),
        }
    }

    fn different_palette_key() -> HighlightPaletteKey {
        HighlightPaletteKey {
            transparent_fallback: 0x404040,
            ..palette_key()
        }
    }

    fn different_raw_key(raw_input_key: RawPaletteInputKey) -> RawPaletteInputKey {
        RawPaletteInputKey {
            fingerprint: raw_input_key.fingerprint ^ 1,
        }
    }

    fn pending_palette_refresh(
        spec: &PaletteSpec,
        raw_input_key: RawPaletteInputKey,
    ) -> PendingPaletteRefresh {
        PendingPaletteRefresh {
            raw_input_key,
            spec: spec.clone(),
        }
    }

    fn active_core_state(
        palette_key: Option<HighlightPaletteKey>,
        raw_input_key: Option<RawPaletteInputKey>,
        epoch: u64,
        pending: Option<PendingPaletteRefresh>,
    ) -> PaletteCoreState {
        PaletteCoreState {
            raw_input_key,
            palette_key,
            deferred_refresh: DeferredPaletteRefreshState {
                phase: DeferredPaletteRefreshPhase::Active { pending },
                epoch,
            },
        }
    }

    #[test]
    fn stage_refresh_returns_ready_when_raw_key_matches_committed_input() {
        let spec = test_palette_spec();
        let raw_input_key = RawPaletteInputKey { fingerprint: 7 };
        let mut state = PaletteCoreState {
            raw_input_key: Some(raw_input_key),
            palette_key: Some(palette_key()),
            deferred_refresh: DeferredPaletteRefreshState::new(11),
        };
        let expected_state = state.clone();

        assert_eq!(
            state.stage_refresh(&spec, raw_input_key),
            PaletteRefreshDisposition::Ready
        );
        assert_eq!(state, expected_state);
    }

    #[test]
    fn stage_refresh_bootstraps_when_no_palette_is_committed() {
        let spec = test_palette_spec();
        let raw_input_key = RawPaletteInputKey { fingerprint: 7 };
        let mut state = active_core_state(None, Some(different_raw_key(raw_input_key)), 11, None);
        let expected_state = state.clone();

        assert_eq!(
            state.stage_refresh(&spec, raw_input_key),
            PaletteRefreshDisposition::BootstrapSynchronously
        );
        assert_eq!(state, expected_state);
    }

    #[test]
    fn stage_refresh_schedules_once_and_deduplicates_matching_pending_request() {
        let spec = test_palette_spec();
        let raw_input_key = RawPaletteInputKey { fingerprint: 7 };
        let mut state = PaletteCoreState::new_with_epoch(11);
        state.palette_key = Some(palette_key());

        assert_eq!(
            state.stage_refresh(&spec, raw_input_key),
            PaletteRefreshDisposition::ScheduleDeferred { epoch: 11 }
        );
        assert_eq!(
            state.stage_refresh(&spec, raw_input_key),
            PaletteRefreshDisposition::DeferredAlreadyScheduled
        );
        assert_eq!(
            state,
            active_core_state(
                Some(palette_key()),
                None,
                11,
                Some(pending_palette_refresh(&spec, raw_input_key)),
            )
        );
    }

    #[test]
    fn stage_refresh_replaces_stale_pending_request_under_palette_churn() {
        let spec = test_palette_spec();
        let raw_input_key = RawPaletteInputKey { fingerprint: 7 };
        let newer_raw_input_key = RawPaletteInputKey { fingerprint: 9 };
        let mut state = active_core_state(
            Some(palette_key()),
            Some(different_raw_key(newer_raw_input_key)),
            41,
            Some(pending_palette_refresh(&spec, raw_input_key)),
        );

        assert_eq!(
            state.stage_refresh(&spec, newer_raw_input_key),
            PaletteRefreshDisposition::DeferredAlreadyScheduled
        );
        assert_eq!(
            state,
            active_core_state(
                Some(palette_key()),
                Some(different_raw_key(newer_raw_input_key)),
                41,
                Some(pending_palette_refresh(&spec, newer_raw_input_key)),
            )
        );
    }

    #[test]
    fn prepare_refresh_reuses_committed_palette_on_raw_hit_without_mutation() {
        let raw_input_key = RawPaletteInputKey { fingerprint: 7 };
        let mut state = PaletteCoreState {
            raw_input_key: Some(raw_input_key),
            palette_key: Some(palette_key()),
            deferred_refresh: DeferredPaletteRefreshState::new(3),
        };
        let expected_state = state.clone();

        assert_eq!(
            state.prepare_refresh(raw_input_key, &different_palette_key()),
            PaletteRefreshPlan::ReuseCommitted
        );
        assert_eq!(state, expected_state);
    }

    #[test]
    fn prepare_refresh_reuses_committed_palette_on_resolved_hit_and_updates_raw_key() {
        let raw_input_key = RawPaletteInputKey { fingerprint: 7 };
        let mut state = PaletteCoreState {
            raw_input_key: Some(different_raw_key(raw_input_key)),
            palette_key: Some(palette_key()),
            deferred_refresh: DeferredPaletteRefreshState::new(3),
        };

        assert_eq!(
            state.prepare_refresh(raw_input_key, &palette_key()),
            PaletteRefreshPlan::ReuseCommitted
        );
        assert_eq!(
            state,
            PaletteCoreState {
                raw_input_key: Some(raw_input_key),
                palette_key: Some(palette_key()),
                deferred_refresh: DeferredPaletteRefreshState::new(3),
            }
        );
    }

    #[test]
    fn prepare_refresh_requests_apply_when_resolved_palette_changes() {
        let raw_input_key = RawPaletteInputKey { fingerprint: 7 };
        let mut state = PaletteCoreState {
            raw_input_key: Some(different_raw_key(raw_input_key)),
            palette_key: Some(palette_key()),
            deferred_refresh: DeferredPaletteRefreshState::new(3),
        };
        let expected_state = state.clone();

        assert_eq!(
            state.prepare_refresh(raw_input_key, &different_palette_key()),
            PaletteRefreshPlan::Apply {
                previous_levels: Some(16),
            }
        );
        assert_eq!(state, expected_state);
    }

    #[test]
    fn clear_drops_committed_palette_and_invalidates_deferred_refresh() {
        let spec = test_palette_spec();
        let raw_input_key = RawPaletteInputKey { fingerprint: 7 };
        let mut state = active_core_state(
            Some(palette_key()),
            Some(raw_input_key),
            17,
            Some(pending_palette_refresh(&spec, raw_input_key)),
        );

        assert_eq!(state.clear(), Some(16));
        assert_eq!(state, PaletteCoreState::new_with_epoch(18));
    }

    #[test]
    fn poll_deferred_refresh_ignores_stale_epochs() {
        let spec = test_palette_spec();
        let raw_input_key = RawPaletteInputKey { fingerprint: 7 };
        let mut state = active_core_state(
            Some(palette_key()),
            Some(raw_input_key),
            17,
            Some(pending_palette_refresh(&spec, raw_input_key)),
        );
        let expected_state = state.clone();

        assert_eq!(
            state.poll_deferred_refresh(18),
            DeferredPaletteRefreshPoll::StaleEpoch
        );
        assert_eq!(state, expected_state);
    }

    #[test]
    fn poll_deferred_refresh_returns_run_then_idle() {
        let spec = test_palette_spec();
        let raw_input_key = RawPaletteInputKey { fingerprint: 7 };
        let mut state = active_core_state(
            Some(palette_key()),
            Some(raw_input_key),
            17,
            Some(pending_palette_refresh(&spec, raw_input_key)),
        );

        assert_eq!(
            state.poll_deferred_refresh(17),
            DeferredPaletteRefreshPoll::Run(pending_palette_refresh(&spec, raw_input_key))
        );
        assert_eq!(
            state.poll_deferred_refresh(17),
            DeferredPaletteRefreshPoll::Idle
        );
        assert_eq!(
            state,
            PaletteCoreState {
                raw_input_key: Some(raw_input_key),
                palette_key: Some(palette_key()),
                deferred_refresh: DeferredPaletteRefreshState::new(17),
            }
        );
    }

    #[test]
    fn commit_refresh_sets_committed_palette_and_raw_input_key() {
        let raw_input_key = RawPaletteInputKey { fingerprint: 7 };
        let mut state = PaletteCoreState::new_with_epoch(17);

        state.commit_refresh(raw_input_key, palette_key());

        assert_eq!(
            state,
            PaletteCoreState {
                raw_input_key: Some(raw_input_key),
                palette_key: Some(palette_key()),
                deferred_refresh: DeferredPaletteRefreshState::new(17),
            }
        );
    }
}
