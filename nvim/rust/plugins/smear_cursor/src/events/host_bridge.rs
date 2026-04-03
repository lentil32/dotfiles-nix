use super::runtime::{mutate_engine_state, read_engine_state};
use super::{EngineAccessError, HostBridgeRevision, HostBridgeState};
use crate::core::types::Generation;
use nvim_oxi::{Array, Object, api};
use thiserror::Error;

const HOST_BRIDGE_REVISION_FUNCTION_NAME: &str = "rs_smear_cursor#host_bridge#revision";
const START_TIMER_ONCE_FUNCTION_NAME: &str = "rs_smear_cursor#host_bridge#start_timer_once";
const INSTALL_PROBE_HELPERS_FUNCTION_NAME: &str =
    "rs_smear_cursor#host_bridge#install_probe_helpers";
const LUAEVAL_FUNCTION_NAME: &str = "luaeval";
const CURSOR_COLOR_AT_CURSOR_LUAEVAL_EXPR: &str = concat!(
    "(package.loaded['rs_smear_cursor.probes'] or require('rs_smear_cursor.probes'))",
    ".cursor_color_at_cursor(_A)"
);
#[cfg(test)]
const CURSOR_COLOR_AT_CURSOR_FUNCTION_NAME: &str =
    "rs_smear_cursor#host_bridge#cursor_color_at_cursor";
const BACKGROUND_ALLOWED_MASK_FUNCTION_NAME: &str =
    "rs_smear_cursor#host_bridge#background_allowed_mask";
#[cfg(test)]
const CORE_TIMER_CALLBACK_FUNCTION_NAME: &str = "rs_smear_cursor#host_bridge#on_core_timer";
#[cfg(test)]
const HOST_BRIDGE_SCRIPT: &str = include_str!("../../autoload/rs_smear_cursor/host_bridge.vim");
#[cfg(test)]
const PROBE_HELPERS_SCRIPT: &str = include_str!("../../lua/rs_smear_cursor/probes.lua");

fn host_bridge_state() -> Result<HostBridgeState, EngineAccessError> {
    read_engine_state(|state| state.shell.host_bridge_state())
}

fn note_host_bridge_verified(revision: HostBridgeRevision) -> Result<(), EngineAccessError> {
    mutate_engine_state(|state| {
        state.shell.note_host_bridge_verified(revision);
    })
}

type HostBridgeResult<T> = std::result::Result<T, HostBridgeError>;

