mod font;
mod palette;
mod queue;
mod renderlist;
mod swapcell;

pub use font::FONT_HEIGHT;
pub use palette::BW_PALETTE;

use core::sync::atomic::{compiler_fence, AtomicBool, Ordering};

use rp_pico::{
    hal::{sio::SioFifo, Sio},
    pac,
};

use crate::{
    dvi::{tmds::TmdsPair, VERTICAL_REPEAT},
    scanlist::{Scanlist, ScanlistBuilder},
};

use self::{
    queue::Queue,
    renderlist::{Renderlist, RenderlistBuilder},
    swapcell::SwapCell,
};

pub const N_LINE_BUFS: usize = 4;

pub struct ScanRender {
    display_list: DisplayList,
    stripe_remaining: u32,
    scan_ptr: *const u32,
    scan_next: *const u32,
    assigned: [bool; N_LINE_BUFS],
    fifo: SioFifo,
    render_ptr: *const u32,
    render_y: u32,
}

/// Size of a line buffer in u32 units.
const LINE_BUF_SIZE: usize = 256;

static mut LINE_BUFS: [LineBuf; N_LINE_BUFS] = [LineBuf::zero(); N_LINE_BUFS];
const ATOMIC_FALSE: AtomicBool = AtomicBool::new(false);
static PENDING: [AtomicBool; N_LINE_BUFS] = [ATOMIC_FALSE; N_LINE_BUFS];

/// The maximum number of lines that can be scheduled on core1.
const MAX_CORE1_PENDING: usize = 1;
const LINE_QUEUE_SIZE: usize = (MAX_CORE1_PENDING + 1).next_power_of_two();
pub static CORE1_QUEUE: Queue<LINE_QUEUE_SIZE> = Queue::new();

/// A complete display list.
///
/// This can be seen as an expression that generates the pixels for a frame.
pub struct DisplayList {
    render: Renderlist,
    scan: Scanlist,
}

static DISPLAY_LIST_SWAPCELL: SwapCell<DisplayList> = SwapCell::new();

#[derive(Clone, Copy)]
pub struct LineBuf {
    render_ptr: *const u32,
    /// Y coordinate relative to top of stripe.
    y: u32,
    buf: [u32; LINE_BUF_SIZE],
}

impl LineBuf {
    const fn zero() -> Self {
        let render_ptr = core::ptr::null();
        let y = 0;
        let buf = [0; LINE_BUF_SIZE];
        LineBuf { render_ptr, y, buf }
    }
}

core::arch::global_asm! {
    include_str!("scan.asm"),
    include_str!("render.asm"),
    options(raw)
}

extern "C" {
    fn tmds_scan(
        scan_list: *const u32,
        input: *const u32,
        output: *mut TmdsPair,
        stride: u32,
    ) -> *const u32;

    fn render_engine(render_list: *const u32, output: *mut u32, y: u32);
}

pub fn rgb(r: u8, g: u8, b: u8) -> [TmdsPair; 3] {
    [
        TmdsPair::encode_balanced_approx(b),
        TmdsPair::encode_balanced_approx(g),
        TmdsPair::encode_balanced_approx(r),
    ]
}

impl ScanRender {
    pub fn new() -> Self {
        let stripe_remaining = 0;
        let scan_ptr = core::ptr::null();
        let scan_next = core::ptr::null();
        // Safety: it makes sense for two cores to both have access to the
        // fifo, as it's designed for that purpose. A better PAC API might
        // allow us to express this safely.
        let pac = unsafe { pac::Peripherals::steal() };
        let sio = Sio::new(pac.SIO);
        let fifo = sio.fifo;
        let render_ptr = core::ptr::null();
        let render_y = 0;
        let display_list = DisplayList::new(640, 480 / VERTICAL_REPEAT as u32);
        ScanRender {
            stripe_remaining,
            scan_ptr,
            scan_next,
            assigned: [false; N_LINE_BUFS],
            fifo,
            render_ptr,
            render_y,
            display_list,
        }
    }

    #[link_section = ".data"]
    #[inline(never)]
    pub fn render_scanline(&mut self, tmds_buf: &mut [TmdsPair], y: u32, available: bool) {
        unsafe {
            if y == 0 {
                self.scan_next = self.display_list.scan.get().as_ptr();
            }
            if self.stripe_remaining == 0 {
                self.stripe_remaining = self.scan_next.read();
                self.scan_ptr = self.scan_next.add(1);
                // TODO: set desperate scan_next
            }
            if available {
                let line_ix = y as usize % N_LINE_BUFS;
                let line_buf_ptr = LINE_BUFS[line_ix].buf.as_ptr();
                self.scan_next =
                    tmds_scan(self.scan_ptr, line_buf_ptr, tmds_buf.as_mut_ptr(), 1280);
            }
            self.stripe_remaining -= 1;
        }
    }

