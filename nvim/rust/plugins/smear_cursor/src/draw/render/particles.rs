use super::super::{
    BRAILLE_CODE_MAX, BRAILLE_CODE_MIN, OCTANT_CODE_MAX, OCTANT_CODE_MIN, RenderFrame,
};
use super::DrawResources;
use crate::lua::i64_from_object;
use nvim_oxi::Result;
use nvim_oxi::api;
use nvim_oxi::{Array, Object};
use std::collections::HashMap;

use super::cell_draw::{draw_braille_character, draw_octant_character};
use super::geometry::{frac01, level_from_shade, round_lua};

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

fn screen_char_code(row: i64, col: i64) -> Option<i64> {
    let args = Array::from_iter([Object::from(row), Object::from(col)]);
    let value = api::call_function("screenchar", args).ok()?;
    i64_from_object("screenchar", value).ok()
}

fn particle_can_draw_at(row: i64, col: i64) -> bool {
    let Some(bg_char_code) = screen_char_code(row, col) else {
        return false;
    };

    let is_space = bg_char_code == 32;
    let is_braille = (BRAILLE_CODE_MIN..=BRAILLE_CODE_MAX).contains(&bg_char_code);
    let is_octant = (OCTANT_CODE_MIN..=OCTANT_CODE_MAX).contains(&bg_char_code);
    is_space || is_braille || is_octant
}

pub(super) fn draw_particles(
    resources: &mut DrawResources<'_>,
    namespace_id: u32,
    frame: &RenderFrame,
    target_row: i64,
    target_col: i64,
) -> Result<()> {
    if frame.particles.is_empty() {
        return Ok(());
    }

    let lifetime_switch_octant_braille = if frame.legacy_computing_symbols_support {
        frame.particle_max_lifetime * frame.particle_switch_octant_braille
    } else {
        f64::INFINITY
    };

    let mut cells: HashMap<(i64, i64), ParticleCellAggregate> =
        HashMap::with_capacity(frame.particles.len());
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
        if row < 1 || row > resources.max_row || col < 1 || col > resources.max_col {
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

        let level_index = usize::try_from(level)
            .unwrap_or(0)
            .min(resources.hl_groups.len().saturating_sub(1));
        if level_index == 0 {
            continue;
        }
        let hl_group = resources.hl_groups[level_index].as_str();

        if !frame.particles_over_text && !particle_can_draw_at(row, col) {
            continue;
        }

        if lifetime_average > lifetime_switch_octant_braille {
            draw_octant_character(
                resources,
                namespace_id,
                row,
                col,
                &aggregate.cell,
                hl_group,
                resources.particle_zindex,
            )?;
        } else {
            draw_braille_character(
                resources,
                namespace_id,
                row,
                col,
                &aggregate.cell,
                hl_group,
                resources.particle_zindex,
            )?;
        }
    }

    Ok(())
}
