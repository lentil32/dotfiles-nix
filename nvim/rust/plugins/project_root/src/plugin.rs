use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use crate::core::RootMatch;
use nvim_oxi::api;
use nvim_oxi::api::Buffer;
use nvim_oxi::api::opts::{CreateAugroupOpts, CreateAutocmdOpts};
use nvim_oxi::api::types::AutocmdCallbackArgs;
use nvim_oxi::{Array, Dictionary, Function, Result, String as NvimString};
use nvim_oxi_utils::{guard, handles, notify, state::StateCell};
use nvim_utils::path::path_is_dir;

use crate::buffer::{
    NormalizedPathKey, cached_root_by_key, debug_log, get_path_from_buffer, normalized_path_key,
    set_buf_root,
};
use crate::config::ProjectRootConfig;
use crate::types::State;

const PROJECT_ROOT_GROUP: &str = "ProjectRoot";
const LOG_CONTEXT: &str = "project_root";

static STATE: LazyLock<StateCell<State>> = LazyLock::new(|| StateCell::new(State::default()));

fn state_lock() -> nvim_oxi_utils::state::StateGuard<'static, State> {
    STATE.lock_recover(|state| {
        notify::warn(
            LOG_CONTEXT,
            "state mutex poisoned; resetting project_root state",
        );
        *state = State::default();
    })
}

fn report_panic(label: &str, info: &guard::PanicInfo) {
    notify::error(LOG_CONTEXT, &format!("{label} panic: {}", info.render()));
}

fn buffer_path_and_key(buf: &Buffer, label: &str) -> Result<Option<(PathBuf, NormalizedPathKey)>> {
    if !buf.is_valid() {
        debug_log(|| format!("{label}: buffer invalid"));
        return Ok(None);
    }

    let Some(path) = get_path_from_buffer(buf)? else {
        debug_log(|| format!("{label}: buf={} no path", buf.handle()));
        set_buf_root(buf, None, None, None)?;
        return Ok(None);
    };
    let key = normalized_path_key(&path);
    Ok(Some((path, key)))
}

fn cached_or_refresh_root(buf: &Buffer) -> Result<Option<String>> {
    let Some((path, key)) = buffer_path_and_key(buf, "cached_or_refresh_root")? else {
        return Ok(None);
    };

    if let Some(root) = cached_root_by_key(buf, &key) {
        return Ok(Some(root));
    }

    refresh_root_for_path(buf, &path, &key)
}

fn root_from_path(path: &Path) -> Option<PathBuf> {
    root_match_from_path(path).map(|value| value.root)
}

fn root_match_from_path(path: &Path) -> Option<RootMatch> {
    let indicators = {
        let state = state_lock();
        state.root_indicators.clone()
    };
    let root = crate::core::root_match_from_path_with(path, &indicators, path_is_dir, Path::exists);
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
                    "root_from_path: path='{}' found='{}' indicator='{}'",
                    path.display(),
                    root.root.display(),
                    root.indicator
                )
            },
        )
    });
    root
}

fn refresh_root_for_path(
    buf: &Buffer,
    path: &Path,
    key: &NormalizedPathKey,
) -> Result<Option<String>> {
    let root_match = root_match_from_path(path);
    let root = root_match
        .as_ref()
        .map(|match_value| match_value.root.to_string_lossy().into_owned());
    let indicator = root_match
        .as_ref()
        .map(|match_value| match_value.indicator.as_str());
    set_buf_root(buf, root.as_deref(), Some(key), indicator)?;
    debug_log(|| {
        let root_value = root.as_deref().unwrap_or("<none>");
        format!(
            "refresh_root_for_path: buf={} path='{}' root='{}'",
            buf.handle(),
            key.as_str(),
            root_value
        )
    });

    Ok(root)
}

fn refresh_root_for_buffer(buf: &Buffer) -> Result<Option<String>> {
    let Some((path, key)) = buffer_path_and_key(buf, "refresh_root_for_buffer")? else {
        return Ok(None);
    };
    refresh_root_for_path(buf, &path, &key)
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
    } else if let Some(alt_buf) = handles::valid_buffer(alt) {
        let root = cached_or_refresh_root(&alt_buf)?;
        let handle = alt_buf.handle();
        debug_log(|| {
            let root_value = root.as_deref().unwrap_or("<none>");
            format!("get_project_root: alternate buf={handle} root='{root_value}'")
        });
        root
    } else {
        debug_log(|| format!("get_project_root: invalid alternate buffer handle (value={alt})"));
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

    api::create_autocmd(["BufEnter", "BufFilePost"], &opts)?;
    Ok(())
}

fn invalidate_cached_roots() {
    for buf in api::list_bufs() {
        if !buf.is_valid() {
            continue;
        }
        if let Err(err) = set_buf_root(&buf, None, None, None) {
            notify::warn(
                LOG_CONTEXT,
                &format!("clear root cache failed for buf {}: {err}", buf.handle()),
            );
        }
    }
}

fn apply_config(config: Option<&Dictionary>) -> bool {
    let config = ProjectRootConfig::from_dict(config);
    let mut state = state_lock();
    if state.root_indicators == config.root_indicators {
        return false;
    }
    state.root_indicators = config.root_indicators;
    true
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "nvim callback signatures pass owned Lua values by value"
)]
fn setup(config: Option<Dictionary>) -> Result<()> {
    let config_changed = apply_config(config.as_ref());
    if config_changed {
        invalidate_cached_roots();
    }

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

    if let Err(err) = refresh_root_for_buffer(&api::get_current_buf()) {
        notify::warn(
            LOG_CONTEXT,
            &format!("initial refresh_root_for_buffer failed: {err}"),
        );
    }
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
