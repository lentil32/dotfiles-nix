use super::*;
use std::cmp::Ordering;
use std::collections::BTreeMap;

type CompiledEntry<'a> = ((i64, i64), &'a CompiledCell);
type PreviousEntry<'a> = ((i64, i64), &'a DecodedCellState);

struct PatchCandidateInput<'a> {
    patch: MicroTile,
    age: AgeMoment,
    previous: Option<DecodedCellState>,
    shade_profiles: &'a [ShadeProfile],
    temporal_stability_weight: f64,
    top_k: usize,
}

fn recycle_candidate_lists(
    cell_candidates: &mut BTreeMap<(i64, i64), Vec<CellCandidate>>,
    reusable_candidate_lists: &mut Vec<Vec<CellCandidate>>,
) {
    let recycled_start = reusable_candidate_lists.len();
    for (_, mut candidates) in std::mem::take(cell_candidates) {
        candidates.clear();
        reusable_candidate_lists.push(candidates);
    }
    // Keep the freshly recycled lists in key order so the next population pass can
    // reuse like-for-like capacities instead of handing the most complex cell a
    // smaller vector from the opposite end of the map.
    reusable_candidate_lists[recycled_start..].reverse();
}

fn insert_cell_candidates_for_patch(
    cell_candidates: &mut BTreeMap<(i64, i64), Vec<CellCandidate>>,
    reusable_candidate_lists: &mut Vec<Vec<CellCandidate>>,
    coord: (i64, i64),
    input: PatchCandidateInput<'_>,
) {
    let mut per_cell = reusable_candidate_lists.pop().unwrap_or_default();
    cell_candidates_for_patch_into(
        &mut per_cell,
        input.patch,
        input.age,
        input.previous,
        input.shade_profiles,
        input.temporal_stability_weight,
        input.top_k,
    );
    cell_candidates.insert(coord, per_cell);
}

fn populate_cell_candidates_from_iters<'a, CompiledIter, PreviousIter>(
    cell_candidates: &mut BTreeMap<(i64, i64), Vec<CellCandidate>>,
    reusable_candidate_lists: &mut Vec<Vec<CellCandidate>>,
    mut compiled_iter: std::iter::Peekable<CompiledIter>,
    mut previous_iter: std::iter::Peekable<PreviousIter>,
    shade_profiles: &[ShadeProfile],
    temporal_stability_weight: f64,
    top_k: usize,
) where
    CompiledIter: Iterator<Item = CompiledEntry<'a>>,
    PreviousIter: Iterator<Item = PreviousEntry<'a>>,
{
    loop {
        match (compiled_iter.peek().copied(), previous_iter.peek().copied()) {
            (Some((compiled_coord, compiled_cell)), Some((previous_coord, previous_state))) => {
                match compiled_coord.cmp(&previous_coord) {
                    Ordering::Less => {
                        insert_cell_candidates_for_patch(
                            cell_candidates,
                            reusable_candidate_lists,
                            compiled_coord,
                            PatchCandidateInput {
                                patch: compiled_cell.tile,
                                age: compiled_cell.age,
                                previous: None,
                                shade_profiles,
                                temporal_stability_weight,
                                top_k,
                            },
                        );
                        compiled_iter.next();
                    }
                    Ordering::Equal => {
                        insert_cell_candidates_for_patch(
                            cell_candidates,
                            reusable_candidate_lists,
                            compiled_coord,
                            PatchCandidateInput {
                                patch: compiled_cell.tile,
                                age: compiled_cell.age,
                                previous: Some(*previous_state),
                                shade_profiles,
                                temporal_stability_weight,
                                top_k,
                            },
                        );
                        compiled_iter.next();
                        previous_iter.next();
                    }
                    Ordering::Greater => {
                        insert_cell_candidates_for_patch(
                            cell_candidates,
                            reusable_candidate_lists,
                            previous_coord,
                            PatchCandidateInput {
                                patch: MicroTile::default(),
                                age: AgeMoment::default(),
                                previous: Some(*previous_state),
                                shade_profiles,
                                temporal_stability_weight,
                                top_k,
                            },
                        );
                        previous_iter.next();
                    }
                }
            }
            (Some((compiled_coord, compiled_cell)), None) => {
                insert_cell_candidates_for_patch(
                    cell_candidates,
                    reusable_candidate_lists,
                    compiled_coord,
                    PatchCandidateInput {
                        patch: compiled_cell.tile,
                        age: compiled_cell.age,
                        previous: None,
                        shade_profiles,
                        temporal_stability_weight,
                        top_k,
                    },
                );
                compiled_iter.next();
            }
            (None, Some((previous_coord, previous_state))) => {
                insert_cell_candidates_for_patch(
                    cell_candidates,
                    reusable_candidate_lists,
                    previous_coord,
                    PatchCandidateInput {
                        patch: MicroTile::default(),
                        age: AgeMoment::default(),
                        previous: Some(*previous_state),
                        shade_profiles,
                        temporal_stability_weight,
                        top_k,
                    },
                );
                previous_iter.next();
            }
            (None, None) => break,
        }
    }
}

