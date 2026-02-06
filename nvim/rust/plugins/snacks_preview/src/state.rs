use std::collections::HashMap;
use std::sync::LazyLock;

use crate::core::{BufKey, PreviewEvent, PreviewRegistry, PreviewToken, PreviewTransition};
use nvim_oxi::mlua;
use nvim_oxi_utils::handles::BufHandle;
use nvim_oxi_utils::notify;
use nvim_oxi_utils::state::{StateCell, StateGuard};

use crate::LOG_CONTEXT;

#[derive(Default)]
pub struct State {
    pub registry: PreviewRegistry,
    next_cleanup_id: i64,
    cleanups: HashMap<i64, mlua::RegistryKey>,
}

impl State {
    const fn next_cleanup_id(&mut self) -> i64 {
        self.next_cleanup_id = if self.next_cleanup_id == i64::MAX {
            1
        } else {
            self.next_cleanup_id + 1
        };
        self.next_cleanup_id
    }

    fn insert_cleanup(&mut self, cleanup_key: mlua::RegistryKey) -> i64 {
        let cleanup_id = self.next_cleanup_id();
        let replaced = self.cleanups.insert(cleanup_id, cleanup_key);
        debug_assert!(replaced.is_none());
        cleanup_id
    }

    fn take_cleanup(&mut self, cleanup_id: i64) -> Option<mlua::RegistryKey> {
        self.cleanups.remove(&cleanup_id)
    }

    fn take_all_cleanups(&mut self) -> Vec<mlua::RegistryKey> {
        self.cleanups.drain().map(|(_, key)| key).collect()
    }
}

static STATE: LazyLock<StateCell<State>> = LazyLock::new(|| StateCell::new(State::default()));

pub fn state_lock() -> StateGuard<'static, State> {
    let mut guard = STATE.lock();
    if guard.poisoned() {
        let dropped_cleanups = guard.cleanups.len();
        notify::warn(
            LOG_CONTEXT,
            &format!(
                "state mutex poisoned; resetting preview registry (dropping {dropped_cleanups} pending cleanups)"
            ),
        );
        *guard = State::default();
        STATE.clear_poison();
    }
    guard
}

pub const fn buf_key(buf_handle: BufHandle) -> Option<BufKey> {
    BufKey::try_new(buf_handle.raw())
}

pub fn is_current_preview_token(key: BufKey, token: PreviewToken) -> bool {
    let state = state_lock();
    state.registry.is_token_current(key, token)
}

pub fn apply_event(event: PreviewEvent) -> PreviewTransition {
    let mut state = state_lock();
    state.registry.reduce(event)
}

pub fn register_cleanup_key(cleanup_key: mlua::RegistryKey) -> i64 {
    let mut state = state_lock();
    state.insert_cleanup(cleanup_key)
}

pub fn take_cleanup_key(cleanup_id: i64) -> Option<mlua::RegistryKey> {
    if cleanup_id <= 0 {
        return None;
    }
    let mut state = state_lock();
    state.take_cleanup(cleanup_id)
}

pub fn take_all_cleanup_keys_and_reset() -> Vec<mlua::RegistryKey> {
    let mut state = state_lock();
    let cleanup_keys = state.take_all_cleanups();
    state.registry = PreviewRegistry::default();
    state.next_cleanup_id = 0;
    cleanup_keys
}
