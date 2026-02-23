use super::super::{
    BOTTOM_BLOCKS, BRAILLE_CODE_MIN, LEFT_BLOCKS, MATRIX_CHARACTERS, RIGHT_BLOCKS, RenderFrame,
    TOP_BLOCKS, VERTICAL_BARS,
};
use super::{DrawResources, DrawState, draw_character};
use crate::octant_chars::OCTANT_CHARACTERS;
use nvim_oxi::Result;

use super::geometry::{QuadGeometry, diagonal_blocks_for_slope, frac01, level_from_shade};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BlockCharacterSet {
    Bottom,
    Left,
    Top,
    Right,
    VerticalBars,
}

#[derive(Clone, Copy, Debug)]
struct PartialBlockProperties {
    character_index: i64,
    character_set: BlockCharacterSet,
    level: u32,
    inverted: bool,
}

fn block_characters(character_set: BlockCharacterSet) -> &'static [&'static str] {
    match character_set {
        BlockCharacterSet::Bottom => &BOTTOM_BLOCKS,
        BlockCharacterSet::Left => &LEFT_BLOCKS,
        BlockCharacterSet::Top => &TOP_BLOCKS,
        BlockCharacterSet::Right => &RIGHT_BLOCKS,
        BlockCharacterSet::VerticalBars => &VERTICAL_BARS,
    }
}

fn draw_partial_block(
    draw_state: &mut DrawState,
    tab_handle: i32,
    namespace_id: u32,
    row: i64,
    col: i64,
    properties: PartialBlockProperties,
    hl_groups: &[String],
    inverted_hl_groups: &[String],
    windows_zindex: u32,
    max_row: i64,
    max_col: i64,
) -> Result<()> {
    let characters = block_characters(properties.character_set);
    let Ok(character_index) = usize::try_from(properties.character_index) else {
        return Ok(());
    };
    let Some(character) = characters.get(character_index).copied() else {
        return Ok(());
    };

    let level_index = usize::try_from(properties.level)
        .unwrap_or(0)
        .min((hl_groups.len().saturating_sub(1)).min(inverted_hl_groups.len().saturating_sub(1)));
    if level_index == 0 {
        return Ok(());
    }

    let hl_group = if properties.inverted {
        inverted_hl_groups[level_index].as_str()
    } else {
        hl_groups[level_index].as_str()
    };

    draw_character(
        draw_state,
        tab_handle,
        namespace_id,
        row,
        col,
        character,
        hl_group,
        windows_zindex,
        max_row,
        max_col,
    )
}

fn get_top_block_properties(
    micro_shift: f64,
    thickness: f64,
    shade: f64,
    frame: &RenderFrame,
) -> Option<PartialBlockProperties> {
    let character_index = micro_shift.ceil() as i64;
    if character_index == 0 {
        return None;
    }

    let character_thickness = character_index as f64 / 8.0;
    let adjusted_shade = shade * thickness / character_thickness;
    let level = level_from_shade(adjusted_shade, frame.color_levels);
    if level == 0 {
        return None;
    }

    if frame.legacy_computing_symbols_support {
        Some(PartialBlockProperties {
            character_index,
            character_set: BlockCharacterSet::Top,
            level,
            inverted: false,
        })
    } else {
        Some(PartialBlockProperties {
            character_index,
            character_set: BlockCharacterSet::Bottom,
            level,
            inverted: true,
        })
    }
}

fn get_bottom_block_properties(
    micro_shift: f64,
    thickness: f64,
    shade: f64,
    frame: &RenderFrame,
) -> Option<PartialBlockProperties> {
    let character_index = micro_shift.floor() as i64;
    if character_index == 8 {
        return None;
    }

    let character_thickness = 1.0 - character_index as f64 / 8.0;
    let adjusted_shade = shade * thickness / character_thickness;
    let level = level_from_shade(adjusted_shade, frame.color_levels);
    if level == 0 {
        return None;
    }

    Some(PartialBlockProperties {
        character_index,
        character_set: BlockCharacterSet::Bottom,
        level,
        inverted: false,
    })
}

