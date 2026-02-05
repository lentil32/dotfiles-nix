use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use nvim_oxi::api;
use nvim_oxi::api::Buffer;
use nvim_oxi::api::opts::{CreateAugroupOpts, CreateAutocmdOpts};
use nvim_oxi::api::types::AutocmdCallbackArgs;
use nvim_oxi::{Array, Dictionary, Function, Result, String as NvimString};
use nvim_oxi_utils::{guard, notify, state::StateCell};
use nvim_utils::path::path_is_dir;
use project_root_core::{
    RootIndicators, default_root_indicators, root_from_path_with as core_root_from_path_with,
};

use crate::buffer::{cached_root_for_buffer, debug_log, get_path_from_buffer, set_buf_root};
use crate::config::ProjectRootConfig;

const PROJECT_ROOT_GROUP: &str = "ProjectRoot";
const LOG_CONTEXT: &str = "project_root";

#[derive(Debug)]
struct State {
    root_indicators: RootIndicators,
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

static STATE: LazyLock<StateCell<State>> = LazyLock::new(|| StateCell::new(State::default()));

fn state_lock() -> nvim_oxi_utils::state::StateGuard<'static, State> {
    let guard = STATE.lock();
    if guard.poisoned() {
        notify::warn(LOG_CONTEXT, "state mutex poisoned; continuing");
    }
    guard
}

fn report_panic(label: &str, info: &guard::PanicInfo) {
    notify::error(LOG_CONTEXT, &format!("{label} panic: {}", info.render()));
}

fn cached_or_refresh_root(buf: &Buffer) -> Result<Option<String>> {
    if let Some(root) = cached_root_for_buffer(buf)? {
        return Ok(Some(root));
    }

    refresh_root_for_buffer(buf)
}

fn root_from_path(path: &Path) -> Option<PathBuf> {
    let indicators = {
        let state = state_lock();
        state.root_indicators.clone()
    };
    let root = core_root_from_path_with(path, &indicators, path_is_dir, Path::exists);
    debug_log(|| {
        root.as_ref().map_or_else(
            || {
                format!(
                    "root_from_path: path='{}' no root (indicators={:?})",
                    path.display(),
                    indicators
                )
            },
            |root| {
                format!(
                    "root_from_path: path='{}' found='{}'",
                    path.display(),
                    root.display()
                )
            },
        )
    });
    root
}

fn refresh_root_for_buffer(buf: &Buffer) -> Result<Option<String>> {
    if !buf.is_valid() {
        debug_log(|| "refresh_root_for_buffer: buffer invalid".to_string());
        return Ok(None);
    }
    let Some(path) = get_path_from_buffer(buf)? else {
        debug_log(|| format!("refresh_root_for_buffer: buf={} no path", buf.handle()));
        set_buf_root(buf, None, None)?;
        return Ok(None);
    };

    let path_string = path.to_string_lossy().into_owned();
    let root = root_from_path(&path).map(|root| root.to_string_lossy().into_owned());
    set_buf_root(buf, root.as_deref(), Some(&path_string))?;
    debug_log(|| {
        let root_value = root.as_deref().unwrap_or("<none>");
        format!(
            "refresh_root_for_buffer: buf={} path='{}' root='{}'",
            buf.handle(),
            path_string,
            root_value
        )
    });

    Ok(root)
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
            let root_value = root.as_deref().unwrap_or("<none>");
            format!("get_project_root: alternate buf={handle} root='{root_value}'")
        });
        root
    } else {
        debug_log(|| format!("get_project_root: alternate buffer handle overflow (value={alt})"));
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
    let normalized = nvim_utils::path::normalize_path(&cwd);
    debug_log(|| {
        let value = normalized.as_ref().map_or_else(
            || "<none>".to_string(),
            |path| path.to_string_lossy().into_owned(),
        );
        format!("get_project_root: cwd='{cwd}' normalized='{value}'")
    });
    let root = normalized
        .as_ref()
        .and_then(|path| root_from_path(path))
        .map(|path| path.to_string_lossy().into_owned());
    debug_log(|| {
        let root_value = root.as_deref().unwrap_or("<none>");
        format!("get_project_root: cwd root='{root_value}'")
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
                |info| report_panic("refresh_root_for_buffer", &info),
            )
        })
        .build();

    api::create_autocmd(["BufEnter", "BufWinEnter", "BufFilePost"], &opts)?;
    Ok(())
}

fn apply_config(config: Option<&Dictionary>) {
    let config = ProjectRootConfig::from_dict(config);
    let mut state = state_lock();
    state.root_indicators = config.root_indicators;
}

#[allow(clippy::needless_pass_by_value)]
fn setup(config: Option<Dictionary>) -> Result<()> {
    apply_config(config.as_ref());

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
    let buf = buf.map_or_else(api::get_current_buf, |buf| buf);
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
    .map_or(root, |value| value.to_string_lossy().into_owned());

    notify::info("", &display);
    Ok(())
}

pub fn build_api() -> Dictionary {
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
    api
}