fn copy_compiled_entry<'a>(
    (coord, compiled): (&'a (i64, i64), &'a CompiledCell),
) -> ((i64, i64), &'a CompiledCell) {
    (*coord, compiled)
}

fn copy_previous_entry<'a>(
    (coord, previous): (&'a (i64, i64), &'a DecodedCellState),
) -> ((i64, i64), &'a DecodedCellState) {
    (*coord, previous)
}

fn compiled_entries_in_bounds<'a>(
    compiled: &'a BTreeMap<(i64, i64), CompiledCell>,
    bounds: SliceSearchBounds,
) -> impl Iterator<Item = CompiledEntry<'a>> + 'a {
    (bounds.min_row..=bounds.max_row).flat_map(move |row| {
        compiled
            .range((row, bounds.min_col)..=(row, bounds.max_col))
            .map(copy_compiled_entry)
    })
}

fn previous_entries_in_bounds<'a>(
    previous_cells: &'a BTreeMap<(i64, i64), DecodedCellState>,
    bounds: SliceSearchBounds,
) -> impl Iterator<Item = PreviousEntry<'a>> + 'a {
    (bounds.min_row..=bounds.max_row).flat_map(move |row| {
        previous_cells
            .range((row, bounds.min_col)..=(row, bounds.max_col))
            .map(copy_previous_entry)
    })
}

pub(super) fn populate_cell_candidates_with_scratch(
    compiled: &CompiledField,
    previous_cells: &BTreeMap<(i64, i64), DecodedCellState>,
    color_levels: u32,
    temporal_stability_weight: f64,
    top_k: usize,
    scratch: &mut PlannerDecodeScratch,
) {
    let PlannerDecodeScratch {
        shade_profiles,
        cell_candidates,
        reusable_candidate_lists,
        ..
    } = scratch;
    build_shade_profiles_into(color_levels, shade_profiles);
    recycle_candidate_lists(cell_candidates, reusable_candidate_lists);

    match compiled {
        CompiledField::Reference(compiled) => populate_cell_candidates_from_iters(
            cell_candidates,
            reusable_candidate_lists,
            compiled.iter().map(copy_compiled_entry).peekable(),
            previous_cells.iter().map(copy_previous_entry).peekable(),
            shade_profiles,
            temporal_stability_weight,
            top_k,
        ),
        CompiledField::Rows(compiled) => populate_cell_candidates_from_iters(
            cell_candidates,
            reusable_candidate_lists,
            compiled.iter().peekable(),
            previous_cells.iter().map(copy_previous_entry).peekable(),
            shade_profiles,
            temporal_stability_weight,
            top_k,
        ),
    }

    crate::events::record_planner_candidate_cells_built_count(cell_candidates.len());
}

