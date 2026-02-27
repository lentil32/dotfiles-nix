use std::sync::{Mutex, MutexGuard};

pub(crate) fn lock_with_poison_recovery<T, Recover, PostRecover, Payload>(
    mutex: &Mutex<T>,
    mut recover: Recover,
    mut post_recover: PostRecover,
) -> MutexGuard<'_, T>
where
    Recover: FnMut(&mut T) -> Payload,
    PostRecover: FnMut(Payload),
{
    loop {
        match mutex.lock() {
            Ok(guard) => return guard,
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                let payload = recover(&mut guard);
                drop(guard);
                mutex.clear_poison();
                post_recover(payload);
            }
        }
    }
}
