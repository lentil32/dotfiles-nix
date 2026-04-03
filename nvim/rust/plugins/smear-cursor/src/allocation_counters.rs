use std::alloc::GlobalAlloc;
use std::alloc::Layout;
use std::alloc::System;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct AllocationCountersSnapshot {
    pub(crate) allocation_ops: u64,
    pub(crate) allocation_bytes: u64,
}

pub(crate) struct CountingAllocator;

static ALLOCATION_COUNTING_ENABLED: AtomicBool = AtomicBool::new(false);
static ALLOCATION_OPS: AtomicU64 = AtomicU64::new(0);
static ALLOCATION_BYTES: AtomicU64 = AtomicU64::new(0);

impl CountingAllocator {
    fn record_allocation(size: usize) {
        if !ALLOCATION_COUNTING_ENABLED.load(Ordering::Relaxed) {
            return;
        }

        ALLOCATION_OPS.fetch_add(1, Ordering::Relaxed);
        ALLOCATION_BYTES.fetch_add(size_to_u64(size), Ordering::Relaxed);
    }
}

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            Self::record_allocation(layout.size());
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            Self::record_allocation(layout.size());
        }
        ptr
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = unsafe { System.realloc(ptr, layout, new_size) };
        if !new_ptr.is_null() {
            Self::record_allocation(new_size);
        }
        new_ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
    }
}

struct CountingStateGuard {
    was_enabled: bool,
}

impl Drop for CountingStateGuard {
    fn drop(&mut self) {
        ALLOCATION_COUNTING_ENABLED.store(self.was_enabled, Ordering::Relaxed);
    }
}

pub(crate) fn configure_from_env() {
    let enabled = std::env::var("SMEAR_ENABLE_ALLOCATION_COUNTERS")
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE"))
        .unwrap_or(false);
    reset();
    ALLOCATION_COUNTING_ENABLED.store(enabled, Ordering::Relaxed);
}

pub(crate) fn snapshot() -> AllocationCountersSnapshot {
    AllocationCountersSnapshot {
        allocation_ops: ALLOCATION_OPS.load(Ordering::Relaxed),
        allocation_bytes: ALLOCATION_BYTES.load(Ordering::Relaxed),
    }
}

pub(crate) fn with_counting_suspended<T>(callback: impl FnOnce() -> T) -> T {
    let guard = CountingStateGuard {
        was_enabled: ALLOCATION_COUNTING_ENABLED.swap(false, Ordering::Relaxed),
    };
    let result = callback();
    drop(guard);
    result
}

fn reset() {
    ALLOCATION_OPS.store(0, Ordering::Relaxed);
    ALLOCATION_BYTES.store(0, Ordering::Relaxed);
}

fn size_to_u64(size: usize) -> u64 {
    u64::try_from(size).unwrap_or(u64::MAX)
}

#[cfg(test)]
pub(crate) fn reset_for_test() {
    reset();
}

#[cfg(test)]
pub(crate) fn set_enabled_for_test(enabled: bool) {
    ALLOCATION_COUNTING_ENABLED.store(enabled, Ordering::Relaxed);
}