fn get_vertical_bar_properties(
    micro_shift: f64,
    thickness: f64,
    shade: f64,
    frame: &RenderFrame,
) -> Option<PartialBlockProperties> {
    let character_index = micro_shift.floor() as i64;
    if !(0..8).contains(&character_index) {
        return None;
    }

    let character_thickness = 1.0 / 8.0;
    let adjusted_shade = (shade * thickness / character_thickness).min(1.0);
    let level = level_from_shade(adjusted_shade, frame.color_levels);
    if level == 0 {
        return None;
    }

    Some(PartialBlockProperties {
        character_index,
        character_set: BlockCharacterSet::VerticalBars,
        level,
        inverted: false,
    })
}

fn get_left_block_properties(
    micro_shift: f64,
    thickness: f64,
    shade: f64,
    frame: &RenderFrame,
) -> Option<PartialBlockProperties> {
    let character_index = micro_shift.ceil() as i64;
    if character_index == 0 {
        return None;
    }

    let character_thickness = character_index as f64 / 8.0;
    let adjusted_shade = shade * thickness / character_thickness;
    let level = level_from_shade(adjusted_shade, frame.color_levels);
    if level == 0 {
        return None;
    }

    Some(PartialBlockProperties {
        character_index,
        character_set: BlockCharacterSet::Left,
        level,
        inverted: false,
    })
}

fn get_right_block_properties(
    micro_shift: f64,
    thickness: f64,
    shade: f64,
    frame: &RenderFrame,
) -> Option<PartialBlockProperties> {
    let character_index = micro_shift.floor() as i64;
    if character_index == 8 {
        return None;
    }

    let character_thickness = 1.0 - character_index as f64 / 8.0;
    let adjusted_shade = shade * thickness / character_thickness;
    let level = level_from_shade(adjusted_shade, frame.color_levels);
    if level == 0 {
        return None;
    }

    if frame.legacy_computing_symbols_support {
        Some(PartialBlockProperties {
            character_index,
            character_set: BlockCharacterSet::Right,
            level,
            inverted: false,
        })
    } else {
        Some(PartialBlockProperties {
            character_index,
            character_set: BlockCharacterSet::Left,
            level,
            inverted: true,
        })
    }
}

pub(super) fn draw_vertically_shifted_sub_block(
    resources: &mut DrawResources<'_>,
    namespace_id: u32,
    frame: &RenderFrame,
    bulge_above: bool,
    row_top: f64,
    row_bottom: f64,
    col: i64,
    shade: f64,
) -> Result<bool> {
    if row_top >= row_bottom {
        return Ok(false);
    }

    let row = row_top.floor() as i64;
    let center = frac01((row_top + row_bottom) / 2.0);
    let thickness = row_bottom - row_top;
    let gap_top = frac01(row_top);
    let gap_bottom = frac01(1.0 - row_bottom);

    let properties = if gap_top.max(gap_bottom) / 2.0 < gap_top.min(gap_bottom) {
        if bulge_above {
            let micro_shift = frac01(row_bottom) * 8.0;
            get_top_block_properties(micro_shift, thickness, shade, frame)
        } else {
            let micro_shift = frac01(row_top) * 8.0;
            get_bottom_block_properties(micro_shift, thickness, shade, frame)
        }
    } else if center < 0.5 {
        get_top_block_properties(center * 16.0, thickness, shade, frame)
    } else {
        get_bottom_block_properties(center * 16.0 - 8.0, thickness, shade, frame)
    };

    let Some(properties) = properties else {
        return Ok(false);
    };

    draw_partial_block(
        resources.draw_state,
        resources.tab_handle,
        namespace_id,
        row,
        col,
        properties,
        resources.hl_groups,
        resources.inverted_hl_groups,
        resources.windows_zindex,
        resources.max_row,
        resources.max_col,
    )?;
    Ok(true)
}

