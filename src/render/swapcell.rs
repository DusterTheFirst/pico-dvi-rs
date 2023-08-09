use core::{
    cell::UnsafeCell,
    mem::MaybeUninit,
    sync::atomic::{AtomicU32, Ordering},
};

use cortex_m::asm::{sev, wfe};

/// A cell of storage useful for atomic swaps.
///
/// There are a number of safety considerations. Basically, use by the "client"
/// must be in a single thread, and similarly by the "system".
pub struct SwapCell<T> {
    state: AtomicU32,
    value: UnsafeCell<MaybeUninit<T>>,
}

unsafe impl<T: Sync> Sync for SwapCell<T> {}

const STATE_TAKEN: u32 = 0;
const STATE_READY_FOR_CLIENT: u32 = 1;
const STATE_READY_FOR_SYSTEM: u32 = 2;

impl<T> SwapCell<T> {
    pub const fn new() -> Self {
        SwapCell {
            state: AtomicU32::new(0),
            value: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }

    pub fn set_for_client(&self, val: T) {
        unsafe {
            (*self.value.get()).write(val);
        }
        self.state.store(STATE_READY_FOR_CLIENT, Ordering::Relaxed);
    }

    pub fn set_for_system(&self, val: T) {
        unsafe {
            (*self.value.get()).write(val);
        }
        self.state.store(STATE_READY_FOR_SYSTEM, Ordering::Release);
    }

    pub fn take_blocking(&self) -> T {
        while self.state.load(Ordering::Acquire) != STATE_READY_FOR_CLIENT {
            wfe();
        }
        let val = unsafe { self.value.get().read().assume_init() };
        self.state.store(STATE_TAKEN, Ordering::Relaxed);
        val
    }

    pub fn ready_for_system(&self) -> bool {
        self.state.load(Ordering::Acquire) == STATE_READY_FOR_SYSTEM
    }

    /// Safety: only valid if `ready_for_system()` returns `true`.
    pub fn swap_by_system(&self, slot: &mut T) {
        unsafe {
            core::mem::swap((*self.value.get()).assume_init_mut(), slot);
        }
        self.state.store(STATE_READY_FOR_CLIENT, Ordering::Release);
        sev();
    }
}
