use std::path::{Path, PathBuf};

use once_cell::sync::Lazy;

use nvim_oxi::api::opts::{CreateAugroupOpts, CreateAutocmdOpts, OptionOpts, OptionScope};
use nvim_oxi::api::types::AutocmdCallbackArgs;
use nvim_oxi::api::{self, Buffer};
use nvim_oxi::conversion::FromObject;
use nvim_oxi::{Array, Dictionary, Function, Object, Result, String as NvimString};
use nvim_oxi_utils::{guard, notify, state::StateCell};
use nvim_utils::path::{has_uri_scheme, normalize_path, path_is_dir, strip_known_prefixes};

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
const LOG_CONTEXT: &str = "project_root";

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

static STATE: Lazy<StateCell<State>> = Lazy::new(|| StateCell::new(State::default()));

fn state_lock() -> nvim_oxi_utils::state::StateGuard<'static, State> {
    let guard = STATE.lock();
    if guard.poisoned() {
        notify::warn(LOG_CONTEXT, "state mutex poisoned; continuing");
    }
    guard
}

fn report_panic(label: &str, info: guard::PanicInfo) {
    notify::error(LOG_CONTEXT, &format!("{label} panic: {}", info.render()));
}

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
    let Ok(value) = api::get_var::<Object>("project_root_debug") else {
        return false;
    };
    truthy_object(value)
}

fn debug_log<F>(build: F)
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
        let value = normalized
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "<none>".to_string());
        format!(
            "get_path_from_buffer: buf={} normalized='{}'",
            buf.handle(),
            value
        )
    });
    Ok(normalized)
}

fn cached_or_refresh_root(buf: &Buffer) -> Result<Option<String>> {
    if let Some(root) = cached_root_for_buffer(buf)? {
        return Ok(Some(root));
    }

    refresh_root_for_buffer(buf)
}

fn root_from_path_with<F, G>(
    path: &Path,
    indicators: &[String],
    is_dir: F,
    exists: G,
) -> Option<PathBuf>
where
    F: Fn(&Path) -> bool,
    G: Fn(&Path) -> bool,
{
    if indicators.is_empty() {
        debug_log(|| "root_from_path: no indicators".to_string());
        return None;
    }

    let mut current = path.to_path_buf();
    if !is_dir(&current) {
        current = current.parent()?.to_path_buf();
    }

    loop {
        for indicator in indicators {
            let candidate = current.join(indicator);
            if exists(&candidate) {
                debug_log(|| {
                    format!(
                        "root_from_path: path='{}' found='{}' via '{}'",
                        path.display(),
                        current.display(),
                        indicator
                    )
                });
                return Some(current);
            }
        }
        if !current.pop() {
            break;
        }
    }

    debug_log(|| {
        format!(
            "root_from_path: path='{}' no root (indicators={:?})",
            path.display(),
            indicators
        )
    });
    None
}

fn root_from_path(path: &Path) -> Option<PathBuf> {
    let indicators = {
        let state = state_lock();
        state.root_indicators.clone()
    };
    root_from_path_with(
        path,
        &indicators,
        |dir| path_is_dir(dir),
        |candidate| candidate.exists(),
    )
}

fn refresh_root_for_buffer(buf: &Buffer) -> Result<Option<String>> {
    if !buf.is_valid() {
        debug_log(|| "refresh_root_for_buffer: buffer invalid".to_string());
        return Ok(None);
    }
    let path = match get_path_from_buffer(buf)? {
        Some(path) => path,
        None => {
            debug_log(|| format!("refresh_root_for_buffer: buf={} no path", buf.handle()));
            set_buf_root(buf, None, None)?;
            return Ok(None);
        }
    };

    let path_string = path.to_string_lossy().into_owned();
    let root = root_from_path(&path).map(|root| root.to_string_lossy().into_owned());
    set_buf_root(buf, root.as_deref(), Some(&path_string))?;
    debug_log(|| {
        format!(
            "refresh_root_for_buffer: buf={} path='{}' root='{}'",
            buf.handle(),
            path_string,
            root.as_deref().unwrap_or("<none>")
        )
    });

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
        debug_log(|| format!("cached_root_for_buffer: buf={} no path", buf.handle()));
        return Ok(None);
    };
    let path_string = path.to_string_lossy().into_owned();
    let cached = get_cached_root(buf, &path_string);
    debug_log(|| {
        format!(
            "cached_root_for_buffer: buf={} path='{}' cached='{}'",
            buf.handle(),
            path_string,
            cached.as_deref().unwrap_or("<none>")
        )
    });
    Ok(cached)
}

