use nvim_oxi::Result;
use nvim_oxi::mlua;
use nvim_oxi_utils::handles::{BufHandle, WinHandle};
use nvim_oxi_utils::{lua, notify};

use crate::LOG_CONTEXT;
use crate::core::PreviewToken;
use crate::state::{register_cleanup_key, take_all_cleanup_keys_and_reset, take_cleanup_key};

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
    let vim: mlua::Table = lua.globals().get("vim")?;
    let filetype: mlua::Table = vim.get("filetype")?;
    let matcher: mlua::Function = filetype.get("match")?;
    let args = lua.create_table()?;
    args.set("filename", path)?;
    let ft: Option<String> = matcher.call(args)?;
    Ok(ft.unwrap_or_default())
}

pub fn is_doc_preview_filetype(ft: &str) -> bool {
    matches!(
        ft,
        "markdown" | "markdown.mdx" | "mdx" | "typst" | "tex" | "plaintex" | "latex"
    )
}

pub fn snacks_has_doc_preview() -> bool {
    let lua = lua::state();
    let Some(snacks) = lua::try_require_table(&lua, "snacks") else {
        return false;
    };
    let Ok(image) = snacks.get::<mlua::Table>("image") else {
        return false;
    };
    let Ok(_doc) = image.get::<mlua::Table>("doc") else {
        return false;
    };
    let Ok(terminal) = image.get::<mlua::Table>("terminal") else {
        return false;
    };

    let inline_enabled = image
        .get::<mlua::Table>("config")
        .ok()
        .and_then(|config| config.get::<mlua::Table>("doc").ok())
        .and_then(|doc_config| doc_config.get::<bool>("inline").ok())
        .unwrap_or(false);
    if !inline_enabled {
        return true;
    }

    let Ok(env_fn) = terminal.get::<mlua::Function>("env") else {
        return true;
    };
    match env_fn.call::<mlua::Table>(()) {
        Ok(env) => !env.get::<bool>("placeholders").unwrap_or(false),
        Err(err) => {
            notify::warn(LOG_CONTEXT, &format!("snacks terminal env failed: {err}"));
            true
        }
    }
}

pub fn snacks_doc_find(
    buf_handle: BufHandle,
    token: PreviewToken,
    win_handle: WinHandle,
) -> Result<()> {
    let lua = lua::state();
    let Some(snacks) = lua::try_require_table(&lua, "snacks") else {
        return Ok(());
    };
    let Ok(image) = snacks.get::<mlua::Table>("image") else {
        return Ok(());
    };
    let Ok(doc) = image.get::<mlua::Table>("doc") else {
        return Ok(());
    };
    let Ok(find_visible) = doc.get::<mlua::Function>("find_visible") else {
        return Ok(());
    };

    let callback = lua
        .create_function(move |lua, imgs: mlua::Value| {
            let require: mlua::Function = match lua.globals().get("require") {
                Ok(require) => require,
                Err(err) => {
                    notify::warn(LOG_CONTEXT, &format!("Lua require unavailable: {err}"));
                    return Ok(());
                }
            };
            let preview = match require.call::<mlua::Table>("rs_snacks_preview") {
                Ok(preview) => preview,
                Err(err) => {
                    notify::warn(
                        LOG_CONTEXT,
                        &format!("load rs_snacks_preview failed: {err}"),
                    );
                    return Ok(());
                }
            };
            let on_doc_find = match preview.get::<mlua::Function>("on_doc_find") {
                Ok(function) => function,
                Err(err) => {
                    notify::warn(LOG_CONTEXT, &format!("resolve on_doc_find failed: {err}"));
                    return Ok(());
                }
            };
            let args = match lua.create_table() {
                Ok(args) => args,
                Err(err) => {
                    notify::warn(LOG_CONTEXT, &format!("create callback args failed: {err}"));
                    return Ok(());
                }
            };
            if let Err(err) = args.set("buf", buf_handle.raw()) {
                notify::warn(LOG_CONTEXT, &format!("set doc-find buf failed: {err}"));
                return Ok(());
            }
            if let Err(err) = args.set("token", token.raw()) {
                notify::warn(LOG_CONTEXT, &format!("set doc-find token failed: {err}"));
                return Ok(());
            }
            if let Err(err) = args.set("win", win_handle.raw()) {
                notify::warn(LOG_CONTEXT, &format!("set doc-find win failed: {err}"));
                return Ok(());
            }
            if let Err(err) = args.set("imgs", imgs) {
                notify::warn(LOG_CONTEXT, &format!("set doc-find imgs failed: {err}"));
                return Ok(());
            }
            if let Err(err) = on_doc_find.call::<()>(args) {
                notify::warn(LOG_CONTEXT, &format!("on_doc_find callback failed: {err}"));
            }
            Ok(())
        })
        .map_err(nvim_oxi::Error::from)?;

    find_visible
        .call::<()>((buf_handle.raw(), callback))
        .map_err(Into::into)
}

pub fn snacks_open_preview(win_handle: WinHandle, src: &str) -> Result<Option<i64>> {
    let lua = lua::state();
    let args = lua.create_table()?;
    args.set("win", win_handle.raw())?;
    args.set("src", src)?;
    let cleanup = call_bridge::<_, Option<mlua::Function>>(&lua, "snacks_open_preview", args)?;
    let Some(cleanup) = cleanup else {
        return Ok(None);
    };
    let cleanup_key = lua
        .create_registry_value(cleanup)
        .map_err(nvim_oxi::Error::from)?;
    let cleanup_id = register_cleanup_key(cleanup_key);
    Ok(Some(cleanup_id))
}

fn run_cleanup_registry_key(cleanup_key: mlua::RegistryKey) -> Result<()> {
    let lua = lua::state();
    let call_result = (|| -> Result<()> {
        let cleanup: mlua::Function = lua
            .registry_value(&cleanup_key)
            .map_err(nvim_oxi::Error::from)?;
        cleanup.call::<()>(()).map_err(nvim_oxi::Error::from)
    })();
    if let Err(err) = lua.remove_registry_value(cleanup_key) {
        notify::warn(
            LOG_CONTEXT,
            &format!("remove cleanup registry value failed: {err}"),
        );
    }
    call_result
}

pub fn snacks_close_preview(cleanup_id: i64) -> Result<()> {
    let Some(cleanup_key) = take_cleanup_key(cleanup_id) else {
        return Ok(());
    };
    run_cleanup_registry_key(cleanup_key)
}

pub fn reset_preview_state() -> Result<()> {
    let cleanup_keys = take_all_cleanup_keys_and_reset();
    for cleanup_key in cleanup_keys {
        if let Err(err) = run_cleanup_registry_key(cleanup_key) {
            notify::warn(LOG_CONTEXT, &format!("preview cleanup reset failed: {err}"));
        }
    }
    Ok(())
}
