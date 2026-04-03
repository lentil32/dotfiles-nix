use super::*;

fn decoded_glyph_to_render(glyph: DecodedGlyph) -> Option<Glyph> {
    match glyph {
        DecodedGlyph::Block => Some(Glyph::BLOCK),
        DecodedGlyph::Matrix(mask) => {
            let index = usize::from(mask.min(0x0F));
            let character = MATRIX_CHARACTERS.get(index).copied().unwrap_or("");
            if character.is_empty() {
                None
            } else {
                Some(Glyph::Static(character))
            }
        }
        DecodedGlyph::Octant(mask) => {
            let index = usize::from(mask.saturating_sub(1));
            let character = OCTANT_CHARACTERS.get(index).copied();
            character.map(Glyph::Static)
        }
    }
}

pub(in super::super) fn push_decoded_cell(
    resources: &mut PlanResources<'_>,
    row: i64,
    col: i64,
    state: DecodedCellState,
) {
    let Some(glyph) = decoded_glyph_to_render(state.glyph) else {
        return;
    };
    let _ = resources.builder.push_cell(
        row,
        col,
        resources.windows_zindex,
        glyph,
        HighlightRef::Normal(state.level),
    );
}

pub(in super::super) fn sanitize_temporal_weight(frame: &RenderFrame) -> f64 {
    if frame.temporal_stability_weight.is_finite() {
        frame.temporal_stability_weight.clamp(0.0, 3.0)
    } else {
        0.12
    }
}

pub(in super::super) fn sanitize_spatial_weight_q10(frame: &RenderFrame) -> u32 {
    let weight = if frame.spatial_coherence_weight.is_finite() {
        frame.spatial_coherence_weight.clamp(0.0, 4.0)
    } else {
        1.0
    };
    (weight * 1024.0).round() as u32
}

pub(in super::super) fn sanitize_top_k(frame: &RenderFrame) -> usize {
    usize::from(frame.top_k_per_cell.clamp(2, 8))
}

pub(in super::super) fn aspect_metric_distance(
    start: Point,
    end: Point,
    block_aspect_ratio: f64,
) -> f64 {
    start.display_distance(end, block_aspect_ratio)
}

fn arc_len_q16_delta(start: Point, end: Point, block_aspect_ratio: f64) -> ArcLenQ16 {
    ArcLenQ16::new(latent_field::q16_from_non_negative(aspect_metric_distance(
        start,
        end,
        block_aspect_ratio,
    )))
}

fn speed_gain(speed_cps: f64, start_cps: f64, full_cps: f64, min_gain: f64) -> f64 {
    if !speed_cps.is_finite() {
        return min_gain.clamp(0.0, 1.0);
    }
    let denom = (full_cps - start_cps).max(1.0e-6);
    let t = ((speed_cps - start_cps) / denom).clamp(0.0, 1.0);
    let eased = smoothstep01(t);
    min_gain + (1.0 - min_gain) * eased
}

fn band_speed_gains(
    start_pose: latent_field::Pose,
    end_pose: latent_field::Pose,
    block_aspect_ratio: f64,
    dt_ms: f64,
) -> (f64, f64) {
    let safe_dt_ms = if dt_ms.is_finite() {
        dt_ms.max(1.0)
    } else {
        latent_field::simulation_step_ms(120.0)
    };
    let dt_seconds = safe_dt_ms / 1000.0;
    let speed_cps =
        aspect_metric_distance(start_pose.center, end_pose.center, block_aspect_ratio) / dt_seconds;
    let sheath = speed_gain(
        speed_cps,
        SPEED_SHEATH_START_CPS,
        SPEED_SHEATH_FULL_CPS,
        SPEED_SHEATH_MIN_GAIN,
    );
    let core = speed_gain(
        speed_cps,
        SPEED_CORE_START_CPS,
        SPEED_CORE_FULL_CPS,
        SPEED_CORE_MIN_GAIN,
    );
    (sheath, core)
}

fn handle_stroke_transition(state: &mut PlannerState, frame: &RenderFrame) {
    let stroke_changed = state
        .last_trail_stroke_id
        .is_some_and(|stroke_id| stroke_id != frame.trail_stroke_id);
    if stroke_changed {
        state.arc_len_q16 = ArcLenQ16::ZERO;
        state.last_pose = None;
        state.center_history.clear();
    }
    state.last_trail_stroke_id = Some(frame.trail_stroke_id);
}

#[cfg(test)]
fn record_history_slice(state: &mut PlannerState, slice: &DepositedSlice) {
    state.history.push_back(slice.clone());
}