    #[link_section = ".data"]
    pub fn is_line_available(&self, y: u32) -> bool {
        let line_ix = y as usize % N_LINE_BUFS;
        self.assigned[line_ix] && !PENDING[line_ix].load(Ordering::Relaxed)
    }

    #[link_section = ".data"]
    pub fn schedule_line_render(&mut self, y: u32) {
        if y == 0 {
            // The swap is scheduled on scanline 0, but we'd want to move this
            // earlier if we wanted to do more stuff like palettes.
            if DISPLAY_LIST_SWAPCELL.ready_for_system() {
                DISPLAY_LIST_SWAPCELL.swap_by_system(&mut self.display_list);
            }

            self.render_ptr = self.display_list.render.get().as_ptr();
            self.render_y = 0;
        }
        let line_ix = y as usize % N_LINE_BUFS;
        if PENDING[line_ix].load(Ordering::Relaxed) {
            self.assigned[line_ix] = false;
            return;
        }
        let render_ptr = self.render_ptr;
        // TODO: set ptr, y in linebuf
        // Safety: we currently own access to the line buffer.
        let line_buf = unsafe { &mut LINE_BUFS[line_ix as usize] };
        line_buf.render_ptr = unsafe { render_ptr.add(2) };
        line_buf.y = self.render_y;
        self.render_y += 1;
        let stripe_height = unsafe { render_ptr.read() };
        if self.render_y == stripe_height {
            let jump = unsafe { render_ptr.add(1).read() as usize };
            self.render_ptr = unsafe { render_ptr.add(jump) };
            self.render_y = 0;
        }
        if CORE1_QUEUE.len() < MAX_CORE1_PENDING {
            // schedule on core1
            PENDING[line_ix].store(true, Ordering::Relaxed);
            CORE1_QUEUE.push_unchecked(line_ix as u32);
            self.assigned[line_ix] = true;
        } else {
            // try to schedule on core0
            if self.fifo.is_write_ready() {
                PENDING[line_ix].store(true, Ordering::Relaxed);
                // Writes to channels are generally considered to be release,
                // but the implementation in rp2040-hal lacks such a fence, so
                // we include it explicitly.
                compiler_fence(Ordering::Release);
                self.fifo.write(line_ix as u32);
                self.assigned[line_ix] = true;
            } else {
                self.assigned[line_ix] = false;
            }
        }
    }
}

/// Entry point for rendering a line.
///
/// This can be called by either core.
#[link_section = ".data"]
pub unsafe fn render_line(line_ix: u32) {
    let line_buf = &mut LINE_BUFS[line_ix as usize];
    render_engine(line_buf.render_ptr, line_buf.buf.as_mut_ptr(), line_buf.y);
    PENDING[line_ix as usize].store(false, Ordering::Release);
}

impl DisplayList {
    pub fn new(width: u32, height: u32) -> Self {
        let mut rb = RenderlistBuilder::new();
        let mut sb = ScanlistBuilder::new(width, height);
        rb.begin_stripe(height);
        rb.end_stripe();
        sb.begin_stripe(height);
        sb.solid(width, rgb(0, 0, 0));
        sb.end_stripe();
        let render = rb.build();
        let scan = sb.build();
        DisplayList { render, scan }
    }
}

/// The system assumes that this is called before [`start_display_list`]
pub fn init_display_swapcell() {
    // The display list doesn't have to be usable.
    DISPLAY_LIST_SWAPCELL.set_for_client(DisplayList::new(0, 0));
}

/// Start building a display list. This blocks until a free display
/// list is available.
///
/// The user must have called [`init_display_swapcell`] before calling this.
pub fn start_display_list() -> (RenderlistBuilder, ScanlistBuilder) {
    let display_list = DISPLAY_LIST_SWAPCELL.take_blocking();
    let rb = RenderlistBuilder::recycle(display_list.render);
    let sb = ScanlistBuilder::recycle(display_list.scan);
    (rb, sb)
}

pub fn end_display_list(rb: RenderlistBuilder, sb: ScanlistBuilder) {
    let render = rb.build();
    let scan = sb.build();
    let display_list = DisplayList { render, scan };
    DISPLAY_LIST_SWAPCELL.set_for_system(display_list);
}