pub(super) fn populate_cell_candidates_in_bounds_with_scratch(
    compiled: &CompiledField,
    previous_cells: &BTreeMap<(i64, i64), DecodedCellState>,
    color_levels: u32,
    temporal_stability_weight: f64,
    top_k: usize,
    bounds: SliceSearchBounds,
    scratch: &mut PlannerDecodeScratch,
) {
    let PlannerDecodeScratch {
        shade_profiles,
        cell_candidates,
        reusable_candidate_lists,
        ..
    } = scratch;
    build_shade_profiles_into(color_levels, shade_profiles);
    recycle_candidate_lists(cell_candidates, reusable_candidate_lists);

    match compiled {
        CompiledField::Reference(compiled) => populate_cell_candidates_from_iters(
            cell_candidates,
            reusable_candidate_lists,
            compiled_entries_in_bounds(compiled, bounds).peekable(),
            previous_entries_in_bounds(previous_cells, bounds).peekable(),
            shade_profiles,
            temporal_stability_weight,
            top_k,
        ),
        CompiledField::Rows(compiled) => populate_cell_candidates_from_iters(
            cell_candidates,
            reusable_candidate_lists,
            compiled.iter_in_bounds(bounds).peekable(),
            previous_entries_in_bounds(previous_cells, bounds).peekable(),
            shade_profiles,
            temporal_stability_weight,
            top_k,
        ),
    }

    crate::events::record_planner_candidate_cells_built_count(cell_candidates.len());
}

#[cfg(test)]
pub(super) fn build_cell_candidates(
    compiled: &BTreeMap<(i64, i64), CompiledCell>,
    previous_cells: &BTreeMap<(i64, i64), DecodedCellState>,
    color_levels: u32,
    temporal_stability_weight: f64,
    top_k: usize,
) -> BTreeMap<(i64, i64), Vec<CellCandidate>> {
    let mut scratch = PlannerDecodeScratch::default();
    let compiled = CompiledField::Reference(compiled.clone());
    populate_cell_candidates_with_scratch(
        &compiled,
        previous_cells,
        color_levels,
        temporal_stability_weight,
        top_k,
        &mut scratch,
    );
    scratch.cell_candidates
}

pub(super) fn decode_locally(
    cell_candidates: &BTreeMap<(i64, i64), Vec<CellCandidate>>,
) -> BTreeMap<(i64, i64), DecodedCellState> {
    let mut next_cells = BTreeMap::<(i64, i64), DecodedCellState>::new();
    for (coord, candidates) in cell_candidates {
        if let Some(state) = candidates.first().and_then(|candidate| candidate.state) {
            next_cells.insert(*coord, state);
        }
    }
    next_cells
}

pub(super) fn non_empty_candidates(candidates: &[CellCandidate]) -> Vec<CellCandidate> {
    candidates
        .iter()
        .copied()
        .filter(|candidate| candidate.state.is_some())
        .collect::<Vec<_>>()
}

#[cfg(test)]
mod candidate_generation_complexity {
    use super::*;

    fn varied_patch() -> MicroTile {
        let mut patch = MicroTile::default();
        for sample_row in 0..MICRO_H {
            for sample_col in 0..MICRO_W {
                let index = sample_row * MICRO_W + sample_col;
                let base = ((sample_row * 521 + sample_col * 193 + index * 17) % 4096) as u16;
                patch.samples_q12[index] = if (sample_row + sample_col) % 5 == 0 {
                    base / 16
                } else {
                    base
                };
            }
        }
        patch
    }

    fn reference_glyph_dot(patch: MicroTile, glyph: GlyphProfile) -> u64 {
        match glyph.glyph {
            DecodedGlyph::Block => patch
                .samples_q12
                .iter()
                .copied()
                .map(u64::from)
                .sum::<u64>(),
            DecodedGlyph::Matrix(mask) => {
                let mut dot = 0_u64;
                for sample_row in 0..MICRO_H {
                    for sample_col in 0..MICRO_W {
                        let row_bucket = (sample_row * 2) / MICRO_H;
                        let col_bucket = (sample_col * 2) / MICRO_W;
                        let bit = MATRIX_BIT_WEIGHTS[row_bucket][col_bucket];
                        if mask & bit != 0 {
                            let index = sample_row * MICRO_W + sample_col;
                            dot = dot.saturating_add(u64::from(patch.samples_q12[index]));
                        }
                    }
                }
                dot
            }
            DecodedGlyph::Octant(mask) => {
                let mut dot = 0_u64;
                for sample_row in 0..MICRO_H {
                    for sample_col in 0..MICRO_W {
                        let row_bucket = (sample_row * 4) / MICRO_H;
                        let col_bucket = (sample_col * 2) / MICRO_W;
                        let bit = OCTANT_BIT_WEIGHTS[row_bucket][col_bucket];
                        if mask & bit != 0 {
                            let index = sample_row * MICRO_W + sample_col;
                            dot = dot.saturating_add(u64::from(patch.samples_q12[index]));
                        }
                    }
                }
                dot
            }
        }
    }

