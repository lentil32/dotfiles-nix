use super::super::BRAILLE_CODE_MIN;
use super::{Glyph, HighlightLevel, HighlightRef, PlanResources};
use crate::octant_chars::OCTANT_CHARACTERS;
use std::sync::LazyLock;

static BRAILLE_GLYPHS: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    (1_u32..=255_u32)
        .filter_map(|index| {
            char::from_u32((BRAILLE_CODE_MIN as u32).saturating_add(index))
                .map(|character| Box::leak(character.to_string().into_boxed_str()) as &'static str)
        })
        .collect()
});

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

    let Some(character) = BRAILLE_GLYPHS.get(braille_index.saturating_sub(1)).copied() else {
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
