use super::PlanResources;
use super::cell_draw::{draw_braille_character, draw_octant_character};
use super::geometry::{frac01, level_from_shade, round_lua};
use crate::types::RenderFrame;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default)]
struct ParticleCellAggregate {
    cell: [[f64; 2]; 4],
    dot_count: u8,
    lifetime_sum: f64,
}

impl ParticleCellAggregate {
    fn add_lifetime(&mut self, sub_row: usize, sub_col: usize, lifetime: f64) {
        let previous = self.cell[sub_row][sub_col];
        self.cell[sub_row][sub_col] = previous + lifetime;
        self.lifetime_sum += lifetime;

        let was_visible = previous > 0.0;
        let is_visible = self.cell[sub_row][sub_col] > 0.0;
        if was_visible == is_visible {
            return;
        }

        if is_visible {
            self.dot_count = self.dot_count.saturating_add(1);
        } else {
            self.dot_count = self.dot_count.saturating_sub(1);
        }
    }

    fn lifetime_average(self) -> Option<f64> {
        if self.dot_count == 0 {
            return None;
        }
        Some(self.lifetime_sum / f64::from(self.dot_count))
    }
}

pub(super) fn draw_particles(
    resources: &mut PlanResources<'_>,
    frame: &RenderFrame,
    target_row: i64,
    target_col: i64,
) {
    if frame.particles.is_empty() {
        return;
    }

    let lifetime_switch_octant_braille = f64::INFINITY;
    let requires_background_probe = !frame.particles_over_text;

    let mut cells: BTreeMap<(i64, i64), ParticleCellAggregate> = BTreeMap::new();
    for particle in &frame.particles {
        let row = particle.position.row.floor() as i64;
        let col = particle.position.col.floor() as i64;
        let sub_row = round_lua(4.0 * frac01(particle.position.row) + 0.5).clamp(1, 4);
        let sub_col = round_lua(2.0 * frac01(particle.position.col) + 0.5).clamp(1, 2);

        let cell = cells.entry((row, col)).or_default();
        cell.add_lifetime(
            (sub_row.saturating_sub(1)) as usize,
            (sub_col.saturating_sub(1)) as usize,
            particle.lifetime,
        );
    }

    for ((row, col), aggregate) in cells {
        if row == target_row && col == target_col {
            continue;
        }

        let Some(lifetime_average) = aggregate.lifetime_average() else {
            continue;
        };

        let shade = if lifetime_average > lifetime_switch_octant_braille {
            let denominator = frame.particle_max_lifetime - lifetime_switch_octant_braille;
            if denominator <= 0.0 {
                1.0
            } else {
                ((lifetime_average - lifetime_switch_octant_braille) / denominator).clamp(0.0, 1.0)
            }
        } else {
            let denominator = frame
                .particle_max_lifetime
                .min(lifetime_switch_octant_braille)
                .max(1.0e-9);
            (lifetime_average / denominator).clamp(0.0, 1.0)
        };

        let level = level_from_shade(shade, frame.color_levels);
        if level == 0 {
            continue;
        }

        if lifetime_average > lifetime_switch_octant_braille {
            draw_octant_character(
                resources,
                row,
                col,
                &aggregate.cell,
                level,
                resources.particle_zindex,
                requires_background_probe,
            );
        } else {
            draw_braille_character(
                resources,
                row,
                col,
                &aggregate.cell,
                level,
                resources.particle_zindex,
                requires_background_probe,
            );
        }
    }
}