    fn reference_cell_candidates_for_patch(
        patch: MicroTile,
        age: AgeMoment,
        previous: Option<DecodedCellState>,
        shade_profiles: &[ShadeProfile],
        temporal_stability_weight: f64,
        top_k: usize,
    ) -> Vec<CellCandidate> {
        let empty_residual = patch
            .samples_q12
            .iter()
            .copied()
            .map(|sample| {
                let value = u64::from(sample);
                value.saturating_mul(value)
            })
            .sum::<u64>();

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
            return vec![empty_candidate];
        }

        let layout = &*GLYPH_BUCKET_LAYOUT;
        let mut non_empty = Vec::<CellCandidate>::with_capacity(keep_non_empty);
        let non_empty_context = NonEmptyCandidateContext {
            empty_residual,
            age,
            previous,
            shade_profiles,
            temporal_stability_weight,
            keep_non_empty,
        };
        let mut glyphs = Vec::with_capacity(1 + 14 + 254);
        glyphs.push(GlyphProfile::block());
        for mask in 1_u8..=14_u8 {
            glyphs.push(GlyphProfile::matrix(mask, layout.matrix_sample_count(mask)));
        }
        for mask in 1_u8..=254_u8 {
            glyphs.push(GlyphProfile::octant(mask, layout.octant_sample_count(mask)));
        }

        for glyph in glyphs {
            let dot = reference_glyph_dot(patch, glyph);
            evaluate_non_empty_glyph_candidate(&mut non_empty, glyph, dot, &non_empty_context);
        }

        let mut kept = Vec::with_capacity(1 + keep_non_empty);
        kept.push(empty_candidate);
        kept.extend(non_empty);
        kept.sort_by(|lhs, rhs| candidate_cmp(*lhs, *rhs, previous));
        kept
    }

    #[test]
    fn patch_candidate_basis_matches_reference_mask_dots_for_all_families() {
        let patch = varied_patch();
        let patch_basis = PatchCandidateBasis::from_patch(patch);
        let matrix_dots: [u64; MATRIX_MASK_LIMIT] =
            build_subset_sums(&patch_basis.matrix_bucket_sums);
        let octant_dots: [u64; OCTANT_MASK_LIMIT] =
            build_subset_sums(&patch_basis.octant_bucket_sums);
        let layout = &*GLYPH_BUCKET_LAYOUT;

        assert_eq!(
            patch_basis.total_mass,
            reference_glyph_dot(patch, GlyphProfile::block())
        );

        for mask in 1_u8..=14_u8 {
            let glyph = GlyphProfile::matrix(mask, layout.matrix_sample_count(mask));
            assert_eq!(
                matrix_dots[usize::from(mask)],
                reference_glyph_dot(patch, glyph)
            );
        }

        for mask in 1_u8..=254_u8 {
            let glyph = GlyphProfile::octant(mask, layout.octant_sample_count(mask));
            assert_eq!(
                octant_dots[usize::from(mask)],
                reference_glyph_dot(patch, glyph)
            );
        }
    }

    #[test]
    fn cell_candidates_for_patch_matches_reference_profile_scan_with_previous_state() {
        let patch = varied_patch();
        let age = AgeMoment {
            total_mass_q12: 4095,
            recent_mass_q12: 1024,
        };
        let previous = Some(DecodedCellState {
            glyph: DecodedGlyph::Octant(18),
            level: HighlightLevel::from_raw_clamped(7),
        });
        let shade_profiles = build_shade_profiles(16);

        let current = cell_candidates_for_patch(patch, age, previous, &shade_profiles, 0.35, 5);
        let reference =
            reference_cell_candidates_for_patch(patch, age, previous, &shade_profiles, 0.35, 5);

        assert_eq!(current, reference);
    }
}
