//! Interface between user-space and interrupt code.
//!
//! This interface is fairly closely patterned after the USB hardware in
//! the RP2350 chip, but in a more idiomatic Rust style.

use core::{
    cell::UnsafeCell,
    ops::{Deref, DerefMut},
    sync::atomic::{
        AtomicU32,
        Ordering::{Acquire, Relaxed, Release},
    },
};

use crate::pio_usb::host_task::SetupPacket;

const N_PIPES: usize = 16;

pub struct Iface {
    // Bitmask of requests
    request: AtomicU32,
    // Bitmask of responses
    response: AtomicU32,
    pipes: [UnsafeCell<Pipe>; N_PIPES],
}

static IFACE: Iface = Iface::new();

unsafe impl Sync for Iface {}

/// User-space access to shared interface.
pub struct UserIface {
    int_owned: u32,
}

#[derive(Default)]
pub struct IntIface {}

/// Size of the buffers in the ports.
///
/// A value of 64 is sufficient as long as we don't do isochronous transfers.
/// If and when we do implement those, then we probably want to move to an
/// arrangement closer to the RP2350 chip, where there is a buffer space.
const BUF_SIZE: usize = 64;

pub struct Pipe {
    pub buf: [u8; BUF_SIZE],
    pub addr: u8,
    pub ep: u8,
    // consider changing this to PID_DATA0 or PID_DATA1, rather than 0 or 1 as now
    pub toggle: u8,
    pub len: u16,
    pub req: Request,
    pub status: Status,
}

pub struct PipeGuard {
    pipe_ix: usize,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Request {
    Empty,
    Setup,
    In,
    Out,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Status {
    Empty,
    Success,
    Timeout,
    #[expect(
        unused,
        reason = "TODO: signal this when hub signals port disconnected"
    )]
    Disconnected,
    Error,
}

impl Pipe {
    const fn new() -> Self {
        Self {
            buf: [0; BUF_SIZE],
            addr: 0,
            ep: 0,
            toggle: 0,
            len: 0,
            req: Request::Empty,
            status: Status::Empty,
        }
    }

    /// Prepare setup packet
    pub fn setup(&mut self, setup: SetupPacket) {
        self.buf[0..8].copy_from_slice(&setup.raw);
        self.ep = 0;
        self.toggle = 0;
        self.req = Request::Setup;
    }
}

impl Deref for PipeGuard {
    type Target = Pipe;

    fn deref(&self) -> &Self::Target {
        unsafe { &*IFACE.pipes[self.pipe_ix].get() }
    }
}

impl DerefMut for PipeGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *IFACE.pipes[self.pipe_ix].get() }
    }
}

impl Iface {
    const fn new() -> Iface {
        Iface {
            request: AtomicU32::new(0),
            response: AtomicU32::new(0),
            pipes: [const { UnsafeCell::new(Pipe::new()) }; N_PIPES],
        }
    }
}

impl UserIface {
    /// Create a new instance user-space access to the shared interface.
    ///
    /// Safety: there needs to be only one instance; it's bound to `IFACE`
    pub unsafe fn new() -> UserIface {
        UserIface { int_owned: 0 }
    }

    pub fn pipe(&mut self, pipe_ix: usize) -> PipeGuard {
        assert!(pipe_ix < N_PIPES);
        assert!((self.int_owned >> pipe_ix) & 1 == 0);
        self.int_owned |= 1 << pipe_ix;
        PipeGuard { pipe_ix }
    }

    pub fn send_req(&mut self, pipe: PipeGuard) {
        IFACE.request.fetch_or(1 << pipe.pipe_ix, Release);
        // possible TODO: trigger interrupt
    }

    pub fn poll(&mut self) -> u32 {
        let mut old = IFACE.response.load(Relaxed);
        loop {
            match IFACE
                .response
                .compare_exchange_weak(old, 0, Acquire, Relaxed)
            {
                Ok(_) => break,
                Err(x) => old = x,
            }
        }
        self.int_owned &= !old;
        old
    }
}

impl IntIface {
    #[link_section = ".data"]
    pub fn poll(&mut self) -> u32 {
        let mut old = IFACE.request.load(Relaxed);
        loop {
            match IFACE
                .request
                .compare_exchange_weak(old, 0, Acquire, Relaxed)
            {
                Ok(_) => break,
                Err(x) => old = x,
            }
        }
        old
    }

    /// Safety: pipe_ix must be valid & owned.
    #[link_section = ".data"]
    pub unsafe fn pipe(&mut self, pipe_ix: usize) -> PipeGuard {
        PipeGuard { pipe_ix }
    }

    #[link_section = ".data"]
    pub fn send_response(&mut self, pipe: PipeGuard) {
        IFACE.response.fetch_or(1 << pipe.pipe_ix, Release);
    }
}
