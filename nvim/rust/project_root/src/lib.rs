use std::{
    fs,
    path::{Component, Path, PathBuf},
    sync::Mutex,
};

use once_cell::sync::Lazy;

use nvim_oxi::api::opts::{CreateAugroupOpts, CreateAutocmdOpts, OptionOpts};
use nvim_oxi::api::types::{AutocmdCallbackArgs, LogLevel};
use nvim_oxi::api::{self, Buffer};
use nvim_oxi::conversion::FromObject;
use nvim_oxi::{Dictionary, Function, Result, String as NvimString};

const ROOT_VAR: &str = "project_root";
const ROOT_FOR_VAR: &str = "project_root_for";
const DEFAULT_ROOT_INDICATORS: &[&str] = &[
    ".git",
    "package.json",
    "Cargo.toml",
    "flake.nix",
    "Makefile",
];
const PROJECT_ROOT_GROUP: &str = "ProjectRoot";

fn default_root_indicators() -> Vec<String> {
    DEFAULT_ROOT_INDICATORS
        .iter()
        .map(|val| (*val).to_string())
        .collect()
}

#[derive(Debug)]
struct State {
    root_indicators: Vec<String>,
    did_setup: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            root_indicators: default_root_indicators(),
            did_setup: false,
        }
    }
}

static STATE: Lazy<Mutex<State>> = Lazy::new(|| Mutex::new(State::default()));

fn normalize_path(path: &str) -> Option<PathBuf> {
    if path.is_empty() {
        return None;
    }
    let normalized = Path::new(path)
        .components()
        .fold(PathBuf::new(), |mut acc, component| {
            match component {
                Component::CurDir => {}
                Component::ParentDir => {
                    acc.pop();
                }
                Component::RootDir | Component::Prefix(_) | Component::Normal(_) => {
                    acc.push(component.as_os_str());
                }
            }
            acc
        });
    Some(normalized)
}

fn path_is_dir(path: &Path) -> bool {
    fs::metadata(path)
        .map(|meta| meta.is_dir())
        .unwrap_or(false)
}

fn has_uri_scheme(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphabetic() {
        return false;
    }
    for ch in chars {
        if ch == ':' {
            return value.contains("://");
        }
        if !(ch.is_ascii_alphanumeric() || ch == '+' || ch == '-' || ch == '.') {
            return false;
        }
    }
    false
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
        None => buf.set_var(var, false)?,
    }
    Ok(())
}

fn set_buf_root(buf: &Buffer, root: Option<&str>, path: Option<&str>) -> Result<()> {
    if !buf.is_valid() {
        return Ok(());
    }
    let mut buf = buf.clone();
    set_buf_var(&mut buf, ROOT_VAR, root)?;
    set_buf_var(&mut buf, ROOT_FOR_VAR, path)?;
    Ok(())
}

fn get_path_from_buffer(buf: &Buffer) -> Result<Option<PathBuf>> {
    if !buf.is_valid() {
        return Ok(None);
    }
    let name = buf.get_name()?;
    if name.as_os_str().is_empty() {
        return Ok(None);
    }
    let mut path = name.to_string_lossy().into_owned();
    if let Some(stripped) = path.strip_prefix("oil://") {
        path = stripped.to_string();
    }

    let bt: NvimString = api::get_option_value(
        "buftype",
        &OptionOpts::builder().buffer(buf.clone()).build(),
    )?;
    if !bt.is_empty() {
        return Ok(None);
    }

    if has_uri_scheme(&path) && !path.starts_with("file://") {
        return Ok(None);
    }

    Ok(normalize_path(&path))
}

fn root_from_path(path: &Path) -> Option<PathBuf> {
    let indicators = {
        let state = STATE.lock().expect("project_root state poisoned");
        state.root_indicators.clone()
    };
    if indicators.is_empty() {
        return None;
    }

    let mut current = path.to_path_buf();
    if !path_is_dir(&current) {
        current = current.parent()?.to_path_buf();
    }

    loop {
        for indicator in &indicators {
            if current.join(indicator).exists() {
                return Some(current);
            }
        }
        if !current.pop() {
            break;
        }
    }

    None
}

