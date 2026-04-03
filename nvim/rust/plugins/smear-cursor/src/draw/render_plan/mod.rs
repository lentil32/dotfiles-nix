use crate::core::types::ArcLenQ16;
use crate::draw::PARTICLE_ZINDEX_OFFSET;
use crate::octant_chars::OCTANT_CHARACTERS;
use crate::types::Point;
use crate::types::RenderFrame;
use crate::types::smoothstep01;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::VecDeque;
use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;
use std::hash::Hasher;

#[path = "../render/cell_draw.rs"]
mod cell_draw;
mod compile;
mod decode_candidates;
#[path = "../render/geometry.rs"]
mod geometry;
mod infra;
#[path = "../render/latent_field.rs"]
mod latent_field;
mod local_envelope;
#[path = "../render/particles.rs"]
mod particles;
mod solver;
use self::compile::compile_render_frame;
#[cfg(test)]
use self::compile::compile_render_frame_reference;
#[cfg(test)]
use self::decode_candidates::build_cell_candidates;
use self::decode_candidates::decode_locally;
use self::decode_candidates::non_empty_candidates;
use self::decode_candidates::populate_cell_candidates_in_bounds_with_scratch;
use self::decode_candidates::populate_cell_candidates_with_scratch;
use self::infra::candidates::*;
pub(crate) use self::infra::shared::CellOp;
pub(crate) use self::infra::shared::ClearOp;
pub(crate) use self::infra::shared::Glyph;
pub(crate) use self::infra::shared::HighlightLevel;
pub(crate) use self::infra::shared::HighlightRef;
#[cfg(test)]
pub(crate) use self::infra::shared::ParticleOp;
pub(crate) use self::infra::shared::PlannerOutput;
pub(crate) use self::infra::shared::PlannerState;
pub(crate) use self::infra::shared::RenderPlan;
pub(crate) use self::infra::shared::TargetCellOverlay;
pub(crate) use self::infra::shared::Viewport;
use self::infra::shared::*;
use self::latent_field::AgeMoment;
use self::latent_field::CompiledCell;
use self::latent_field::DepositedSlice;
use self::latent_field::MICRO_H;
use self::latent_field::MICRO_W;
use self::latent_field::MicroTile;
use self::latent_field::TailBand;
#[cfg(test)]
use self::local_envelope::CellRowIndex;
#[cfg(test)]
use self::local_envelope::CellRowIndexScratch;
use self::local_envelope::SliceSearchBounds;
#[cfg(test)]
use self::local_envelope::build_ribbon_slices;
#[cfg(test)]
use self::local_envelope::build_ribbon_slices_with_compiled;
use self::local_envelope::build_ribbon_slices_with_compiled_and_scratch;
#[cfg(test)]
use self::local_envelope::comet_target_width_cells;
#[cfg(test)]
use self::local_envelope::compute_local_query_envelope;
use self::local_envelope::fast_path_query_bounds;
use self::local_envelope::populate_resampled_centerline_with_scratch;
#[cfg(test)]
use self::local_envelope::resample_centerline;
#[cfg(test)]
use self::local_envelope::to_q16;
use self::particles::draw_particles;
#[cfg(test)]
use self::solver::RunEnumerationCursor;
#[cfg(test)]
use self::solver::RunEnumerationInput;
#[cfg(test)]
use self::solver::active_support_is_disconnected;
#[cfg(test)]
use self::solver::adjusted_candidate_cost;
use self::solver::aspect_metric_distance;
#[cfg(test)]
use self::solver::build_slice_states;
#[cfg(test)]
use self::solver::build_slice_states_with_peak_working_set;
#[cfg(test)]
use self::solver::decode_compiled_field;
#[cfg(test)]
use self::solver::decode_compiled_field_trace;
#[cfg(test)]
use self::solver::decode_compiled_field_trace_with_compiled_and_scratch;
use self::solver::decode_compiled_field_with_compiled_and_scratch;
#[cfg(test)]
use self::solver::decode_compiled_field_with_solver;
#[cfg(test)]
use self::solver::linear_cells_penalty;
#[cfg(test)]
use self::solver::merge_ribbon_assignments;
#[cfg(test)]
use self::solver::overlap_penalty;
use self::solver::push_decoded_cell;
#[cfg(test)]
use self::solver::ribbon_support_is_oversized;
#[cfg(test)]
use self::solver::run_width_cells;
#[cfg(test)]
use self::solver::sanitize_spatial_weight_q10;
use self::solver::sanitize_temporal_weight;
use self::solver::sanitize_top_k;
#[cfg(test)]
use self::solver::scale_penalty;
#[cfg(test)]
use self::solver::select_decode_path;
#[cfg(test)]
use self::solver::slice_peak_highlight_level;
#[cfg(test)]
use self::solver::slice_state_cmp;
#[cfg(test)]
use self::solver::solve_pairwise_fallback;
#[cfg(test)]
use self::solver::solve_pairwise_fallback_with_scratch;
#[cfg(test)]
use self::solver::solve_ribbon_dp;
#[cfg(test)]
use self::solver::squared_cells_penalty;
use self::solver::stage_deposited_samples;
#[cfg(test)]
use self::solver::state_for_slice_cell;
#[cfg(test)]
use self::solver::state_local_prior;
include!("lifecycle.rs");

#[cfg(test)]
mod tests;