pub(super) fn draw_horizontally_shifted_sub_block(
    resources: &mut DrawResources<'_>,
    namespace_id: u32,
    frame: &RenderFrame,
    bulge_above: bool,
    row: i64,
    col_left: f64,
    col_right: f64,
    shade: f64,
) -> Result<bool> {
    if col_left >= col_right {
        return Ok(false);
    }

    let col = col_left.floor() as i64;
    let center = frac01((col_left + col_right) / 2.0);
    let thickness = col_right - col_left;
    let gap_left = frac01(col_left);
    let gap_right = frac01(1.0 - col_right);

    let properties = if (frame.legacy_computing_symbols_support
        || frame.legacy_computing_symbols_support_vertical_bars)
        && thickness <= 1.5 / 8.0
    {
        get_vertical_bar_properties(center * 8.0, thickness, shade, frame)
    } else if gap_left.max(gap_right) / 2.0 < gap_left.min(gap_right) {
        if bulge_above {
            get_left_block_properties(frac01(col_right) * 8.0, thickness, shade, frame)
        } else {
            get_right_block_properties(frac01(col_left) * 8.0, thickness, shade, frame)
        }
    } else if center < 0.5 {
        get_left_block_properties(center * 16.0, thickness, shade, frame)
    } else {
        get_right_block_properties(center * 16.0 - 8.0, thickness, shade, frame)
    };

    let Some(properties) = properties else {
        return Ok(false);
    };

    draw_partial_block(
        resources.draw_state,
        resources.tab_handle,
        namespace_id,
        row,
        col,
        properties,
        resources.hl_groups,
        resources.inverted_hl_groups,
        resources.windows_zindex,
        resources.max_row,
        resources.max_col,
    )?;
    Ok(true)
}

pub(super) fn draw_diagonal_block(
    resources: &mut DrawResources<'_>,
    namespace_id: u32,
    frame: &RenderFrame,
    geometry: &QuadGeometry,
    edge_index: usize,
    row: i64,
    col: i64,
    shade: f64,
) -> Result<bool> {
    let edge_type = geometry.edge_types[edge_index];
    let slope = geometry.slopes[edge_index];
    let Some(candidates) = diagonal_blocks_for_slope(edge_type, slope) else {
        return Ok(false);
    };

    let Some(centerline) = geometry.intersections[edge_index]
        .centerlines
        .get(&row)
        .copied()
    else {
        return Ok(false);
    };

    let mut min_offset = f64::INFINITY;
    let mut matching_character = None;
    for (shift, character) in candidates.iter().copied() {
        let offset = (centerline - col as f64 - 0.5 - shift).abs();
        if offset < min_offset {
            min_offset = offset;
            matching_character = Some(character);
        }
    }

    let Some(character) = matching_character else {
        return Ok(false);
    };
    if min_offset > frame.max_offset_diagonal {
        return Ok(false);
    }

    let adjusted_shade = if frame.vertical_bar {
        shade / 8.0
    } else {
        shade
    };
    let level = level_from_shade(adjusted_shade, frame.color_levels);
    if level == 0 {
        return Ok(false);
    }

    let level_index = usize::try_from(level)
        .unwrap_or(0)
        .min(resources.hl_groups.len().saturating_sub(1));
    if level_index == 0 {
        return Ok(false);
    }
    let hl_group = resources.hl_groups[level_index].as_str();

    draw_character(
        resources.draw_state,
        resources.tab_handle,
        namespace_id,
        row,
        col,
        character,
        hl_group,
        resources.windows_zindex,
        resources.max_row,
        resources.max_col,
    )?;

    Ok(true)
}

