use super::runtime::{mutate_engine_state, read_engine_state};
use super::{HostBridgeRevision, HostBridgeState};
use core::ffi::CStr;
use nvim_oxi::lua::{
    self, Pushable,
    ffi::{self, LUA_OK, LUA_REGISTRYINDEX},
    macros::cstr,
};
use nvim_oxi::{Array, Function, LuaRef, Object, Result, api};
use thiserror::Error;

const HOST_BRIDGE_REVISION_FUNCTION_NAME: &str = "rs_smear_cursor#host_bridge#revision";
const START_TIMER_ONCE_FUNCTION_NAME: &str = "rs_smear_cursor#host_bridge#start_timer_once";
#[cfg(test)]
const CORE_TIMER_CALLBACK_FUNCTION_NAME: &str = "rs_smear_cursor#host_bridge#on_core_timer";
#[cfg(test)]
const HOST_BRIDGE_SCRIPT: &str = include_str!("../../autoload/rs_smear_cursor/host_bridge.vim");

fn host_bridge_state() -> HostBridgeState {
    read_engine_state(|state| state.shell.host_bridge_state())
}

fn note_host_bridge_verified(revision: HostBridgeRevision) {
    mutate_engine_state(|state| {
        state.shell.note_host_bridge_verified(revision);
    });
}

fn on_key_callback_lua_ref() -> Option<LuaRef> {
    read_engine_state(|state| state.shell.on_key_callback_lua_ref())
}

fn note_on_key_callback_lua_ref(lua_ref: LuaRef) {
    mutate_engine_state(|state| {
        state.shell.note_on_key_callback_lua_ref(lua_ref);
    });
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
    #[error("failed to register vim.on_key namespace argument: {message}")]
    LuaPush { message: String },
    #[error("failed to register smear cursor vim.on_key listener: {message}")]
    VimOnKeyRegistration { message: String },
}

impl From<HostBridgeError> for nvim_oxi::Error {
    fn from(error: HostBridgeError) -> Self {
        nvim_oxi::api::Error::Other(error.to_string()).into()
    }
}

fn with_vim_on_key(callback_lua_ref: Option<LuaRef>, namespace_id: u32) -> HostBridgeResult<()> {
    unsafe {
        lua::with_state(|lstate| {
            ffi::lua_getglobal(lstate, cstr!("vim"));
            ffi::lua_getfield(lstate, -1, cstr!("on_key"));

            if let Some(lua_ref) = callback_lua_ref {
                ffi::lua_rawgeti(lstate, LUA_REGISTRYINDEX, lua_ref);
            } else {
                ffi::lua_pushnil(lstate);
            }

            namespace_id
                .push(lstate)
                .map_err(|err| HostBridgeError::LuaPush {
                    message: err.to_string(),
                })?;

            if ffi::lua_pcall(lstate, 2, 0, 0) != LUA_OK {
                let message = {
                    let ptr = ffi::lua_tostring(lstate, -1);
                    if ptr.is_null() {
                        "missing Lua error message while calling vim.on_key".to_string()
                    } else {
                        CStr::from_ptr(ptr).to_string_lossy().into_owned()
                    }
                };
                ffi::lua_pop(lstate, 1);
                ffi::lua_pop(lstate, 1);
                return Err(HostBridgeError::VimOnKeyRegistration { message });
            }

            ffi::lua_pop(lstate, 1);
            Ok(())
        })
    }
}

fn install_on_key_listener_callback() -> LuaRef {
    if let Some(lua_ref) = on_key_callback_lua_ref() {
        return lua_ref;
    }

    // Surprising: nvim-oxi exposes no first-class `vim.on_key` wrapper, so we
    // cache one Lua registry callback and bind it through the raw Lua state.
    let callback = Function::<(Object, Object), ()>::from_fn(
        |(_key, _typed): (Object, Object)| -> Result<()> {
            crate::events::on_key_listener_event();
            Ok(())
        },
    );
    let lua_ref = callback.lua_ref();
    note_on_key_callback_lua_ref(lua_ref);
    lua_ref
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) struct InstalledHostBridge;

impl InstalledHostBridge {
    pub(super) fn start_timer_once(self, timeout_ms: i64) -> HostBridgeResult<i64> {
        let args = Array::from_iter([Object::from(timeout_ms)]);
        api::call_function(START_TIMER_ONCE_FUNCTION_NAME, args)
            .map_err(|error| HostBridgeError::StartTimerOnce(error.into()))
    }

    pub(super) fn set_on_key_listener(
        self,
        namespace_id: u32,
        enabled: bool,
    ) -> HostBridgeResult<()> {
        let callback_lua_ref = enabled.then(install_on_key_listener_callback);
        with_vim_on_key(callback_lua_ref, namespace_id)
    }
}

fn installed_host_bridge_from_state() -> HostBridgeResult<InstalledHostBridge> {
    match host_bridge_state() {
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
            Ok(host_bridge) => Ok(host_bridge),
            Err(_) => {
                let revision = query_host_bridge_revision()?;
                if revision != HostBridgeRevision::CURRENT {
                    return Err(HostBridgeError::RevisionMismatch {
                        expected: HostBridgeRevision::CURRENT,
                        found: revision,
                    });
                }

                note_host_bridge_verified(revision);
                Ok(InstalledHostBridge)
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

pub(super) fn set_on_key_listener(
    host_bridge: InstalledHostBridge,
    namespace_id: u32,
    enabled: bool,
) -> HostBridgeResult<()> {
    host_bridge.set_on_key_listener(namespace_id, enabled)
}

pub(super) fn ensure_namespace_id() -> u32 {
    if let Some(namespace_id) = read_engine_state(|state| state.shell.namespace_id()) {
        return namespace_id;
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
    }

    #[test]
    fn host_bridge_script_describes_the_versioned_contract() {
        let script = HOST_BRIDGE_SCRIPT;
        assert!(script.contains("versioned host bridge contract"));
    }

    #[test]
    fn host_bridge_script_registers_the_core_timer_callback_name() {
        let script = HOST_BRIDGE_SCRIPT;
        assert!(script.contains(CORE_TIMER_CALLBACK_FUNCTION_NAME));
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
}