#[derive(Debug, Error)]
pub(super) enum HostBridgeError {
    #[error(
        "smear cursor host bridge revision mismatch: expected v{expected}, found v{found}",
        expected = .expected.get(),
        found = .found.get()
    )]
    RevisionMismatch {
        expected: HostBridgeRevision,
        found: HostBridgeRevision,
    },
    #[error(
        "smear cursor host bridge is not verified; call setup() before scheduling host effects"
    )]
    Unverified,
    #[error("failed to query smear cursor host bridge revision: {0}")]
    RevisionQuery(#[source] nvim_oxi::Error),
    #[error("smear cursor host bridge returned invalid revision: {value}")]
    RevisionDecode { value: i64 },
    #[error("failed to start smear cursor host bridge timer: {0}")]
    StartTimerOnce(#[source] nvim_oxi::Error),
    #[error("failed to install smear cursor probe helpers: {0}")]
    InstallProbeHelpers(#[source] nvim_oxi::Error),
    #[error("failed to call smear cursor cursor-color probe: {0}")]
    CursorColorProbe(#[source] nvim_oxi::Error),
    #[error("failed to encode smear cursor colorscheme generation for host bridge: {value}")]
    CursorColorGenerationEncode { value: u64 },
    #[error("failed to call smear cursor background-mask probe: {0}")]
    BackgroundAllowedMask(#[source] nvim_oxi::Error),
    #[error("engine state access failed while resolving host bridge state: {0}")]
    EngineAccess(#[from] EngineAccessError),
}

impl From<HostBridgeError> for nvim_oxi::Error {
    fn from(error: HostBridgeError) -> Self {
        nvim_oxi::api::Error::Other(error.to_string()).into()
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) struct InstalledHostBridge;

impl InstalledHostBridge {
    pub(super) fn start_timer_once(self, timeout_ms: i64) -> HostBridgeResult<i64> {
        let args = Array::from_iter([Object::from(timeout_ms)]);
        api::call_function(START_TIMER_ONCE_FUNCTION_NAME, args)
            .map_err(|error| HostBridgeError::StartTimerOnce(error.into()))
    }

    pub(super) fn install_probe_helpers(self) -> HostBridgeResult<()> {
        let _: i64 = api::call_function(INSTALL_PROBE_HELPERS_FUNCTION_NAME, Array::new())
            .map_err(|error| HostBridgeError::InstallProbeHelpers(error.into()))?;
        Ok(())
    }

    pub(super) fn cursor_color_at_cursor(
        self,
        colorscheme_generation: Generation,
    ) -> HostBridgeResult<Object> {
        let generation = i64::try_from(colorscheme_generation.value()).map_err(|_| {
            HostBridgeError::CursorColorGenerationEncode {
                value: colorscheme_generation.value(),
            }
        })?;
        let args = Array::from_iter([
            Object::from(CURSOR_COLOR_AT_CURSOR_LUAEVAL_EXPR),
            Object::from(generation),
        ]);
        api::call_function(LUAEVAL_FUNCTION_NAME, args)
            .map_err(|error| HostBridgeError::CursorColorProbe(error.into()))
    }

    pub(super) fn background_allowed_mask(self, request: Array) -> HostBridgeResult<Object> {
        let args = Array::from_iter([Object::from(request)]);
        api::call_function(BACKGROUND_ALLOWED_MASK_FUNCTION_NAME, args)
            .map_err(|error| HostBridgeError::BackgroundAllowedMask(error.into()))
    }
}

fn installed_host_bridge_from_state() -> HostBridgeResult<InstalledHostBridge> {
    match host_bridge_state()? {
        HostBridgeState::Verified { revision } if revision == HostBridgeRevision::CURRENT => {
            Ok(InstalledHostBridge)
        }
        HostBridgeState::Verified { revision } => Err(HostBridgeError::RevisionMismatch {
            expected: HostBridgeRevision::CURRENT,
            found: revision,
        }),
        HostBridgeState::Unverified => Err(HostBridgeError::Unverified),
    }
}

fn query_host_bridge_revision() -> HostBridgeResult<HostBridgeRevision> {
    let revision: i64 = api::call_function(HOST_BRIDGE_REVISION_FUNCTION_NAME, Array::new())
        .map_err(|error| HostBridgeError::RevisionQuery(error.into()))?;
    let revision =
        u32::try_from(revision).map_err(|_| HostBridgeError::RevisionDecode { value: revision })?;
    Ok(HostBridgeRevision(revision))
}

struct HostBridge;

impl HostBridge {
    fn verify() -> HostBridgeResult<InstalledHostBridge> {
        match installed_host_bridge_from_state() {
            Ok(host_bridge) => {
                host_bridge.install_probe_helpers()?;
                Ok(host_bridge)
            }
            Err(_) => {
                let revision = query_host_bridge_revision()?;
                if revision != HostBridgeRevision::CURRENT {
                    return Err(HostBridgeError::RevisionMismatch {
                        expected: HostBridgeRevision::CURRENT,
                        found: revision,
                    });
                }

                let host_bridge = InstalledHostBridge;
                host_bridge.install_probe_helpers()?;
                note_host_bridge_verified(revision)?;
                Ok(host_bridge)
            }
        }
    }
}

pub(super) fn verify_host_bridge() -> HostBridgeResult<InstalledHostBridge> {
    HostBridge::verify()
}

pub(super) fn installed_host_bridge() -> HostBridgeResult<InstalledHostBridge> {
    installed_host_bridge_from_state()
}

pub(super) fn ensure_namespace_id() -> Result<u32, EngineAccessError> {
    if let Some(namespace_id) = read_engine_state(|state| state.shell.namespace_id())? {
        return Ok(namespace_id);
    }

    let created = api::create_namespace("rs_smear_cursor");
    mutate_engine_state(|state| {
        state.shell.namespace_id().unwrap_or_else(|| {
            state.shell.set_namespace_id(created);
            created
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_bridge_script_registers_named_entrypoints() {
        let script = HOST_BRIDGE_SCRIPT;
        assert!(script.contains(HOST_BRIDGE_REVISION_FUNCTION_NAME));
        assert!(script.contains(CORE_TIMER_CALLBACK_FUNCTION_NAME));
        assert!(script.contains(START_TIMER_ONCE_FUNCTION_NAME));
        assert!(script.contains(INSTALL_PROBE_HELPERS_FUNCTION_NAME));
        assert!(script.contains(CURSOR_COLOR_AT_CURSOR_FUNCTION_NAME));
        assert!(script.contains(BACKGROUND_ALLOWED_MASK_FUNCTION_NAME));
    }

    #[test]
    fn host_bridge_script_uses_named_vimscript_timer_callbacks() {
        let script = HOST_BRIDGE_SCRIPT;
        assert!(script.contains("timer_start("));
        assert!(script.contains("function('rs_smear_cursor#host_bridge#on_core_timer')"));
    }

    #[test]
    fn host_bridge_script_avoids_legacy_on_key_listener_hooks() {
        let script = HOST_BRIDGE_SCRIPT;
        assert!(!script.contains("vim.on_key("));
        assert!(!script.contains("set_on_key_listener"));
    }

    #[test]
    fn host_bridge_script_avoids_global_lua_callback_state() {
        let script = HOST_BRIDGE_SCRIPT;
        assert!(!script.contains("v:lua."));
        assert!(!script.contains("_G.__rs_smear_cursor"));
    }

    #[test]
    fn host_bridge_script_loads_probe_helpers_from_runtime_module() {
        let script = HOST_BRIDGE_SCRIPT;
        assert!(script.contains("require('rs_smear_cursor.probes')"));
        assert!(!script.contains("CURSOR_COLOR_LUAEVAL_EXPR"));
        assert!(!script.contains("BACKGROUND_ALLOWED_MASK_LUAEVAL_EXPR"));
    }

    #[test]
    fn host_bridge_script_passes_colorscheme_generation_to_cursor_probe() {
        let script = HOST_BRIDGE_SCRIPT;
        assert!(script.contains("cursor_color_at_cursor(colorscheme_generation) abort"));
        assert!(script.contains(".cursor_color_at_cursor(_A)"));
    }

    #[test]
    fn cursor_color_probe_luaeval_expr_loads_probe_helpers_from_runtime_module() {
        let expr = CURSOR_COLOR_AT_CURSOR_LUAEVAL_EXPR;
        assert!(expr.contains("require('rs_smear_cursor.probes')"));
        assert!(expr.contains(".cursor_color_at_cursor(_A)"));
    }

    #[test]
    fn probe_helpers_cache_highlight_colors_by_generation() {
        let script = PROBE_HELPERS_SCRIPT;
        assert!(script.contains("hl_color_cache_generation"));
        assert!(script.contains("reset_hl_color_cache(colorscheme_generation)"));
        assert!(script.contains("hl_fg_cache[group]"));
    }
}