pub(super) fn draw_matrix_character(
    resources: &mut DrawResources<'_>,
    namespace_id: u32,
    frame: &RenderFrame,
    row: i64,
    col: i64,
    matrix: [[f64; 2]; 2],
    shade: f64,
) -> Result<bool> {
    let max_matrix_coverage = matrix
        .iter()
        .flat_map(|row_values| row_values.iter())
        .copied()
        .fold(0.0_f64, f64::max);

    let matrix_pixel_threshold = if frame.vertical_bar {
        frame.matrix_pixel_threshold_vertical_bar
    } else {
        frame.matrix_pixel_threshold
    };

    if max_matrix_coverage < matrix_pixel_threshold {
        return Ok(false);
    }

    let threshold = max_matrix_coverage * frame.matrix_pixel_min_factor;
    let bit_1 = usize::from(matrix[0][0] > threshold);
    let bit_2 = usize::from(matrix[0][1] > threshold);
    let bit_3 = usize::from(matrix[1][0] > threshold);
    let bit_4 = usize::from(matrix[1][1] > threshold);
    let index = bit_1 + bit_2 * 2 + bit_3 * 4 + bit_4 * 8;
    if index == 0 {
        return Ok(false);
    }

    let matrix_shade = matrix[0][0] + matrix[0][1] + matrix[1][0] + matrix[1][1];
    let max_matrix_shade = bit_1 + bit_2 + bit_3 + bit_4;
    if max_matrix_shade == 0 {
        return Ok(false);
    }

    let level = level_from_shade(
        shade * matrix_shade / max_matrix_shade as f64,
        frame.color_levels,
    );
    if level == 0 {
        return Ok(false);
    }

    let level_index = usize::try_from(level)
        .unwrap_or(0)
        .min(resources.hl_groups.len().saturating_sub(1));
    if level_index == 0 {
        return Ok(false);
    }
    let hl_group = resources.hl_groups[level_index].as_str();

    draw_character(
        resources.draw_state,
        resources.tab_handle,
        namespace_id,
        row,
        col,
        MATRIX_CHARACTERS[index],
        hl_group,
        resources.windows_zindex,
        resources.max_row,
        resources.max_col,
    )?;
    Ok(true)
}

pub(super) fn draw_braille_character(
    resources: &mut DrawResources<'_>,
    namespace_id: u32,
    row: i64,
    col: i64,
    cell: &[[f64; 2]; 4],
    hl_group: &str,
    zindex: u32,
) -> Result<bool> {
    let braille_index = usize::from(cell[0][0] > 0.0)
        + usize::from(cell[1][0] > 0.0) * 2
        + usize::from(cell[2][0] > 0.0) * 4
        + usize::from(cell[0][1] > 0.0) * 8
        + usize::from(cell[1][1] > 0.0) * 16
        + usize::from(cell[2][1] > 0.0) * 32
        + usize::from(cell[3][0] > 0.0) * 64
        + usize::from(cell[3][1] > 0.0) * 128;

    if braille_index == 0 {
        return Ok(false);
    }

    let Some(character) = char::from_u32((BRAILLE_CODE_MIN as usize + braille_index) as u32) else {
        return Ok(false);
    };
    let character_text = character.to_string();

    draw_character(
        resources.draw_state,
        resources.tab_handle,
        namespace_id,
        row,
        col,
        &character_text,
        hl_group,
        zindex,
        resources.max_row,
        resources.max_col,
    )?;

    Ok(true)
}

pub(super) fn draw_octant_character(
    resources: &mut DrawResources<'_>,
    namespace_id: u32,
    row: i64,
    col: i64,
    cell: &[[f64; 2]; 4],
    hl_group: &str,
    zindex: u32,
) -> Result<bool> {
    let octant_index = usize::from(cell[0][0] > 0.0)
        + usize::from(cell[0][1] > 0.0) * 2
        + usize::from(cell[1][0] > 0.0) * 4
        + usize::from(cell[1][1] > 0.0) * 8
        + usize::from(cell[2][0] > 0.0) * 16
        + usize::from(cell[2][1] > 0.0) * 32
        + usize::from(cell[3][0] > 0.0) * 64
        + usize::from(cell[3][1] > 0.0) * 128;

    if octant_index == 0 {
        return Ok(false);
    }

    let Some(character) = OCTANT_CHARACTERS.get(octant_index - 1).copied() else {
        return Ok(false);
    };

    draw_character(
        resources.draw_state,
        resources.tab_handle,
        namespace_id,
        row,
        col,
        character,
        hl_group,
        zindex,
        resources.max_row,
        resources.max_col,
    )?;

    Ok(true)
}
