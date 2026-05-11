use super::CellOp;
use super::Glyph;
use super::HighlightRef;
use super::ParticleOp;
use super::PlanResources;
use super::RenderFrame;
use super::geometry::level_from_shade;
use crate::draw::PARTICLE_ZINDEX_OFFSET;
use crate::octant_chars::OCTANT_CHARACTERS;

fn octant_glyph(cell: &[[f64; 2]; 4]) -> Option<Glyph> {
    let octant_index = usize::from(cell[0][0] > 0.0)
        + usize::from(cell[0][1] > 0.0) * 2
        + usize::from(cell[1][0] > 0.0) * 4
        + usize::from(cell[1][1] > 0.0) * 8
        + usize::from(cell[2][0] > 0.0) * 16
        + usize::from(cell[2][1] > 0.0) * 32
        + usize::from(cell[3][0] > 0.0) * 64
        + usize::from(cell[3][1] > 0.0) * 128;

    if octant_index == 0 {
        return None;
    }

    OCTANT_CHARACTERS
        .get(octant_index.saturating_sub(1))
        .copied()
        .map(Glyph::Static)
}

fn braille_glyph(cell: &[[f64; 2]; 4]) -> Option<Glyph> {
    let braille_index = usize::from(cell[0][0] > 0.0)
        + usize::from(cell[1][0] > 0.0) * 2
        + usize::from(cell[2][0] > 0.0) * 4
        + usize::from(cell[0][1] > 0.0) * 8
        + usize::from(cell[1][1] > 0.0) * 16
        + usize::from(cell[2][1] > 0.0) * 32
        + usize::from(cell[3][0] > 0.0) * 64
        + usize::from(cell[3][1] > 0.0) * 128;

    if braille_index == 0 {
        return None;
    }

    u8::try_from(braille_index).ok().map(Glyph::Braille)
}

pub(crate) fn for_each_particle_overlay_op(
    frame: &RenderFrame,
    target_row: i64,
    target_col: i64,
    mut emit: impl FnMut(ParticleOp),
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

        let cell = aggregate.cell();
        let glyph = if lifetime_average > lifetime_switch_octant_braille {
            octant_glyph(cell)
        } else {
            braille_glyph(cell)
        };
        let Some(glyph) = glyph else {
            continue;
        };

        emit(ParticleOp {
            cell: CellOp {
                row,
                col,
                zindex: frame.windows_zindex.saturating_sub(PARTICLE_ZINDEX_OFFSET),
                glyph,
                highlight: HighlightRef::Normal(level),
            },
            requires_background_probe,
        });
    }
}

pub(super) fn draw_particles(
    resources: &mut PlanResources<'_>,
    frame: &RenderFrame,
    target_row: i64,
    target_col: i64,
) {
    for_each_particle_overlay_op(frame, target_row, target_col, |op| {
        resources.builder.push_particle(
            op.cell.row,
            op.cell.col,
            op.cell.zindex,
            op.cell.glyph,
            op.cell.highlight,
            op.requires_background_probe,
        );
    });
}
