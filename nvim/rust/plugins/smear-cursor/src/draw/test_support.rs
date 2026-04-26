use super::context::clear_draw_context_for_test;
use crate::mutex::lock_with_poison_recovery;
use std::sync::LazyLock;
use std::sync::Mutex;

static DRAW_TEST_MUTEX: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

struct DrawContextResetGuard;

impl Drop for DrawContextResetGuard {
    fn drop(&mut self) {
        clear_draw_context_for_test();
    }
}

pub(super) fn with_isolated_draw_context<T>(test: impl FnOnce() -> T) -> T {
    let _guard = lock_with_poison_recovery(&DRAW_TEST_MUTEX, |_| (), |_| {});
    clear_draw_context_for_test();
    let _reset = DrawContextResetGuard;
    test()
}
