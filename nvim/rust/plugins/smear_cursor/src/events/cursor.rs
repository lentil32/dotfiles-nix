use super::CURSOR_COLOR_LUAEVAL_EXPR;
use super::logging::warn;
use crate::lua::{i64_from_object, parse_indexed_objects, string_from_object};
use crate::state::RuntimeState;
use crate::types::Point;
use nvim_oxi::api::opts::OptionOpts;
use nvim_oxi::conversion::FromObject;
use nvim_oxi::{Array, Dictionary, Object, Result, String as NvimString, api};
use nvim_utils::mode::is_cmdline_mode;

pub(super) fn mode_string() -> String {
    api::get_mode().mode.to_string_lossy().into_owned()
}

fn screen_cursor_position(window: &api::Window) -> Result<Option<(f64, f64)>> {
    let mut row = i64_from_object("screenrow", api::call_function("screenrow", Array::new())?)?;
    let mut col = i64_from_object("screencol", api::call_function("screencol", Array::new())?)?;

    if window.get_config()?.relative.is_some() {
        let wininfo_args = Array::from_iter([Object::from(window.handle())]);
        let wininfo = api::call_function("getwininfo", wininfo_args)?;
        let entries = parse_indexed_objects("getwininfo", wininfo, Some(1))
            .map_err(|_| nvim_oxi::api::Error::Other("getwininfo returned no entries".into()))?;
        let wininfo_entry = Dictionary::from_object(entries[0].clone())
            .map_err(|_| nvim_oxi::api::Error::Other("getwininfo[1] invalid dictionary".into()))?;

        let winrow_obj = wininfo_entry
            .get(&NvimString::from("winrow"))
            .cloned()
            .ok_or_else(|| nvim_oxi::api::Error::Other("getwininfo.winrow missing".into()))?;
        let wincol_obj = wininfo_entry
            .get(&NvimString::from("wincol"))
            .cloned()
            .ok_or_else(|| nvim_oxi::api::Error::Other("getwininfo.wincol missing".into()))?;
        let info_row = i64_from_object("getwininfo.winrow", winrow_obj)?;
        let info_col = i64_from_object("getwininfo.wincol", wincol_obj)?;

        row = row.saturating_add(info_row.saturating_sub(1));
        col = col.saturating_add(info_col.saturating_sub(1));
    }

    Ok(Some((row as f64, col as f64)))
}

fn cmdline_cursor_position() -> Result<Option<(f64, f64)>> {
    let cmdpos_value = api::call_function("getcmdpos", Array::new())?;
    let cmdpos = i64_from_object("getcmdpos", cmdpos_value)?;

    if let Ok(ui_cmdline_pos) = api::get_var::<Object>("ui_cmdline_pos") {
        if let Ok(indexed) =
            parse_indexed_objects("ui_cmdline_pos", ui_cmdline_pos.clone(), Some(2))
        {
            let row = i64_from_object("ui_cmdline_pos[1]", indexed[0].clone())?;
            let col = i64_from_object("ui_cmdline_pos[2]", indexed[1].clone())?;
            let final_col = col.saturating_add(cmdpos).saturating_add(1);
            return Ok(Some((row as f64, final_col as f64)));
        } else if let Ok(dict) = Dictionary::from_object(ui_cmdline_pos) {
            let maybe_row = dict.get(&NvimString::from("row")).cloned();
            let maybe_col = dict.get(&NvimString::from("col")).cloned();
            if let (Some(row_obj), Some(col_obj)) = (maybe_row, maybe_col) {
                let row = i64_from_object("ui_cmdline_pos.row", row_obj)?;
                let col = i64_from_object("ui_cmdline_pos.col", col_obj)?;
                let final_col = col.saturating_add(cmdpos).saturating_add(1);
                return Ok(Some((row as f64, final_col as f64)));
            }
        }
    }

    let opts = nvim_oxi::api::opts::OptionOpts::builder().build();
    let lines: i64 = api::get_option_value("lines", &opts)?;
    let cmdheight: i64 = api::get_option_value("cmdheight", &opts)?;
    let row = lines.saturating_sub(cmdheight).saturating_add(1);
    let col = cmdpos.saturating_add(1);
    Ok(Some((row as f64, col as f64)))
}

pub(super) fn cursor_position_for_mode(
    window: &api::Window,
    mode: &str,
    smear_to_cmd: bool,
) -> Result<Option<(f64, f64)>> {
    if is_cmdline_mode(mode) {
        if !smear_to_cmd {
            return Ok(None);
        }
        return cmdline_cursor_position();
    }
    screen_cursor_position(window)
}

fn current_buffer_option_string(buffer: &api::Buffer, option_name: &str) -> Result<String> {
    let opts = OptionOpts::builder().buf(buffer.clone()).build();
    let value: String = api::get_option_value(option_name, &opts)?;
    Ok(value)
}

pub(super) fn current_buffer_filetype(buffer: &api::Buffer) -> Result<String> {
    current_buffer_option_string(buffer, "filetype")
}

fn cursor_color_at_current_position() -> Result<Option<String>> {
    let args = Array::from_iter([Object::from(CURSOR_COLOR_LUAEVAL_EXPR)]);
    let value: Object = api::call_function("luaeval", args)?;
    if value.is_nil() {
        return Ok(None);
    }
    Ok(Some(string_from_object("cursor_color_luaeval", value)?))
}

pub(super) fn update_tracked_cursor_color(state: &mut RuntimeState) {
    if !state.config.requires_cursor_color_sampling() {
        state.clear_color_at_cursor();
        return;
    }

    match cursor_color_at_current_position() {
        Ok(color) => state.set_color_at_cursor(color),
        Err(err) => warn(&format!("cursor color sampling failed: {err}")),
    }
}

pub(super) fn line_value(key: &str) -> Result<i64> {
    let args = Array::from_iter([Object::from(key)]);
    let value = api::call_function("line", args)?;
    i64_from_object("line", value)
}

fn command_row() -> Result<f64> {
    let opts = nvim_oxi::api::opts::OptionOpts::builder().build();
    let lines: i64 = api::get_option_value("lines", &opts)?;
    let cmdheight: i64 = api::get_option_value("cmdheight", &opts)?;
    Ok(lines.saturating_sub(cmdheight).saturating_add(1) as f64)
}

pub(super) fn smear_outside_cmd_row(corners: &[Point; 4]) -> Result<bool> {
    let cmd_row = command_row()?;
    Ok(corners.iter().any(|point| point.row < cmd_row))
}
