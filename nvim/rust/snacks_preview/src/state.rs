use std::sync::LazyLock;

use nvim_oxi_utils::handles::BufHandle;
use nvim_oxi_utils::notify;
use nvim_oxi_utils::state::{StateCell, StateGuard};
use snacks_preview_core::{BufKey, PreviewRegistry};

use crate::LOG_CONTEXT;

#[derive(Default)]
pub struct State {
    pub registry: PreviewRegistry,
}

static STATE: LazyLock<StateCell<State>> = LazyLock::new(|| StateCell::new(State::default()));

pub fn state_lock() -> StateGuard<'static, State> {
    let guard = STATE.lock();
    if guard.poisoned() {
        notify::warn(LOG_CONTEXT, "state mutex poisoned; continuing");
    }
    guard
}

pub const fn buf_key(buf_handle: BufHandle) -> Option<BufKey> {
    BufKey::try_new(buf_handle.raw())
}

pub fn state_ok(key: BufKey, token: i64) -> bool {
    let state = state_lock();
    state.registry.is_token_current(key, token)
}
