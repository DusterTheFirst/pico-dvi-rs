use core::sync::atomic::{AtomicU32, Ordering};

use cortex_m::asm::wfe;

/// A simple SPSC queue of u32 values.
///
/// The implementation is specialized to the use case of scheduling line
/// renders on the same core as the interrupt.
pub struct Queue<const SIZE: usize> {
    rd_ix: AtomicU32,
    wr_ix: AtomicU32,
    buf: [AtomicU32; SIZE],
}

impl<const SIZE: usize> Queue<SIZE> {
    pub const fn new() -> Self {
        const ZERO: AtomicU32 = AtomicU32::new(0);
        Queue {
            rd_ix: ZERO,
            wr_ix: ZERO,
            buf: [ZERO; SIZE],
        }
    }

    /// Push a value, assuming queue is not full.
    pub fn push_unchecked(&self, val: u32) {
        let wr_ix = self.wr_ix.load(Ordering::Relaxed);
        let next = (wr_ix + 1) % SIZE as u32;
        self.buf[wr_ix as usize].store(val, Ordering::Relaxed);
        self.wr_ix.store(next, Ordering::Release);
    }

    /// Return next value to become available.
    ///
    /// Waits while queue is empty. Does not remove the value.
    ///
    /// The waiting is implemented with the `wfe` instruction, which depends
    /// on an event being signaled. This should be reliable when the queue is
    /// pushed from an interrupt handler on the same core.
    pub fn peek_blocking(&self) -> u32 {
        let rd_ix = self.rd_ix.load(Ordering::Relaxed);
        while rd_ix == self.wr_ix.load(Ordering::Acquire) {
            wfe();
        }
        self.buf[rd_ix as usize].load(Ordering::Relaxed)
    }

    /// Remove an item from the queue.
    ///
    /// Only valid if the queue is not empty, otherwise unexpected
    /// results can occur.
    pub fn remove(&self) {
        let rd_ix = self.rd_ix.load(Ordering::Relaxed);
        let next = (rd_ix + 1) % SIZE as u32;
        self.rd_ix.store(next, Ordering::Release);
    }

    /// Take an item, blocking until available.
    pub fn take_blocking(&self) -> u32 {
        let rd_ix = self.rd_ix.load(Ordering::Relaxed);
        while rd_ix == self.wr_ix.load(Ordering::Acquire) {
            wfe();
        }
        let item = self.buf[rd_ix as usize].load(Ordering::Relaxed);
        let next = (rd_ix + 1) % SIZE as u32;
        self.rd_ix.store(next, Ordering::Release);
        item
    }

    pub fn len(&self) -> usize {
        let rd_ix = self.rd_ix.load(Ordering::Acquire);
        let wr_ix = self.wr_ix.load(Ordering::Relaxed);
        (wr_ix as usize + SIZE - rd_ix as usize) % SIZE
    }
}
