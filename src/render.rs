mod font;
mod palette;
mod queue;
mod renderlist;
mod swapcell;

pub use font::FONT_HEIGHT;

pub use palette::{Palette1bpp, Palette4bppFast, BW_PALETTE_1BPP};

pub use queue::Queue;

use crate::dvi::BPP;

use crate::{
    dvi::VERTICAL_REPEAT,
    scanlist::{Scanlist, ScanlistBuilder},
};

use self::{
    renderlist::{Renderlist, RenderlistBuilder},
    swapcell::SwapCell,
};

pub struct ScanRender {
    display_list: DisplayList,
    stripe_remaining: u32,
    scan_ptr: *const u32,
    scan_next: *const u32,
    render_ptr: *const u32,
    render_y: u32,
    last_y: u32,
}

/// Size of a line buffer in u32 units.
const LINE_BUF_SIZE: usize = 256;

static mut LINE_BUF: LineBuf = LineBuf::zero();

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
    buf: [u32; LINE_BUF_SIZE],
}

impl LineBuf {
    const fn zero() -> Self {
        let buf = [0; LINE_BUF_SIZE];
        LineBuf { buf }
    }
}

core::arch::global_asm! {
    include_str!("scan.asm"),
    include_str!("render.asm"),
    options(raw)
}

extern "C" {
    fn video_scan(scan_list: *const u32, input: *const u32, output: *mut u32) -> *const u32;

    fn render_engine(render_list: *const u32, output: *mut u32, y: u32);
}

pub const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    if BPP == 16 {
        (b as u32 >> 3) | ((g as u32 & 0xf8) << 2) | ((r as u32 & 0xf8) << 7)
    } else {
        panic!("unsupported color depth")
    }
}

/// Creates a packed color for a 24 bit RGB color.
///
/// The input color is of the form 0xRRGGBB.
///
/// If each channel is already separated out, use [`rgb`] instead.
pub const fn xrgb(color: u32) -> u32 {
    if BPP == 32 {
        color
    } else {
        rgb((color >> 16) as u8, (color >> 8) as u8, color as u8)
    }
}

impl ScanRender {
    pub fn new() -> Self {
        let stripe_remaining = 0;
        let scan_ptr = core::ptr::null();
        let scan_next = core::ptr::null();
        let render_ptr = core::ptr::null();
        let render_y = 0;
        let display_list = DisplayList::new(640, 480 / VERTICAL_REPEAT as u32);
        ScanRender {
            stripe_remaining,
            scan_ptr,
            scan_next,
            render_ptr,
            render_y,
            display_list,
            last_y: 0,
        }
    }

    /// Render one scanline from the line buffer into the provided video buffer.
    #[link_section = ".data"]
    // TODO: probably should be inline-able, this was probably for inspecting disasm
    #[inline(never)]
    pub fn render_scanline(&mut self, video_buf: &mut [u32], y: u32) {
        unsafe {
            if y <= self.last_y {
                DISPLAY_LIST_SWAPCELL.try_swap_by_system(&mut self.display_list);

                self.render_ptr = self.display_list.render.get().as_ptr();
                self.render_y = 0;
                self.scan_next = self.display_list.scan.get().as_ptr();
                self.stripe_remaining = 0;
            }
            if self.stripe_remaining == 0 {
                self.stripe_remaining = self.scan_next.read();
                self.scan_ptr = self.scan_next.add(1);
                // TODO: set desperate scan_next
            }
            // we could just stack allocate the tmp, as we're currently
            // completely synchronous.
            let line_buf_ptr = LINE_BUF.buf.as_mut_ptr();
            let render_ptr = self.render_ptr.add(2);
            render_engine(render_ptr, line_buf_ptr, self.render_y);
            self.render_y += 1;
            let stripe_height = self.render_ptr.read();
            // TODO: this assumes we don't miss; can be made much more robust.
            if self.render_y == stripe_height {
                let jump = self.render_ptr.add(1).read() as usize;
                self.render_ptr = self.display_list.render.get().as_ptr().add(jump);
                self.render_y = 0;
            }
            self.scan_next = video_scan(self.scan_ptr, line_buf_ptr, video_buf.as_mut_ptr());
            self.stripe_remaining -= 1;
            self.last_y = y;
        }
    }
}

impl DisplayList {
    pub fn new(width: u32, height: u32) -> Self {
        let mut rb = RenderlistBuilder::new(width);
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
pub fn init_display_swapcell(width: u32) {
    // The display list doesn't have to be usable.
    DISPLAY_LIST_SWAPCELL.set_for_client(DisplayList::new(width, 0));
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
