use super::PlanResources;
use super::RenderFrame;
use super::cell_draw::draw_braille_character;
use super::cell_draw::draw_octant_character;
use super::geometry::level_from_shade;

pub(super) fn draw_particles(
    resources: &mut PlanResources<'_>,
    frame: &RenderFrame,
    target_row: i64,
    target_col: i64,
) {
    if !frame.has_particles() {
        return;
    }

    let particle_max_lifetime = if frame.particle_max_lifetime.is_finite() {
        frame.particle_max_lifetime.max(0.0)
    } else {
        0.0
    };
    let switch_ratio = if frame.particle_switch_octant_braille.is_finite() {
        frame.particle_switch_octant_braille.clamp(0.0, 1.0)
    } else {
        // Surprising: invalid switch values are normalized in-band to keep rendering total.
        0.0
    };
    let lifetime_switch_octant_braille = switch_ratio * particle_max_lifetime;
    let requires_background_probe = !frame.particles_over_text;

    // Surprising-but-important invariant: output particles are already unique per screen cell,
    // so downstream probe logic should avoid re-deduplicating by `(row, col)`.
    for aggregate in frame.aggregated_particle_cells() {
        let row = aggregate.row();
        let col = aggregate.col();
        if row == target_row && col == target_col {
            continue;
        }

        let Some(lifetime_average) = aggregate.lifetime_average() else {
            continue;
        };

        let shade = if lifetime_average > lifetime_switch_octant_braille {
            let denominator = (particle_max_lifetime - lifetime_switch_octant_braille).max(1.0e-9);
            ((lifetime_average - lifetime_switch_octant_braille) / denominator).clamp(0.0, 1.0)
        } else {
            let denominator = lifetime_switch_octant_braille.max(1.0e-9);
            (lifetime_average / denominator).clamp(0.0, 1.0)
        };

        let Some(level) = level_from_shade(shade, frame.color_levels) else {
            continue;
        };

        if lifetime_average > lifetime_switch_octant_braille {
            draw_octant_character(
                resources,
                row,
                col,
                aggregate.cell(),
                level,
                resources.particle_zindex,
                requires_background_probe,
            );
        } else {
            draw_braille_character(
                resources,
                row,
                col,
                aggregate.cell(),
                level,
                resources.particle_zindex,
                requires_background_probe,
            );
        }
    }
}
