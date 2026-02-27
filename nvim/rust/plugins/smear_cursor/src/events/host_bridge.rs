use super::runtime::engine_lock;
use super::{HostBridgeRevision, HostBridgeState};
use core::ffi::CStr;
use nvim_oxi::lua::{
    self, Pushable,
    ffi::{self, LUA_OK, LUA_REGISTRYINDEX},
    macros::cstr,
};
use nvim_oxi::{Array, Function, LuaRef, Object, Result, api};

const HOST_BRIDGE_REVISION_FUNCTION_NAME: &str = "rs_smear_cursor#host_bridge#revision";
const START_TIMER_ONCE_FUNCTION_NAME: &str = "rs_smear_cursor#host_bridge#start_timer_once";
#[cfg(test)]
const CORE_TIMER_CALLBACK_FUNCTION_NAME: &str = "rs_smear_cursor#host_bridge#on_core_timer";
#[cfg(test)]
const HOST_BRIDGE_SCRIPT: &str = include_str!("../../autoload/rs_smear_cursor/host_bridge.vim");

fn host_bridge_state() -> HostBridgeState {
    let state = engine_lock();
    state.shell.host_bridge_state()
}

fn note_host_bridge_verified(revision: HostBridgeRevision) {
    let mut state = engine_lock();
    state.shell.note_host_bridge_verified(revision);
}

fn on_key_callback_lua_ref() -> Option<LuaRef> {
    let state = engine_lock();
    state.shell.on_key_callback_lua_ref()
}

fn note_on_key_callback_lua_ref(lua_ref: LuaRef) {
    let mut state = engine_lock();
    state.shell.note_on_key_callback_lua_ref(lua_ref);
}

fn host_bridge_error(message: impl Into<String>) -> nvim_oxi::Error {
    nvim_oxi::api::Error::Other(message.into()).into()
}

fn with_vim_on_key(callback_lua_ref: Option<LuaRef>, namespace_id: u32) -> Result<()> {
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
                .map_err(|err| host_bridge_error(err.to_string()))?;

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
                return Err(host_bridge_error(format!(
                    "failed to register smear cursor vim.on_key listener: {message}"
                )));
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
pub(super) struct InstalledHostBridge(HostBridgeRevision);

impl InstalledHostBridge {
    pub(super) fn start_timer_once(self, timeout_ms: i64) -> Result<i64> {
        let args = Array::from_iter([Object::from(timeout_ms)]);
        Ok(api::call_function(START_TIMER_ONCE_FUNCTION_NAME, args)?)
    }

    pub(super) fn set_on_key_listener(self, namespace_id: u32, enabled: bool) -> Result<()> {
        let callback_lua_ref = enabled.then(install_on_key_listener_callback);
        with_vim_on_key(callback_lua_ref, namespace_id)
    }
}

fn installed_host_bridge_from_state() -> Result<InstalledHostBridge> {
    match host_bridge_state() {
        HostBridgeState::Verified { revision } if revision == HostBridgeRevision::CURRENT => {
            Ok(InstalledHostBridge(revision))
        }
        HostBridgeState::Verified { revision } => Err(nvim_oxi::api::Error::Other(format!(
            "smear cursor host bridge revision mismatch: expected v{}, found v{}",
            HostBridgeRevision::CURRENT.get(),
            revision.get(),
        ))
        .into()),
        HostBridgeState::Unverified => Err(nvim_oxi::api::Error::Other(
            "smear cursor host bridge is not verified; call setup() before scheduling host effects"
                .to_string(),
        )
        .into()),
    }
}

fn query_host_bridge_revision() -> Result<HostBridgeRevision> {
    let revision: i64 = api::call_function(HOST_BRIDGE_REVISION_FUNCTION_NAME, Array::new())?;
    let revision = u32::try_from(revision).map_err(nvim_oxi::api::Error::from)?;
    Ok(HostBridgeRevision(revision))
}

struct HostBridge;

impl HostBridge {
    fn verify() -> Result<InstalledHostBridge> {
        match installed_host_bridge_from_state() {
            Ok(host_bridge) => Ok(host_bridge),
            Err(_) => {
                let revision = query_host_bridge_revision()?;
                if revision != HostBridgeRevision::CURRENT {
                    return Err(nvim_oxi::api::Error::Other(format!(
                        "smear cursor host bridge revision mismatch: expected v{}, found v{}",
                        HostBridgeRevision::CURRENT.get(),
                        revision.get(),
                    ))
                    .into());
                }

                note_host_bridge_verified(revision);
                Ok(InstalledHostBridge(revision))
            }
        }
    }
}

pub(super) fn verify_host_bridge() -> Result<InstalledHostBridge> {
    HostBridge::verify()
}

pub(super) fn installed_host_bridge() -> Result<InstalledHostBridge> {
    installed_host_bridge_from_state()
}

pub(super) fn set_on_key_listener(
    host_bridge: InstalledHostBridge,
    namespace_id: u32,
    enabled: bool,
) -> Result<()> {
    host_bridge.set_on_key_listener(namespace_id, enabled)
}

pub(super) fn ensure_namespace_id() -> u32 {
    if let Some(namespace_id) = {
        let state = engine_lock();
        state.shell.namespace_id()
    } {
        return namespace_id;
    }

    let created = api::create_namespace("rs_smear_cursor");
    let mut state = engine_lock();
    if let Some(existing) = state.shell.namespace_id() {
        existing
    } else {
        state.shell.set_namespace_id(created);
        created
    }
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
    fn host_bridge_script_routes_through_named_vimscript_callbacks() {
        let script = HOST_BRIDGE_SCRIPT;
        assert!(script.contains("versioned host bridge contract"));
        assert!(script.contains(CORE_TIMER_CALLBACK_FUNCTION_NAME));
        assert!(script.contains("timer_start("));
        assert!(script.contains("function('rs_smear_cursor#host_bridge#on_core_timer')"));
        assert!(!script.contains("vim.on_key("));
        assert!(!script.contains("set_on_key_listener"));
        assert!(!script.contains("v:lua."));
        assert!(!script.contains("_G.__rs_smear_cursor"));
    }
}