fn get_project_root() -> Result<Option<String>> {
    let buf = api::get_current_buf();
    debug_log(|| format!("get_project_root: current buf={}", buf.handle()));
    if let Some(root) = cached_or_refresh_root(&buf)? {
        debug_log(|| {
            format!(
                "get_project_root: current buf={} root='{}'",
                buf.handle(),
                root
            )
        });
        return Ok(Some(root));
    }

    let alt: i64 = api::call_function("bufnr", Array::from_iter(["#"]))?;
    let alt_root = if alt <= 0 {
        debug_log(|| "get_project_root: no alternate buffer".to_string());
        None
    } else if let Ok(handle) = i32::try_from(alt) {
        let alt_buf = Buffer::from(handle);
        let root = cached_or_refresh_root(&alt_buf)?;
        debug_log(|| {
            format!(
                "get_project_root: alternate buf={} root='{}'",
                handle,
                root.as_deref().unwrap_or("<none>")
            )
        });
        root
    } else {
        debug_log(|| {
            format!(
                "get_project_root: alternate buffer handle overflow (value={})",
                alt
            )
        });
        None
    };

    if alt_root.is_some() {
        return Ok(alt_root);
    }

    let cwd: NvimString = api::call_function("getcwd", Array::new())?;
    let cwd = cwd.to_string_lossy().into_owned();
    if cwd.is_empty() {
        debug_log(|| "get_project_root: empty cwd".to_string());
        return Ok(None);
    }
    let normalized = normalize_path(&cwd);
    debug_log(|| {
        let value = normalized
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "<none>".to_string());
        format!("get_project_root: cwd='{}' normalized='{}'", cwd, value)
    });
    let root = normalized
        .as_ref()
        .and_then(|path| root_from_path(path))
        .map(|path| path.to_string_lossy().into_owned());
    debug_log(|| {
        format!(
            "get_project_root: cwd root='{}'",
            root.as_deref().unwrap_or("<none>")
        )
    });
    Ok(root)
}

fn setup_autocmd() -> Result<()> {
    let group = api::create_augroup(
        PROJECT_ROOT_GROUP,
        &CreateAugroupOpts::builder().clear(true).build(),
    )?;

    let opts = CreateAutocmdOpts::builder()
        .group(group)
        .callback(|args: AutocmdCallbackArgs| {
            guard::with_panic(
                false,
                || {
                    if let Err(err) = refresh_root_for_buffer(&args.buffer) {
                        notify::warn(
                            LOG_CONTEXT,
                            &format!("refresh_root_for_buffer failed: {err}"),
                        );
                    }
                    false
                },
                |info| report_panic("refresh_root_for_buffer", info),
            )
        })
        .build();

    api::create_autocmd(["BufEnter", "BufWinEnter", "BufFilePost"], &opts)?;
    Ok(())
}

fn update_root_indicators(config: Option<Dictionary>) {
    let mut state = state_lock();
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
        let mut state = state_lock();
        if state.did_setup {
            false
        } else {
            state.did_setup = true;
            true
        }
    };

    if should_setup {
        setup_autocmd()?;
    }

    let _ = refresh_root_for_buffer(&api::get_current_buf());
    Ok(())
}

fn swap_root(buf: Option<Buffer>) -> Result<()> {
    let buf = buf.unwrap_or_else(api::get_current_buf);
    let _ = refresh_root_for_buffer(&buf)?;
    Ok(())
}

fn project_root_value() -> Result<Option<String>> {
    get_project_root()
}

fn project_root_or_warn_value() -> Result<Option<String>> {
    let root = get_project_root()?;
    if root.is_none() {
        notify::warn("", "No project root found");
    }
    Ok(root)
}

fn show_project_root() -> Result<()> {
    let Some(root) = get_project_root()? else {
        notify::warn("", "No project root found");
        return Ok(());
    };

    let display = api::call_function::<_, NvimString>(
        "fnamemodify",
        nvim_oxi::Array::from_iter([root.as_str(), ":~"]),
    )
    .ok()
    .map(|value| value.to_string_lossy().into_owned())
    .unwrap_or(root);

    notify::info("", &display);
    Ok(())
}

#[nvim_oxi::plugin]
fn project_root() -> Result<Dictionary> {
    let mut api = Dictionary::new();
    api.insert("setup", Function::<Option<Dictionary>, ()>::from_fn(setup));
    api.insert(
        "swap_root",
        Function::<Option<Buffer>, ()>::from_fn(swap_root),
    );
    api.insert(
        "project_root",
        Function::<(), Option<String>>::from_fn(|()| project_root_value()),
    );
    api.insert(
        "project_root_or_warn",
        Function::<(), Option<String>>::from_fn(|()| project_root_or_warn_value()),
    );
    api.insert(
        "show_project_root",
        Function::<(), ()>::from_fn(|()| show_project_root()),
    );
    Ok(api)
}
