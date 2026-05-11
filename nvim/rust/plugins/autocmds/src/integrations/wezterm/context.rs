use std::path::PathBuf;

use nvim_oxi::Array;
use nvim_oxi::Result;
use nvim_oxi::String as NvimString;
use nvim_oxi::api;
use nvim_oxi::api::opts::OptionOpts;
use nvimrs_support::ProjectRoot;

const PROJECT_ROOT_VAR: &str = "project_root";

#[derive(Debug, Clone)]
pub(super) struct WeztermContext {
    pub(super) home: Option<PathBuf>,
}

impl WeztermContext {
    pub(super) fn detect() -> Option<Self> {
        let in_wezterm = std::env::var_os("WEZTERM_PANE").is_some_and(|value| !value.is_empty());
        if !in_wezterm {
            return None;
        }
        let home = std::env::var_os("HOME").map(PathBuf::from);
        Some(Self { home })
    }
}

pub(super) fn current_buf_project_root() -> Result<Option<ProjectRoot>> {
    let buf = api::get_current_buf();
    if !buf.is_valid() {
        return Ok(None);
    }
    let root = match buf.get_var::<NvimString>(PROJECT_ROOT_VAR) {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };
    Ok(ProjectRoot::try_new(root.to_string_lossy().into_owned()).ok())
}

pub(super) fn current_window_cwd() -> Result<Option<String>> {
    let cwd: NvimString = api::call_function("getcwd", Array::new())?;
    let cwd = cwd.to_string_lossy().into_owned();
    if !crate::is_dir(&cwd) {
        return Ok(None);
    }
    Ok(Some(cwd))
}

pub(super) fn should_skip_sync_for_current_buffer() -> Result<bool> {
    let current = api::get_current_buf();
    if !current.is_valid() {
        return Ok(true);
    }
    let buftype: NvimString =
        api::get_option_value("buftype", &OptionOpts::builder().buf(current).build())?;
    Ok(!buftype_requires_wezterm_sync(&buftype.to_string_lossy()))
}

fn buftype_requires_wezterm_sync(buftype: &str) -> bool {
    matches!(buftype, "" | "acwrite")
}

#[cfg(test)]
mod tests {
    #[test]
    fn buftype_requires_wezterm_sync_allows_normal_and_acwrite_buffers() {
        assert!(super::buftype_requires_wezterm_sync(""));
        assert!(super::buftype_requires_wezterm_sync("acwrite"));
    }

    #[test]
    fn buftype_requires_wezterm_sync_skips_special_ui_buffers() {
        assert!(!super::buftype_requires_wezterm_sync("terminal"));
        assert!(!super::buftype_requires_wezterm_sync("nofile"));
        assert!(!super::buftype_requires_wezterm_sync("prompt"));
    }
}
