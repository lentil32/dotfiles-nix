use nvim_oxi::Result;
use nvim_oxi::mlua;
use nvim_oxi_utils::handles::{BufHandle, WinHandle};
use nvim_oxi_utils::{lua, notify};

use crate::LOG_CONTEXT;

const BRIDGE_MODULE: &str = "myLuaConf.snacks_preview_bridge";

fn bridge_table(lua: &mlua::Lua) -> Result<mlua::Table> {
    lua::require_table(lua, BRIDGE_MODULE)
}

fn call_bridge<A, R>(lua: &mlua::Lua, name: &str, args: A) -> Result<R>
where
    A: mlua::IntoLuaMulti,
    R: mlua::FromLuaMulti,
{
    let bridge = bridge_table(lua)?;
    lua::call_table_function(&bridge, name, args)
}

pub fn filetype_for_path(path: &str) -> Result<String> {
    let lua = lua::state();
    let ft: Option<String> = call_bridge(&lua, "filetype_match", (path,))?;
    Ok(ft.map_or_else(String::new, |value| value))
}

pub fn is_doc_preview_filetype(ft: &str) -> bool {
    matches!(
        ft,
        "markdown" | "markdown.mdx" | "mdx" | "typst" | "tex" | "plaintex" | "latex"
    )
}

pub fn snacks_has_doc_preview() -> bool {
    let lua = lua::state();
    match call_bridge::<_, bool>(&lua, "snacks_has_doc", ()) {
        Ok(value) => value,
        Err(err) => {
            notify::warn(
                LOG_CONTEXT,
                &format!("snacks doc preview check failed: {err}"),
            );
            false
        }
    }
}

pub fn snacks_doc_find(buf_handle: BufHandle, token: i64, win_handle: WinHandle) -> Result<()> {
    let lua = lua::state();
    let args = lua.create_table()?;
    args.set("buf", buf_handle.raw())?;
    args.set("token", token)?;
    args.set("win", win_handle.raw())?;
    call_bridge(&lua, "snacks_doc_find", args)
}

pub fn snacks_open_preview(win_handle: WinHandle, src: &str) -> Result<Option<i64>> {
    let lua = lua::state();
    let args = lua.create_table()?;
    args.set("win", win_handle.raw())?;
    args.set("src", src)?;
    call_bridge(&lua, "snacks_open_preview", args)
}

pub fn snacks_close_preview(cleanup_id: i64) -> Result<()> {
    let lua = lua::state();
    call_bridge(&lua, "snacks_close_preview", cleanup_id)
}
