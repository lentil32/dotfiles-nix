use super::Glyph;
use super::HighlightLevel;
use super::HighlightRef;
use super::PlanResources;
use crate::octant_chars::OCTANT_CHARACTERS;

pub(super) fn draw_braille_character(
    resources: &mut PlanResources<'_>,
    row: i64,
    col: i64,
    cell: &[[f64; 2]; 4],
    level: HighlightLevel,
    zindex: u32,
    requires_background_probe: bool,
) -> bool {
    let braille_index = usize::from(cell[0][0] > 0.0)
        + usize::from(cell[1][0] > 0.0) * 2
        + usize::from(cell[2][0] > 0.0) * 4
        + usize::from(cell[0][1] > 0.0) * 8
        + usize::from(cell[1][1] > 0.0) * 16
        + usize::from(cell[2][1] > 0.0) * 32
        + usize::from(cell[3][0] > 0.0) * 64
        + usize::from(cell[3][1] > 0.0) * 128;

    if braille_index == 0 {
        return false;
    }

    let Ok(character) = u8::try_from(braille_index) else {
        return false;
    };

    resources.builder.push_particle(
        row,
        col,
        zindex,
        Glyph::Braille(character),
        HighlightRef::Normal(level),
        requires_background_probe,
    )
}

pub(super) fn draw_octant_character(
    resources: &mut PlanResources<'_>,
    row: i64,
    col: i64,
    cell: &[[f64; 2]; 4],
    level: HighlightLevel,
    zindex: u32,
    requires_background_probe: bool,
) -> bool {
    let octant_index = usize::from(cell[0][0] > 0.0)
        + usize::from(cell[0][1] > 0.0) * 2
        + usize::from(cell[1][0] > 0.0) * 4
        + usize::from(cell[1][1] > 0.0) * 8
        + usize::from(cell[2][0] > 0.0) * 16
        + usize::from(cell[2][1] > 0.0) * 32
        + usize::from(cell[3][0] > 0.0) * 64
        + usize::from(cell[3][1] > 0.0) * 128;

    if octant_index == 0 {
        return false;
    }

    let Some(character) = OCTANT_CHARACTERS.get(octant_index - 1).copied() else {
        return false;
    };

    resources.builder.push_particle(
        row,
        col,
        zindex,
        Glyph::Static(character),
        HighlightRef::Normal(level),
        requires_background_probe,
    )
}