fn refresh_root_for_buffer(buf: &Buffer) -> Result<Option<String>> {
    if !buf.is_valid() {
        return Ok(None);
    }
    let path = match get_path_from_buffer(buf)? {
        Some(path) => path,
        None => {
            set_buf_root(buf, None, None)?;
            return Ok(None);
        }
    };

    let path_string = path.to_string_lossy().into_owned();
    let root = root_from_path(&path).map(|root| root.to_string_lossy().into_owned());
    set_buf_root(buf, root.as_deref(), Some(&path_string))?;

    Ok(root)
}

fn get_cached_root(buf: &Buffer, path: &str) -> Option<String> {
    let cached_for = get_buf_root_for(buf)?;
    if cached_for != path {
        return None;
    }
    get_buf_root(buf)
}

fn cached_root_for_buffer(buf: &Buffer) -> Result<Option<String>> {
    let Some(path) = get_path_from_buffer(buf)? else {
        return Ok(None);
    };
    let path_string = path.to_string_lossy().into_owned();
    Ok(get_cached_root(buf, &path_string))
}

fn get_project_root() -> Result<Option<String>> {
    let buf = api::get_current_buf();
    if let Some(root) = cached_root_for_buffer(&buf)? {
        return Ok(Some(root));
    }

    let alt: i64 = api::call_function("bufnr", nvim_oxi::Array::from_iter(["#"]))?;
    if alt <= 0 {
        return Ok(None);
    }
    let Ok(handle) = i32::try_from(alt) else {
        return Ok(None);
    };
    let alt_buf = Buffer::from(handle);

    cached_root_for_buffer(&alt_buf)
}

fn setup_autocmd() -> Result<()> {
    let group = api::create_augroup(
        PROJECT_ROOT_GROUP,
        &CreateAugroupOpts::builder().clear(true).build(),
    )?;

    let opts = CreateAutocmdOpts::builder()
        .group(group)
        .callback(|args: AutocmdCallbackArgs| {
            let _ = refresh_root_for_buffer(&args.buffer);
            false
        })
        .build();

    api::create_autocmd(["BufEnter", "BufWinEnter", "BufFilePost"], &opts)?;
    Ok(())
}

fn update_root_indicators(config: Option<Dictionary>) {
    let mut state = STATE.lock().expect("project_root state poisoned");
    let default_indicators = default_root_indicators();

    let Some(config) = config else {
        state.root_indicators = default_indicators;
        return;
    };

    let key = NvimString::from("root_indicators");
    let Some(value) = config.get(&key) else {
        state.root_indicators = default_indicators;
        return;
    };

    match Vec::<NvimString>::from_object(value.clone()) {
        Ok(values) => {
            state.root_indicators = values
                .into_iter()
                .map(|val| val.to_string_lossy().into_owned())
                .collect();
        }
        Err(_) => {
            state.root_indicators = default_indicators;
        }
    }
}

fn setup(config: Option<Dictionary>) -> Result<()> {
    update_root_indicators(config);

    let should_setup = {
        let mut state = STATE.lock().expect("project_root state poisoned");
        if state.did_setup {
            false
        } else {
            state.did_setup = true;
            true
        }
    };

    if should_setup {
        setup_autocmd()
    } else {
        Ok(())
    }
}

fn swap_root(buf: Option<Buffer>) -> Result<()> {
    let buf = buf.unwrap_or_else(api::get_current_buf);
    let _ = refresh_root_for_buffer(&buf)?;
    Ok(())
}

fn project_root() -> Result<Option<String>> {
    get_project_root()
}

fn show_project_root() -> Result<()> {
    let Some(root) = get_project_root()? else {
        api::notify("No project root found", LogLevel::Warn, &Dictionary::new())?;
        return Ok(());
    };

    let display = api::call_function::<_, NvimString>(
        "fnamemodify",
        nvim_oxi::Array::from_iter([root.as_str(), ":~"]),
    )
    .ok()
    .map(|value| value.to_string_lossy().into_owned())
    .unwrap_or(root);

    api::notify(&display, LogLevel::Info, &Dictionary::new())?;
    Ok(())
}

#[nvim_oxi::plugin]
fn project_root_plugin() -> Result<Dictionary> {
    let mut api = Dictionary::new();
    api.insert("setup", Function::<Option<Dictionary>, ()>::from_fn(setup));
    api.insert(
        "swap_root",
        Function::<Option<Buffer>, ()>::from_fn(swap_root),
    );
    api.insert(
        "project_root",
        Function::<(), Option<String>>::from_fn(|()| project_root()),
    );
    api.insert(
        "show_project_root",
        Function::<(), ()>::from_fn(|()| show_project_root()),
    );
    Ok(api)
}
