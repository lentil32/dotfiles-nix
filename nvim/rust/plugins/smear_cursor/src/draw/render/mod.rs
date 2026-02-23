use super::{
    DrawState, EXTMARK_ID, PARTICLE_ZINDEX_OFFSET, RenderFrame, clear_cached_windows,
    draw_state_lock, ensure_highlight_palette, get_or_create_window, hide_available_windows,
    highlight_group_names,
};
use nvim_oxi::Result;
use nvim_oxi::api;
use nvim_oxi::api::opts::{OptionOpts, SetExtmarkOpts};
use nvim_oxi::api::types::ExtmarkVirtTextPosition;

mod cell_draw;
mod geometry;
mod particles;
#[cfg(test)]
mod tests;

use cell_draw::{
    draw_diagonal_block, draw_horizontally_shifted_sub_block, draw_matrix_character,
    draw_vertically_shifted_sub_block,
};
use geometry::{
    CellIntersections, EdgeType, ensure_clockwise, get_edge_cell_intersection,
    precompute_quad_geometry, update_matrix_with_edge,
};
use particles::draw_particles;

struct DrawResources<'a> {
    draw_state: &'a mut DrawState,
    tab_handle: i32,
    hl_groups: &'a [String],
    inverted_hl_groups: &'a [String],
    max_row: i64,
    max_col: i64,
    windows_zindex: u32,
    particle_zindex: u32,
}

fn compute_gradient_shade(row_center: f64, col_center: f64, frame: &RenderFrame) -> f64 {
    let Some(gradient) = frame.gradient else {
        return 1.0;
    };

    let dy = row_center - gradient.origin.row;
    let dx = col_center - gradient.origin.col;
    let projection =
        (dy * gradient.direction_scaled.row + dx * gradient.direction_scaled.col).clamp(0.0, 1.0);
    (1.0 - projection).powf(frame.gradient_exponent)
}

fn editor_bounds() -> Result<(i64, i64)> {
    let opts = OptionOpts::builder().build();
    let lines: i64 = api::get_option_value("lines", &opts)?;
    let cmdheight: i64 = api::get_option_value("cmdheight", &opts)?;
    let columns: i64 = api::get_option_value("columns", &opts)?;
    let max_row = (lines - cmdheight).max(1);
    let max_col = columns.max(1);
    Ok((max_row, max_col))
}

fn draw_character(
    draw_state: &mut DrawState,
    tab_handle: i32,
    namespace_id: u32,
    row: i64,
    col: i64,
    character: &str,
    hl_group: &str,
    zindex: u32,
    max_row: i64,
    max_col: i64,
) -> Result<()> {
    if row < 1 || row > max_row || col < 1 || col > max_col {
        return Ok(());
    }

    let (window_id, mut buffer) =
        get_or_create_window(draw_state, namespace_id, tab_handle, row, col, zindex)?;
    let payload_matches = draw_state.tabs.get(&tab_handle).is_some_and(|tab_windows| {
        tab_windows.cached_payload_matches(window_id, character, hl_group)
    });
    if payload_matches {
        return Ok(());
    }

    let extmark_opts = SetExtmarkOpts::builder()
        .id(EXTMARK_ID)
        .virt_text([(character, hl_group)])
        .virt_text_pos(ExtmarkVirtTextPosition::Overlay)
        .virt_text_win_col(0)
        .build();

    buffer.set_extmark(namespace_id, 0, 0, &extmark_opts)?;
    if let Some(tab_windows) = draw_state.tabs.get_mut(&tab_handle) {
        tab_windows.cache_payload(window_id, character, hl_group);
    }
    Ok(())
}

