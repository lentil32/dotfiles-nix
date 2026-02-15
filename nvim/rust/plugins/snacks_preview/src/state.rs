use std::collections::HashMap;
use std::sync::LazyLock;

use crate::reducer::{BufKey, PreviewEvent, PreviewRegistry, PreviewToken, PreviewTransition};
use nvim_oxi::mlua;
use nvim_oxi_utils::handles::BufHandle;
use nvim_oxi_utils::notify;
use nvim_oxi_utils::state::{StateCell, StateGuard};

use crate::LOG_CONTEXT;

#[derive(Debug, Default)]
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

#[derive(Debug)]
pub struct PreviewContext {
    state: StateCell<State>,
}

impl PreviewContext {
    fn new() -> Self {
        Self {
            state: StateCell::new(State::default()),
        }
    }

    fn state_lock(&self) -> StateGuard<'_, State> {
        self.state.lock_recover(|state| {
            let dropped_cleanups = state.cleanups.len();
            notify::warn(
                LOG_CONTEXT,
                &format!(
                    "state mutex poisoned; resetting preview registry (dropping {dropped_cleanups} pending cleanups)"
                ),
            );
            *state = State::default();
        })
    }

    pub fn is_current_preview_token(&self, key: BufKey, token: PreviewToken) -> bool {
        let state = self.state_lock();
        state.registry.is_token_current(key, token)
    }

    pub fn apply_event(&self, event: PreviewEvent) -> PreviewTransition {
        let mut state = self.state_lock();
        state.registry.reduce(event)
    }

    pub fn register_cleanup_key(&self, cleanup_key: mlua::RegistryKey) -> i64 {
        let mut state = self.state_lock();
        state.insert_cleanup(cleanup_key)
    }

    pub fn take_cleanup_key(&self, cleanup_id: i64) -> Option<mlua::RegistryKey> {
        if cleanup_id <= 0 {
            return None;
        }
        let mut state = self.state_lock();
        state.take_cleanup(cleanup_id)
    }

    pub fn take_all_cleanup_keys_and_reset(&self) -> Vec<mlua::RegistryKey> {
        let mut state = self.state_lock();
        let cleanup_keys = state.take_all_cleanups();
        state.registry = PreviewRegistry::default();
        state.next_cleanup_id = 0;
        cleanup_keys
    }
}

static CONTEXT: LazyLock<PreviewContext> = LazyLock::new(PreviewContext::new);

pub fn context() -> &'static PreviewContext {
    &CONTEXT
}

pub const fn buf_key(buf_handle: BufHandle) -> Option<BufKey> {
    BufKey::try_new(buf_handle.raw())
}