#[cfg(test)]
fn prune_debug_history(state: &mut PlannerState) {
    let mut retained = VecDeque::with_capacity(state.history.len());
    while let Some(slice) = state.history.pop_front() {
        let support_steps = u64::try_from(slice.support_steps).unwrap_or(u64::MAX);
        let age_steps = state
            .step_index
            .value()
            .saturating_sub(slice.step_index.value());
        if age_steps < support_steps {
            retained.push_back(slice);
        }
    }
    state.history = retained;
}

pub(in super::super) fn stage_deposited_samples(state: &mut PlannerState, frame: &RenderFrame) {
    handle_stroke_transition(state, frame);

    let mut latest_pose = state.last_pose;
    let mut sweep_scratch = latent_field::SweepMaterializeScratch::default();

    for (sample, current_pose) in frame.step_samples.iter().zip(frame_sample_poses(frame)) {
        let start_pose = latest_pose.unwrap_or(current_pose);
        let arc_len_delta_q16 = arc_len_q16_delta(
            start_pose.center,
            current_pose.center,
            frame.block_aspect_ratio,
        );

        state.step_index = state.step_index.next();
        state.latent_cache.advance_to(state.step_index);
        state.arc_len_q16 = state.arc_len_q16.saturating_add(arc_len_delta_q16);
        let (sheath_gain, core_gain) = band_speed_gains(
            start_pose,
            current_pose,
            frame.block_aspect_ratio,
            sample.dt_ms,
        );
        let tail_profiles = latent_field::comet_tail_profiles(frame.tail_duration_ms);
        let max_width_scale = tail_profiles
            .iter()
            .fold(0.0_f64, |max, profile| max.max(profile.width_scale));
        let sweep_geometry = latent_field::prepare_swept_occupancy_geometry(
            start_pose,
            current_pose,
            frame.block_aspect_ratio,
            frame.trail_thickness * max_width_scale,
            frame.trail_thickness_x * max_width_scale,
        );
        for profile in tail_profiles {
            let band_gain = match profile.band {
                TailBand::Sheath => sheath_gain,
                TailBand::Core => core_gain,
                TailBand::Filament => 1.0,
            };
            let band_intensity = profile.intensity * band_gain;
            let intensity_q16 = latent_field::intensity_q16(band_intensity);
            if intensity_q16 == 0 {
                continue;
            }

            let microtiles = latent_field::materialize_swept_occupancy_with_scratch(
                &sweep_geometry,
                frame.trail_thickness * profile.width_scale,
                frame.trail_thickness_x * profile.width_scale,
                &mut sweep_scratch,
            );
            if microtiles.is_empty() {
                continue;
            }
            let Some(bbox) = latent_field::CellRect::from_microtiles(&microtiles) else {
                continue;
            };

            let slice = DepositedSlice {
                stroke_id: frame.trail_stroke_id,
                step_index: state.step_index,
                dt_ms_q16: latent_field::q16_from_non_negative(sample.dt_ms),
                arc_len_q16: state.arc_len_q16,
                bbox,
                band: profile.band,
                support_steps: profile.support_steps(frame.simulation_hz),
                intensity_q16,
                microtiles,
            };
            state.latent_cache.insert_slice(&slice);
            #[cfg(test)]
            {
                record_history_slice(state, &slice);
            }
        }
        state.center_history.push_back(CenterPathSample {
            step_index: state.step_index,
            pos: current_pose.center,
        });
        latest_pose = Some(current_pose);
    }

    // presentation ticks can advance latent support windows even when the motion
    // reducer has stopped depositing new samples. That keeps settle-time disappearance inside
    // the normal render pipeline instead of falling back to shell-side cleanup truth.
    for _ in 0..frame.planner_idle_steps {
        state.step_index = state.step_index.next();
        state.latent_cache.advance_to(state.step_index);
    }

    #[cfg(test)]
    {
        prune_debug_history(state);
    }

    let support_steps =
        latent_field::max_comet_support_steps(frame.tail_duration_ms, frame.simulation_hz);
    let support_steps_u64 = u64::try_from(support_steps).unwrap_or(u64::MAX);
    while state.center_history.front().is_some_and(|sample| {
        state
            .step_index
            .value()
            .saturating_sub(sample.step_index.value())
            >= support_steps_u64
    }) {
        let _ = state.center_history.pop_front();
    }
    if let Some(latest_pose) = latest_pose {
        state.last_pose = Some(latest_pose);
    }
}
