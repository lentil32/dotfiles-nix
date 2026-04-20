use super::ProjectionHandle;
#[cfg(test)]
use super::ProjectionReuseKey;
use super::ProjectionWitness;
use super::RetainedProjection;
use crate::core::realization::LogicalRaster;
use crate::core::realization::RealizationProjection;
#[cfg(test)]
use crate::draw::render_plan::PlannerState as ProjectionPlannerState;

// Cache-free projection of shell-visible truth. Equality intentionally ignores
// planner reuse state, reuse keys, and cached realization materialization.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct ProjectionSemanticView<'a> {
    witness: ProjectionWitness,
    logical_raster: &'a LogicalRaster,
}

// Shell apply is allowed to consume cached realization materialization, but it
// must opt in through this explicit boundary view instead of treating the whole
// retained projection as authoritative state.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct ShellProjection<'a> {
    witness: ProjectionWitness,
    logical_raster: &'a LogicalRaster,
    realization: &'a RealizationProjection,
}

impl ShellProjection<'_> {
    pub(crate) const fn witness(self) -> ProjectionWitness {
        self.witness
    }

    pub(crate) fn logical_raster(&self) -> &LogicalRaster {
        self.logical_raster
    }

    pub(crate) fn realization(&self) -> &RealizationProjection {
        self.realization
    }
}

impl RetainedProjection {
    pub(crate) fn semantic_view(&self) -> ProjectionSemanticView<'_> {
        ProjectionSemanticView {
            witness: self.witness(),
            logical_raster: self.cached_logical_raster(),
        }
    }

    pub(crate) fn shell_projection(&self) -> ShellProjection<'_> {
        ShellProjection {
            witness: self.witness(),
            logical_raster: self.cached_logical_raster(),
            realization: self.cached_realization(),
        }
    }
}

impl ProjectionHandle {
    pub(crate) fn witness(&self) -> ProjectionWitness {
        self.retained().witness()
    }

    pub(crate) fn semantic_view(&self) -> ProjectionSemanticView<'_> {
        self.retained().semantic_view()
    }

    pub(crate) fn shell_projection(&self) -> ShellProjection<'_> {
        self.retained().shell_projection()
    }

    pub(crate) fn same_render_output_as(&self, other: &Self) -> bool {
        // shell redraw authority is observation-bound, not just raster-bound.
        // If the reducer accepted a new projection witness, the shell must treat that as
        // replace work so animation can redraw after jumps, window changes, or other
        // observation churn even when the retained raster happens to be identical.
        self.semantic_view() == other.semantic_view()
    }

    #[cfg(test)]
    pub(crate) fn cached_logical_raster(&self) -> &LogicalRaster {
        self.retained().cached_logical_raster()
    }

    #[cfg(test)]
    pub(crate) fn cached_planner_state(&self) -> &ProjectionPlannerState {
        self.retained().cached_planner_state()
    }

    #[cfg(test)]
    pub(crate) fn cached_realization(&self) -> &RealizationProjection {
        self.retained().cached_realization()
    }

    #[cfg(test)]
    pub(crate) fn reuse_key(&self) -> ProjectionReuseKey {
        self.retained().reuse_key()
    }
}

#[cfg(test)]
mod tests {
    use super::ProjectionHandle;
    use super::ProjectionWitness;
    use super::RetainedProjection;
    use crate::core::realization::LogicalRaster;
    use crate::core::runtime_reducer::TargetCellPresentation;
    use crate::core::state::PatchBasis;
    use crate::core::state::ProjectionReuseKey;
    use crate::core::state::ScenePatchKind;
    use crate::core::types::IngressSeq;
    use crate::core::types::ObservationId;
    use crate::core::types::ProjectionPolicyRevision;
    use crate::core::types::ProjectorRevision;
    use crate::core::types::RenderRevision;
    use crate::draw::render_plan::CellOp;
    use crate::draw::render_plan::ClearOp;
    use crate::draw::render_plan::Glyph;
    use crate::draw::render_plan::HighlightLevel;
    use crate::draw::render_plan::HighlightRef;
    use crate::draw::render_plan::PlannerState as ProjectionPlannerState;
    use crate::position::ViewportBounds;
    use pretty_assertions::assert_eq;
    use std::sync::Arc;

    fn projection_witness() -> ProjectionWitness {
        ProjectionWitness::new(
            RenderRevision::INITIAL,
            ObservationId::from_ingress_seq(IngressSeq::new(7)),
            ViewportBounds::new(20, 40).expect("positive viewport bounds"),
            ProjectorRevision::CURRENT,
        )
    }

    fn projection_reuse_key(trail_signature: Option<u64>) -> ProjectionReuseKey {
        ProjectionReuseKey::new(
            trail_signature,
            None,
            None,
            TargetCellPresentation::None,
            ProjectionPolicyRevision::INITIAL,
        )
    }

    fn logical_raster() -> LogicalRaster {
        LogicalRaster::new(
            Some(ClearOp {
                max_kept_windows: 4,
            }),
            Arc::from([CellOp {
                row: 3,
                col: 5,
                zindex: 9,
                glyph: Glyph::Static("A"),
                highlight: HighlightRef::Normal(HighlightLevel::from_raw_clamped(1)),
            }]),
        )
    }

    fn retained_projection(trail_signature: Option<u64>) -> ProjectionHandle {
        RetainedProjection::new(
            projection_witness(),
            projection_reuse_key(trail_signature),
            ProjectionPlannerState::default(),
            logical_raster(),
        )
        .into_handle()
    }

    #[test]
    fn patch_kind_ignores_projection_reuse_key_cache_drift() {
        let acknowledged = retained_projection(Some(11));
        let target = retained_projection(Some(29));

        assert_eq!(acknowledged.semantic_view(), target.semantic_view());
        assert!(acknowledged.same_render_output_as(&target));
        assert_eq!(
            PatchBasis::new(Some(acknowledged), Some(target)).kind(),
            ScenePatchKind::Noop
        );
    }

    #[test]
    fn shell_projection_is_an_explicit_cached_materialization_view() {
        let projection = retained_projection(Some(11));
        let shell_projection = projection.shell_projection();

        assert_eq!(shell_projection.witness(), projection.witness());
        assert_eq!(
            shell_projection.logical_raster(),
            projection.cached_logical_raster()
        );
        assert_eq!(
            shell_projection.realization(),
            projection.cached_realization()
        );
    }
}
