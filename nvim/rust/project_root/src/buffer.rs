use std::path::PathBuf;

use nvim_oxi::api;
use nvim_oxi::api::Buffer;
use nvim_oxi::api::opts::{OptionOpts, OptionScope};
use nvim_oxi::conversion::FromObject;
use nvim_oxi::{Object, Result, String as NvimString};
use nvim_oxi_utils::notify;
use nvim_utils::path::{has_uri_scheme, normalize_path, strip_known_prefixes};

const ROOT_VAR: &str = "project_root";
const ROOT_FOR_VAR: &str = "project_root_for";
const DEBUG_VAR: &str = "project_root_debug";

fn truthy_object(obj: Object) -> bool {
    if let Ok(value) = bool::from_object(obj.clone()) {
        return value;
    }
    if let Ok(value) = i64::from_object(obj.clone()) {
        return value != 0;
    }
    if let Ok(value) = f64::from_object(obj.clone()) {
        return value != 0.0;
    }
    if let Ok(value) = NvimString::from_object(obj) {
        let value = value.to_string_lossy().to_ascii_lowercase();
        return matches!(value.as_str(), "1" | "true" | "yes" | "on");
    }
    false
}

fn debug_enabled() -> bool {
    let Ok(value) = api::get_var::<Object>(DEBUG_VAR) else {
        return false;
    };
    truthy_object(value)
}

pub fn debug_log<F>(build: F)
where
    F: FnOnce() -> String,
{
    if !debug_enabled() {
        return;
    }
    let message = build();
    notify::info("", &message);
}

fn get_buf_var(buf: &Buffer, var: &str) -> Option<String> {
    if !buf.is_valid() {
        return None;
    }
    buf.get_var::<NvimString>(var)
        .ok()
        .map(|val| val.to_string_lossy().into_owned())
}

fn get_buf_root(buf: &Buffer) -> Option<String> {
    get_buf_var(buf, ROOT_VAR)
}

fn get_buf_root_for(buf: &Buffer) -> Option<String> {
    get_buf_var(buf, ROOT_FOR_VAR)
}

fn set_buf_var(buf: &mut Buffer, var: &str, value: Option<&str>) -> Result<()> {
    match value {
        Some(value) => buf.set_var(var, value)?,
        None => {
            if let Err(err) = buf.del_var(var) {
                debug_log(|| format!("set_buf_var: del_var {var} failed: {err}"));
            }
        }
    }
    Ok(())
}

pub fn set_buf_root(buf: &Buffer, root: Option<&str>, path: Option<&str>) -> Result<()> {
    if !buf.is_valid() {
        return Ok(());
    }
    let mut buf = buf.clone();
    set_buf_var(&mut buf, ROOT_VAR, root)?;
    set_buf_var(&mut buf, ROOT_FOR_VAR, path)?;
    Ok(())
}

pub fn get_path_from_buffer(buf: &Buffer) -> Result<Option<PathBuf>> {
    if !buf.is_valid() {
        debug_log(|| "get_path_from_buffer: buffer invalid".to_string());
        return Ok(None);
    }
    let name = buf.get_name()?;
    if name.is_empty() {
        debug_log(|| format!("get_path_from_buffer: buf={} empty name", buf.handle()));
        return Ok(None);
    }
    let raw_path = name.to_string_lossy().into_owned();
    debug_log(|| {
        format!(
            "get_path_from_buffer: buf={} name='{}'",
            buf.handle(),
            raw_path
        )
    });
    let stripped = strip_known_prefixes(&raw_path);
    if stripped != raw_path {
        debug_log(|| {
            format!(
                "get_path_from_buffer: buf={} stripped path='{}'",
                buf.handle(),
                stripped
            )
        });
    }

    let bt: NvimString = api::get_option_value(
        "buftype",
        &OptionOpts::builder()
            .scope(OptionScope::Local)
            .buf(buf.clone())
            .build(),
    )?;
    let allow_nonfile_buftype = raw_path.starts_with("file://") || raw_path.starts_with("oil://");
    if !bt.is_empty() && !allow_nonfile_buftype {
        debug_log(|| {
            format!(
                "get_path_from_buffer: buf={} buftype='{}' -> skip",
                buf.handle(),
                bt.to_string_lossy()
            )
        });
        return Ok(None);
    }

    if has_uri_scheme(&raw_path)
        && !raw_path.starts_with("file://")
        && !raw_path.starts_with("oil://")
    {
        debug_log(|| {
            format!(
                "get_path_from_buffer: buf={} uri scheme in '{}' -> skip",
                buf.handle(),
                raw_path
            )
        });
        return Ok(None);
    }

    let normalized = normalize_path(stripped);
    debug_log(|| {
        let value = normalized.as_ref().map_or_else(
            || "<none>".to_string(),
            |path| path.to_string_lossy().into_owned(),
        );
        format!(
            "get_path_from_buffer: buf={} normalized='{}'",
            buf.handle(),
            value
        )
    });
    Ok(normalized)
}

fn get_cached_root(buf: &Buffer, path: &str) -> Option<String> {
    let cached_for = get_buf_root_for(buf)?;
    if cached_for != path {
        return None;
    }
    get_buf_root(buf)
}

pub fn cached_root_for_buffer(buf: &Buffer) -> Result<Option<String>> {
    let Some(path) = get_path_from_buffer(buf)? else {
        debug_log(|| format!("cached_root_for_buffer: buf={} no path", buf.handle()));
        return Ok(None);
    };
    let path_string = path.to_string_lossy().into_owned();
    let cached = get_cached_root(buf, &path_string);
    debug_log(|| {
        let cached_value = cached.as_deref().unwrap_or("<none>");
        format!(
            "cached_root_for_buffer: buf={} path='{}' cached='{}'",
            buf.handle(),
            path_string,
            cached_value
        )
    });
    Ok(cached)
}