pub(crate) fn draw_target_hack_block(namespace_id: u32, frame: &RenderFrame) -> Result<()> {
    if namespace_id == 0 || !frame.hide_target_hack || frame.vertical_bar {
        return Ok(());
    }

    ensure_highlight_palette(frame)?;
    let (editor_max_row, editor_max_col) = editor_bounds()?;
    let tab_handle = api::get_current_tabpage().handle();
    let mut draw_state = draw_state_lock();
    let level = frame.color_levels.max(1);
    let group_names = highlight_group_names(level);
    let level_index = usize::try_from(level)
        .unwrap_or(0)
        .min(group_names.normal.len().saturating_sub(1));
    let hl_group = group_names
        .normal
        .get(level_index)
        .map(String::as_str)
        .unwrap_or("SmearCursor1");
    draw_character(
        &mut draw_state,
        tab_handle,
        namespace_id,
        frame.target.row.round() as i64,
        frame.target.col.round() as i64,
        "█",
        hl_group,
        frame.windows_zindex,
        editor_max_row,
        editor_max_col,
    )
}

pub(crate) fn draw_current(namespace_id: u32, frame: &RenderFrame) -> Result<()> {
    if namespace_id == 0 {
        return Ok(());
    }

    ensure_highlight_palette(frame)?;

    let corners = ensure_clockwise(&frame.corners);
    let geometry = precompute_quad_geometry(&corners, frame);
    let (editor_max_row, editor_max_col) = editor_bounds()?;
    let tab_handle = api::get_current_tabpage().handle();
    let mut draw_state = draw_state_lock();
    clear_cached_windows(&mut draw_state, namespace_id, frame.max_kept_windows);
    let draw_result = (|| -> Result<()> {
        if geometry.top > geometry.bottom || geometry.left > geometry.right {
            return Ok(());
        }

        draw_state.bulge_above = !draw_state.bulge_above;
        let bulge_above = draw_state.bulge_above;

        let target_row = frame.target.row.round() as i64;
        let target_col = frame.target.col.round() as i64;
        let color_levels = frame.color_levels.max(1);
        let group_names = highlight_group_names(color_levels);
        let min_shade_no_diagonal = if frame.vertical_bar {
            frame.min_shade_no_diagonal_vertical_bar
        } else {
            frame.min_shade_no_diagonal
        };

        {
            let mut resources = DrawResources {
                draw_state: &mut draw_state,
                tab_handle,
                hl_groups: &group_names.normal,
                inverted_hl_groups: &group_names.inverted,
                max_row: editor_max_row,
                max_col: editor_max_col,
                windows_zindex: frame.windows_zindex,
                particle_zindex: frame.windows_zindex.saturating_sub(PARTICLE_ZINDEX_OFFSET),
            };

            draw_particles(&mut resources, namespace_id, frame, target_row, target_col)?;

            for row in geometry.top..=geometry.bottom {
                for col in geometry.left..=geometry.right {
                    if frame.never_draw_over_target
                        && !frame.vertical_bar
                        && row == target_row
                        && col == target_col
                    {
                        continue;
                    }

                    let mut intersections = CellIntersections::default();
                    let mut single_diagonal = true;
                    let mut diagonal_edge_index = None;
                    let mut skip_cell = false;

                    for edge_index in 0..4 {
                        let intersection =
                            get_edge_cell_intersection(edge_index, row, col, &geometry, false);
                        match geometry.edge_types[edge_index] {
                            EdgeType::LeftDiagonal | EdgeType::RightDiagonal => {
                                let intersection_low = get_edge_cell_intersection(
                                    edge_index, row, col, &geometry, true,
                                );
                                if intersection_low >= 1.0 {
                                    skip_cell = true;
                                    break;
                                }

                                if intersection > min_shade_no_diagonal
                                    && intersections.diagonal.is_some()
                                {
                                    single_diagonal = false;
                                }

                                if intersection > 0.0
                                    && intersections
                                        .diagonal
                                        .is_none_or(|current| intersection > current)
                                {
                                    intersections.diagonal = Some(intersection);
                                    diagonal_edge_index = Some(edge_index);
                                }
                            }
                            edge_type => {
                                if intersection >= 1.0 {
                                    skip_cell = true;
                                    break;
                                }
                                if intersection > min_shade_no_diagonal {
                                    single_diagonal = false;
                                }

                                if intersection > 0.0 {
                                    let current = match edge_type {
                                        EdgeType::Top => &mut intersections.top,
                                        EdgeType::Bottom => &mut intersections.bottom,
                                        EdgeType::Left => &mut intersections.left,
                                        EdgeType::Right => &mut intersections.right,
                                        EdgeType::None => continue,
                                        EdgeType::LeftDiagonal | EdgeType::RightDiagonal => {
                                            continue;
                                        }
                                    };
                                    if current.is_none_or(|existing| intersection > existing) {
                                        *current = Some(intersection);
                                    }
                                }
                            }
                        }
                    }

                    if skip_cell {
                        continue;
                    }

                    if intersections
                        .diagonal
                        .is_none_or(|diagonal| diagonal < 1.0 - frame.max_shade_no_matrix)
                    {
                        let top_intersection = intersections.top.unwrap_or(0.0).max(0.0);
                        let bottom_intersection = intersections.bottom.unwrap_or(0.0).max(0.0);
                        let left_intersection = intersections.left.unwrap_or(0.0).max(0.0);
                        let right_intersection = intersections.right.unwrap_or(0.0).max(0.0);

                        let mut is_vertically_shifted =
                            intersections.top.is_some() || intersections.bottom.is_some();
                        let vertical_shade = 1.0 - top_intersection - bottom_intersection;
                        let mut is_horizontally_shifted =
                            intersections.left.is_some() || intersections.right.is_some();
                        let horizontal_shade = 1.0 - left_intersection - right_intersection;

                        if is_vertically_shifted && is_horizontally_shifted {
                            if vertical_shade < frame.max_shade_no_matrix
                                && horizontal_shade < frame.max_shade_no_matrix
                            {
                                is_vertically_shifted = false;
                                is_horizontally_shifted = false;
                            } else if 2.0 * (1.0 - vertical_shade) > (1.0 - horizontal_shade) {
                                is_horizontally_shifted = false;
                            } else {
                                is_vertically_shifted = false;
                            }
                        }

                        if is_vertically_shifted {
                            let shade = horizontal_shade
                                * compute_gradient_shade(row as f64 + 0.5, col as f64 + 0.5, frame);
                            draw_vertically_shifted_sub_block(
                                &mut resources,
                                namespace_id,
                                frame,
                                bulge_above,
                                row as f64 + top_intersection,
                                row as f64 + 1.0 - bottom_intersection,
                                col,
                                shade,
                            )?;
                            continue;
                        }

                        if is_horizontally_shifted {
                            if 1.0 - right_intersection <= 1.0 / 8.0
                                && row == target_row
                                && (col == target_col || col == target_col + 1)
                            {
                                continue;
                            }

                            let shade = vertical_shade
                                * compute_gradient_shade(row as f64 + 0.5, col as f64 + 0.5, frame);
                            draw_horizontally_shifted_sub_block(
                                &mut resources,
                                namespace_id,
                                frame,
                                bulge_above,
                                row,
                                col as f64 + left_intersection,
                                col as f64 + 1.0 - right_intersection,
                                shade,
                            )?;
                            continue;
                        }
                    }

                    let gradient_shade =
                        compute_gradient_shade(row as f64 + 0.5, col as f64 + 0.5, frame);
                    if single_diagonal
                        && frame.use_diagonal_blocks
                        && frame.legacy_computing_symbols_support
                        && diagonal_edge_index.is_some()
                    {
                        let has_drawn = draw_diagonal_block(
                            &mut resources,
                            namespace_id,
                            frame,
                            &geometry,
                            diagonal_edge_index.unwrap_or(0),
                            row,
                            col,
                            gradient_shade,
                        )?;
                        if has_drawn {
                            continue;
                        }
                    }

                    let mut matrix = [[1.0_f64; 2]; 2];
                    for edge_index in 0..4 {
                        for fraction_index in 0..2 {
                            update_matrix_with_edge(
                                edge_index,
                                fraction_index,
                                row,
                                col,
                                &geometry,
                                &mut matrix,
                            );
                        }
                    }

                    draw_matrix_character(
                        &mut resources,
                        namespace_id,
                        frame,
                        row,
                        col,
                        matrix,
                        gradient_shade,
                    )?;
                }
            }
        }

        Ok(())
    })();

    hide_available_windows(&mut draw_state, namespace_id);
    draw_result
}
