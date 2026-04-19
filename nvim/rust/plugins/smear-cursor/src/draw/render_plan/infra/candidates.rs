use super::super::latent_field::AgeMoment;
use super::super::latent_field::MICRO_H;
use super::super::latent_field::MICRO_TILE_SAMPLES;
use super::super::latent_field::MICRO_W;
use super::super::latent_field::MicroTile;
use super::shared::CellCandidate;
use super::shared::DecodedCellState;
use super::shared::DecodedGlyph;
use super::shared::HighlightLevel;
use super::shared::ShadeProfile;
use std::cmp::Ordering;
use std::sync::LazyLock;

#[derive(Clone, Copy, Debug)]
pub(crate) struct GlyphProfile {
    pub(crate) glyph: DecodedGlyph,
    pub(crate) sample_count: usize,
    pub(crate) complexity: u8,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct LocalCellProfile {
    pub(crate) state: DecodedCellState,
    pub(crate) glyph: GlyphProfile,
    pub(crate) shade: ShadeProfile,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct GlyphBucketLayout {
    pub(crate) matrix_bucket_for_sample: [u8; MICRO_TILE_SAMPLES],
    pub(crate) octant_bucket_for_sample: [u8; MICRO_TILE_SAMPLES],
    pub(crate) matrix_sample_counts_by_mask: [u8; MATRIX_MASK_LIMIT],
    pub(crate) octant_sample_counts_by_mask: [u8; OCTANT_MASK_LIMIT],
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct PatchCandidateBasis {
    pub(crate) empty_residual: u64,
    pub(crate) total_mass: u64,
    pub(crate) matrix_bucket_sums: [u64; MATRIX_BUCKET_COUNT],
    pub(crate) octant_bucket_sums: [u64; OCTANT_BUCKET_COUNT],
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ShadeProfileIndexSet {
    pub(crate) indices: [usize; MAX_SHADE_PROFILE_CANDIDATES],
    pub(crate) len: usize,
}

impl ShadeProfileIndexSet {
    pub(crate) fn push_unique(&mut self, index: usize) {
        if self.indices[..self.len].contains(&index) {
            return;
        }
        if self.len >= self.indices.len() {
            return;
        }
        self.indices[self.len] = index;
        self.len += 1;
    }

    pub(crate) fn as_slice(&self) -> &[usize] {
        &self.indices[..self.len]
    }
}

pub(crate) const MATRIX_CHARACTERS: [&str; 16] = [
    "", "▘", "▝", "▀", "▖", "▌", "▞", "▛", "▗", "▚", "▐", "▜", "▄", "▙", "▟", "█",
];
pub(crate) const MATRIX_BUCKET_COUNT: usize = 4;
pub(crate) const MATRIX_MASK_LIMIT: usize = 1 << MATRIX_BUCKET_COUNT;
pub(crate) const OCTANT_BUCKET_COUNT: usize = 8;
pub(crate) const OCTANT_MASK_LIMIT: usize = 1 << OCTANT_BUCKET_COUNT;
pub(crate) const MIN_VISIBLE_SAMPLE_Q12: u16 = 6;
pub(crate) const SHADE_PROFILE_NEIGHBORHOOD: usize = 1;
pub(crate) const MAX_SHADE_PROFILE_CANDIDATES: usize = SHADE_PROFILE_NEIGHBORHOOD * 2 + 2;
pub(crate) const PRIOR_COMPLEXITY_WEIGHT: u64 = 48;
pub(crate) const TRANSITION_SCALE: f64 = 1024.0;

#[cfg(test)]
pub(crate) const MATRIX_BIT_WEIGHTS: [[u8; 2]; 2] = [[1, 2], [4, 8]];
#[cfg(test)]
pub(crate) const OCTANT_BIT_WEIGHTS: [[u8; 2]; 4] = [[1, 2], [4, 8], [16, 32], [64, 128]];

pub(crate) static GLYPH_BUCKET_LAYOUT: LazyLock<GlyphBucketLayout> =
    LazyLock::new(build_glyph_bucket_layout);

pub(crate) fn build_glyph_bucket_layout() -> GlyphBucketLayout {
    let mut matrix_bucket_for_sample = [0_u8; MICRO_TILE_SAMPLES];
    let mut octant_bucket_for_sample = [0_u8; MICRO_TILE_SAMPLES];
    let mut matrix_bucket_sample_counts = [0_u8; MATRIX_BUCKET_COUNT];
    let mut octant_bucket_sample_counts = [0_u8; OCTANT_BUCKET_COUNT];

    for sample_row in 0..MICRO_H {
        for sample_col in 0..MICRO_W {
            let index = sample_row * MICRO_W + sample_col;

            let matrix_row_bucket = (sample_row * 2) / MICRO_H;
            let matrix_col_bucket = (sample_col * 2) / MICRO_W;
            let matrix_bucket = matrix_row_bucket * 2 + matrix_col_bucket;
            matrix_bucket_for_sample[index] = u8::try_from(matrix_bucket).unwrap_or(u8::MAX);
            matrix_bucket_sample_counts[matrix_bucket] =
                matrix_bucket_sample_counts[matrix_bucket].saturating_add(1);

            let octant_row_bucket = (sample_row * 4) / MICRO_H;
            let octant_col_bucket = (sample_col * 2) / MICRO_W;
            let octant_bucket = octant_row_bucket * 2 + octant_col_bucket;
            octant_bucket_for_sample[index] = u8::try_from(octant_bucket).unwrap_or(u8::MAX);
            octant_bucket_sample_counts[octant_bucket] =
                octant_bucket_sample_counts[octant_bucket].saturating_add(1);
        }
    }

    GlyphBucketLayout {
        matrix_bucket_for_sample,
        octant_bucket_for_sample,
        matrix_sample_counts_by_mask: build_mask_sample_counts(&matrix_bucket_sample_counts),
        octant_sample_counts_by_mask: build_mask_sample_counts(&octant_bucket_sample_counts),
    }
}

pub(crate) fn build_shade_profiles_into(color_levels: u32, shades: &mut Vec<ShadeProfile>) {
    shades.clear();
    let capacity = usize::try_from(color_levels).unwrap_or(0);
    shades.reserve(capacity.saturating_sub(shades.len()));
    for raw_level in 1..=color_levels {
        let Some(level) = HighlightLevel::try_new(raw_level) else {
            continue;
        };
        shades.push(ShadeProfile {
            level,
            sample_q12: quantized_level_to_sample_q12(level, color_levels),
        });
    }
}

#[cfg(test)]
pub(crate) fn build_shade_profiles(color_levels: u32) -> Vec<ShadeProfile> {
    let mut shades = Vec::new();
    build_shade_profiles_into(color_levels, &mut shades);
    shades
}

pub(crate) fn quantized_level_to_sample_q12(level: HighlightLevel, color_levels: u32) -> u16 {
    if color_levels == 0 {
        return 0;
    }
    let numerator = u64::from(level.value()).saturating_mul(4095_u64);
    let denominator = u64::from(color_levels).max(1);
    (numerator / denominator).min(u64::from(u16::MAX)) as u16
}

impl LocalCellProfile {
    pub(crate) fn new(glyph: GlyphProfile, shade: ShadeProfile) -> Self {
        Self {
            state: DecodedCellState {
                glyph: glyph.glyph,
                level: shade.level,
            },
            glyph,
            shade,
        }
    }
}

impl GlyphProfile {
    pub(crate) fn block() -> Self {
        Self {
            glyph: DecodedGlyph::Block,
            sample_count: MICRO_TILE_SAMPLES,
            complexity: u8::try_from(MICRO_TILE_SAMPLES).unwrap_or(u8::MAX),
        }
    }

    pub(crate) fn matrix(mask: u8, sample_count: usize) -> Self {
        Self {
            glyph: DecodedGlyph::Matrix(mask),
            sample_count,
            complexity: mask.count_ones() as u8,
        }
    }

    pub(crate) fn octant(mask: u8, sample_count: usize) -> Self {
        Self {
            glyph: DecodedGlyph::Octant(mask),
            sample_count,
            complexity: mask.count_ones() as u8,
        }
    }
}

impl GlyphBucketLayout {
    pub(crate) fn matrix_sample_count(&self, mask: u8) -> usize {
        usize::from(self.matrix_sample_counts_by_mask[usize::from(mask)])
    }

    pub(crate) fn octant_sample_count(&self, mask: u8) -> usize {
        usize::from(self.octant_sample_counts_by_mask[usize::from(mask)])
    }
}

impl PatchCandidateBasis {
    pub(crate) fn from_patch(patch: MicroTile) -> Self {
        let layout = &*GLYPH_BUCKET_LAYOUT;
        let mut basis = Self::default();

        // All glyph families are unions of these micro-buckets, so one patch pass recovers the
        // exact per-glyph dot products that the old occupied-sample scan computed.
        for (index, sample_q12) in patch.samples_q12.iter().copied().enumerate() {
            let value = u64::from(sample_q12);
            basis.empty_residual = basis
                .empty_residual
                .saturating_add(value.saturating_mul(value));
            basis.total_mass = basis.total_mass.saturating_add(value);

            let matrix_bucket = usize::from(layout.matrix_bucket_for_sample[index]);
            basis.matrix_bucket_sums[matrix_bucket] =
                basis.matrix_bucket_sums[matrix_bucket].saturating_add(value);

            let octant_bucket = usize::from(layout.octant_bucket_for_sample[index]);
            basis.octant_bucket_sums[octant_bucket] =
                basis.octant_bucket_sums[octant_bucket].saturating_add(value);
        }

        basis
    }
}

pub(crate) fn build_mask_sample_counts<const BUCKETS: usize, const MASK_LIMIT: usize>(
    bucket_sample_counts: &[u8; BUCKETS],
) -> [u8; MASK_LIMIT] {
    let mut sample_counts = [0_u8; MASK_LIMIT];
    let mut mask = 1_usize;
    while mask < MASK_LIMIT {
        let lsb_index = mask.trailing_zeros() as usize;
        let previous = mask & (mask - 1);
        sample_counts[mask] =
            sample_counts[previous].saturating_add(bucket_sample_counts[lsb_index]);
        mask += 1;
    }
    sample_counts
}

pub(crate) fn build_subset_sums<const BUCKETS: usize, const MASK_LIMIT: usize>(
    bucket_sums: &[u64; BUCKETS],
) -> [u64; MASK_LIMIT] {
    let mut subset_sums = [0_u64; MASK_LIMIT];
    let mut mask = 1_usize;
    while mask < MASK_LIMIT {
        let lsb_index = mask.trailing_zeros() as usize;
        let previous = mask & (mask - 1);
        subset_sums[mask] = subset_sums[previous].saturating_add(bucket_sums[lsb_index]);
        mask += 1;
    }
    subset_sums
}

pub(crate) fn nearest_shade_profile_index(
    shades: &[ShadeProfile],
    alpha_q12: u16,
) -> Option<usize> {
    let shade_count = shades.len();
    if shade_count == 0 {
        return None;
    }

    let shade_count_u64 = u64::try_from(shade_count).unwrap_or(u64::MAX);
    let rounded_level = u64::from(alpha_q12)
        .saturating_mul(shade_count_u64)
        .saturating_add(2047)
        .saturating_div(4095)
        .clamp(1, shade_count_u64);
    let candidate_index = usize::try_from(rounded_level.saturating_sub(1)).unwrap_or(0);
    let start = candidate_index.saturating_sub(1);
    let end = candidate_index
        .saturating_add(1)
        .min(shade_count.saturating_sub(1));

    (start..=end).min_by_key(|index| shades[*index].sample_q12.abs_diff(alpha_q12))
}

pub(crate) fn previous_shade_profile_index(
    shades: &[ShadeProfile],
    previous: Option<DecodedCellState>,
    glyph: DecodedGlyph,
) -> Option<usize> {
    let DecodedCellState {
        glyph: previous_glyph,
        level,
    } = previous?;
    if previous_glyph != glyph {
        return None;
    }

    let index = usize::try_from(level.value().saturating_sub(1)).ok()?;
    (index < shades.len()).then_some(index)
}

pub(crate) fn shade_profile_indices_for_glyph(
    shades: &[ShadeProfile],
    alpha_q12: u16,
    previous: Option<DecodedCellState>,
    glyph: DecodedGlyph,
) -> ShadeProfileIndexSet {
    let Some(nearest_index) = nearest_shade_profile_index(shades, alpha_q12) else {
        return ShadeProfileIndexSet::default();
    };
    let start = nearest_index.saturating_sub(SHADE_PROFILE_NEIGHBORHOOD);
    let end = nearest_index
        .saturating_add(SHADE_PROFILE_NEIGHBORHOOD)
        .min(shades.len().saturating_sub(1));
    let previous_index = previous_shade_profile_index(shades, previous, glyph);
    let mut indices = ShadeProfileIndexSet::default();

    if let Some(previous_index) = previous_index.filter(|index| *index < start) {
        indices.push_unique(previous_index);
    }

    for index in start..=end {
        indices.push_unique(index);
    }

    // Surprising: temporal stability cannot suppress shade flicker if local fitting only offers
    // one quantized level per glyph, so we keep a tiny neighboring shade ladder alive here.
    if let Some(previous_index) = previous_index.filter(|index| *index > end) {
        indices.push_unique(previous_index);
    }
    indices
}

pub(crate) fn residual_cost(empty_residual: u64, dot: u64, profile: LocalCellProfile) -> u64 {
    let shade = i128::from(profile.shade.sample_q12);
    let occupied_mass = i128::try_from(profile.glyph.sample_count).unwrap_or(i128::MAX);
    let residual = i128::from(empty_residual) + occupied_mass * shade * shade
        - 2_i128 * shade * i128::from(dot);
    debug_assert!(residual >= 0);
    residual.max(0) as u64
}

pub(crate) fn state_sort_key(state: Option<DecodedCellState>) -> u32 {
    match state {
        None => 0,
        Some(DecodedCellState {
            glyph: DecodedGlyph::Block,
            level,
        }) => 100_000 + level.value(),
        Some(DecodedCellState {
            glyph: DecodedGlyph::Matrix(mask),
            level,
        }) => 200_000 + u32::from(mask) * 256 + level.value(),
        Some(DecodedCellState {
            glyph: DecodedGlyph::Octant(mask),
            level,
        }) => 300_000 + u32::from(mask) * 256 + level.value(),
    }
}

pub(crate) fn temporal_transition_distance(
    previous: Option<DecodedCellState>,
    next: Option<DecodedCellState>,
) -> u32 {
    match (previous, next) {
        (None, None) => 0,
        (None, Some(_)) | (Some(_), None) => 48,
        (Some(prev), Some(next_state)) if prev == next_state => 0,
        (Some(prev), Some(next_state)) if prev.glyph == next_state.glyph => {
            prev.level.abs_diff(next_state.level) * 2
        }
        (Some(prev), Some(next_state)) => {
            24_u32.saturating_add(prev.level.abs_diff(next_state.level))
        }
    }
}

pub(crate) fn temporal_cost(
    previous: Option<DecodedCellState>,
    next: Option<DecodedCellState>,
    age: AgeMoment,
    temporal_stability_weight: f64,
) -> u64 {
    let total_mass = age.total_mass_q12;
    let head_ratio = if total_mass == 0 {
        1.0
    } else {
        (age.recent_mass_q12 as f64 / total_mass as f64).clamp(0.0, 1.0)
    };

    let adaptive_weight = if temporal_stability_weight.is_finite() {
        temporal_stability_weight.max(0.0) * (1.0 - head_ratio)
    } else {
        0.0
    };

    let transition = temporal_transition_distance(previous, next) as f64;
    (adaptive_weight * transition * TRANSITION_SCALE)
        .round()
        .max(0.0) as u64
}

pub(crate) fn candidate_cmp(
    lhs: CellCandidate,
    rhs: CellCandidate,
    previous: Option<DecodedCellState>,
) -> Ordering {
    lhs.unary_cost
        .cmp(&rhs.unary_cost)
        .then_with(|| u8::from(lhs.state != previous).cmp(&u8::from(rhs.state != previous)))
        .then_with(|| state_sort_key(lhs.state).cmp(&state_sort_key(rhs.state)))
}

pub(crate) fn insert_best_non_empty_candidate(
    candidates: &mut Vec<CellCandidate>,
    candidate: CellCandidate,
    previous: Option<DecodedCellState>,
    keep_non_empty: usize,
) {
    if keep_non_empty == 0 {
        return;
    }

    let insert_at = candidates.partition_point(|existing| {
        candidate_cmp(*existing, candidate, previous) != Ordering::Greater
    });
    if candidates.len() >= keep_non_empty && insert_at == candidates.len() {
        return;
    }

    candidates.insert(insert_at, candidate);
    if candidates.len() > keep_non_empty {
        let _ = candidates.pop();
    }
}

pub(crate) struct NonEmptyCandidateContext<'a> {
    pub(crate) empty_residual: u64,
    pub(crate) age: AgeMoment,
    pub(crate) previous: Option<DecodedCellState>,
    pub(crate) shade_profiles: &'a [ShadeProfile],
    pub(crate) temporal_stability_weight: f64,
    pub(crate) keep_non_empty: usize,
}

pub(crate) fn evaluate_non_empty_glyph_candidate(
    candidates: &mut Vec<CellCandidate>,
    glyph: GlyphProfile,
    dot: u64,
    context: &NonEmptyCandidateContext<'_>,
) {
    if glyph.sample_count == 0 || dot == 0 {
        return;
    }

    let alpha_q12 = (dot / u64::try_from(glyph.sample_count).unwrap_or(u64::MAX))
        .min(u64::from(u16::MAX)) as u16;
    if alpha_q12 < MIN_VISIBLE_SAMPLE_Q12 {
        return;
    }

    let prior = u64::from(glyph.complexity)
        .saturating_mul(u64::from(glyph.complexity))
        .saturating_mul(PRIOR_COMPLEXITY_WEIGHT);
    let shade_indices = shade_profile_indices_for_glyph(
        context.shade_profiles,
        alpha_q12,
        context.previous,
        glyph.glyph,
    );
    for shade_index in shade_indices.as_slice().iter().copied() {
        let shade = context.shade_profiles[shade_index];
        let profile = LocalCellProfile::new(glyph, shade);
        let residual = residual_cost(context.empty_residual, dot, profile);
        let state = Some(profile.state);
        let total_cost = residual.saturating_add(prior).saturating_add(temporal_cost(
            context.previous,
            state,
            context.age,
            context.temporal_stability_weight,
        ));
        insert_best_non_empty_candidate(
            candidates,
            CellCandidate {
                state,
                unary_cost: total_cost,
            },
            context.previous,
            context.keep_non_empty,
        );
    }
}

pub(crate) fn cell_candidates_for_patch_into(
    output: &mut Vec<CellCandidate>,
    patch: MicroTile,
    age: AgeMoment,
    previous: Option<DecodedCellState>,
    shade_profiles: &[ShadeProfile],
    temporal_stability_weight: f64,
    top_k: usize,
) {
    output.clear();
    let patch_basis = PatchCandidateBasis::from_patch(patch);
    let empty_residual = patch_basis.empty_residual;

    let empty_candidate = CellCandidate {
        state: None,
        unary_cost: empty_residual.saturating_add(temporal_cost(
            previous,
            None,
            age,
            temporal_stability_weight,
        )),
    };
    let keep_non_empty = top_k.saturating_sub(1);
    if patch.max_sample_q12() < MIN_VISIBLE_SAMPLE_Q12
        || keep_non_empty == 0
        || shade_profiles.is_empty()
    {
        output.push(empty_candidate);
        return;
    }
    output.reserve((1 + keep_non_empty).saturating_sub(output.len()));
    let non_empty_context = NonEmptyCandidateContext {
        empty_residual,
        age,
        previous,
        shade_profiles,
        temporal_stability_weight,
        keep_non_empty,
    };

    evaluate_non_empty_glyph_candidate(
        output,
        GlyphProfile::block(),
        patch_basis.total_mass,
        &non_empty_context,
    );

    let layout = &*GLYPH_BUCKET_LAYOUT;
    let matrix_dots: [u64; MATRIX_MASK_LIMIT] = build_subset_sums(&patch_basis.matrix_bucket_sums);
    for mask in 1_u8..=14_u8 {
        evaluate_non_empty_glyph_candidate(
            output,
            GlyphProfile::matrix(mask, layout.matrix_sample_count(mask)),
            matrix_dots[usize::from(mask)],
            &non_empty_context,
        );
    }

    let octant_dots: [u64; OCTANT_MASK_LIMIT] = build_subset_sums(&patch_basis.octant_bucket_sums);
    for mask in 1_u8..=254_u8 {
        evaluate_non_empty_glyph_candidate(
            output,
            GlyphProfile::octant(mask, layout.octant_sample_count(mask)),
            octant_dots[usize::from(mask)],
            &non_empty_context,
        );
    }

    output.insert(0, empty_candidate);
    output.sort_by(|lhs, rhs| candidate_cmp(*lhs, *rhs, previous));
}

#[cfg(test)]
pub(crate) fn cell_candidates_for_patch(
    patch: MicroTile,
    age: AgeMoment,
    previous: Option<DecodedCellState>,
    shade_profiles: &[ShadeProfile],
    temporal_stability_weight: f64,
    top_k: usize,
) -> Vec<CellCandidate> {
    let mut candidates = Vec::new();
    cell_candidates_for_patch_into(
        &mut candidates,
        patch,
        age,
        previous,
        shade_profiles,
        temporal_stability_weight,
        top_k,
    );
    candidates
}
