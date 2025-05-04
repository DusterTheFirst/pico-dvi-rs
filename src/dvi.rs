pub mod timing;
pub mod tmds;

use crate::{
    hal::pac::{
        interrupt, Interrupt, Peripherals, DMA, HSTX_CTRL, HSTX_FIFO, IO_BANK0, PADS_BANK0,
    },
    render::{render_line, ScanRender, CORE1_QUEUE, N_LINE_BUFS},
};
use alloc::boxed::Box;
use cortex_m::peripheral::NVIC;
use timing::{DviTiming, DviTimingLineState, DviTimingState, SYNC_LINE_WORDS};

use crate::DVI_INST;

/// Bits per pixel
pub const BPP: usize = 16;

/// Currently only 1 is supported
pub const VERTICAL_REPEAT: usize = 1;

/// The additional time (in scanlines) for the video encoding routine.
///
/// If video encoding can reliably happen in less than one scanline time,
/// this should be 0. If there is variance that sometimes pushes it over
/// the line, then a value of 1 may eliminate artifacts.
///
/// Note: currently only 0 is supported.
const VIDEO_PIPELINE_SLACK: u32 = 0;

const N_VIDEO_BUFFERS: usize = if VIDEO_PIPELINE_SLACK > 0 && VERTICAL_REPEAT == 1 {
    3
} else {
    2
};

pub struct DviInst {
    timing: DviTiming,
    dma_pong: bool,
    timing_state: DviTimingState,
    vblank_line_vsync_off: [u32; SYNC_LINE_WORDS],
    vblank_line_vsync_on: [u32; SYNC_LINE_WORDS],
    vactive_lines: [Box<[u32]>; N_VIDEO_BUFFERS],
    available: [bool; N_VIDEO_BUFFERS],
    scan_render: ScanRender,
}

#[allow(unused)]
const fn hstx_cmd_raw(len: u32) -> u32 {
    (0 << 12) | len
}

const fn hstx_cmd_raw_repeat(len: u32) -> u32 {
    (1 << 12) | len
}

const fn hstx_cmd_tmds(len: u32) -> u32 {
    (2 << 12) | len
}

#[allow(unused)]
const fn hstx_cmd_tmds_repeat(len: u32) -> u32 {
    (3 << 12) | len
}

const fn hstx_cmd_nop() -> u32 {
    0xf << 12
}

#[inline(never)]
pub unsafe fn setup_hstx(hstx: &HSTX_CTRL) {
    unsafe {
        match BPP {
            16 => {
                // configure for rgb 555
                hstx.expand_tmds().write(|w| {
                    w.l0_nbits()
                        .bits(4)
                        .l0_rot()
                        .bits(29)
                        .l1_nbits()
                        .bits(4)
                        .l1_rot()
                        .bits(2)
                        .l2_nbits()
                        .bits(4)
                        .l2_rot()
                        .bits(7)
                });
                hstx.expand_shift().write(|w| {
                    w.enc_n_shifts()
                        .bits(2)
                        .enc_shift()
                        .bits(16)
                        .raw_n_shifts()
                        .bits(1)
                });
            }
            32 => {
                // configure for rgb 888
                hstx.expand_tmds().write(|w| {
                    w.l0_nbits()
                        .bits(7)
                        .l0_rot()
                        .bits(0)
                        .l1_nbits()
                        .bits(7)
                        .l1_rot()
                        .bits(8)
                        .l2_nbits()
                        .bits(7)
                        .l2_rot()
                        .bits(16)
                });
                hstx.expand_shift().write(|w| {
                    w.enc_n_shifts()
                        .bits(1)
                        .enc_shift()
                        .bits(0)
                        .raw_n_shifts()
                        .bits(1)
                });
            }
            _ => panic!("unsupported pixel depth"),
        }
        // default expand_shift is fine for non-doubled 888
        hstx.csr().write(|w| {
            w.expand_en()
                .set_bit()
                .clkdiv()
                .bits(5)
                .n_shifts()
                .bits(5)
                .shift()
                .bits(2)
                .en()
                .set_bit()
        });
        hstx.bit2().write(|w| w.clk().set_bit());
        hstx.bit3().write(|w| w.clk().set_bit().inv().set_bit());
        // Lane assignments for Adafruit feather board;
        const PERM: [u8; 3] = [2, 1, 0];
        hstx.bit0()
            .write(|w| w.sel_p().bits(PERM[0] * 10).sel_n().bits(PERM[0] * 10 + 1));
        hstx.bit1().write(|w| {
            w.sel_p()
                .bits(PERM[0] * 10)
                .sel_n()
                .bits(PERM[0] * 10 + 1)
                .inv()
                .set_bit()
        });
        hstx.bit4()
            .write(|w| w.sel_p().bits(PERM[1] * 10).sel_n().bits(PERM[1] * 10 + 1));
        hstx.bit5().write(|w| {
            w.sel_p()
                .bits(PERM[1] * 10)
                .sel_n()
                .bits(PERM[1] * 10 + 1)
                .inv()
                .set_bit()
        });
        hstx.bit6()
            .write(|w| w.sel_p().bits(PERM[2] * 10).sel_n().bits(PERM[2] * 10 + 1));
        hstx.bit7().write(|w| {
            w.sel_p()
                .bits(PERM[2] * 10)
                .sel_n()
                .bits(PERM[2] * 10 + 1)
                .inv()
                .set_bit()
        });
    }
}

const DREQ_HSTX: u8 = 52;

#[inline(never)]
pub unsafe fn setup_dma(dma: &DMA, hstx_fifo: &HSTX_FIFO) {
    let inst = (*DVI_INST.0.get()).assume_init_mut();
    unsafe {
        for i in 0..2 {
            let ch = dma.ch(i);
            ch.ch_read_addr()
                .write(|w| w.bits(inst.vblank_line_vsync_off.as_ptr() as u32));
            ch.ch_write_addr()
                .write(|w| w.bits(hstx_fifo.fifo().as_ptr() as u32));
            ch.ch_trans_count()
                .write(|w| w.bits(inst.vblank_line_vsync_off.len() as u32));
            ch.ch_al1_ctrl().write(|w| {
                w.chain_to()
                    .bits((i ^ 1) as u8)
                    .data_size()
                    .bits(2)
                    .incr_read()
                    .set_bit()
                    .treq_sel()
                    .bits(DREQ_HSTX)
                    .en()
                    .set_bit()
            });
        }
        dma.ints0().write(|w| w.ints0().bits(3));
        dma.inte0().write(|w| w.inte0().bits(3));
    }
}

pub unsafe fn start_dma(dma: &DMA) {
    unsafe {
        dma.multi_chan_trigger()
            .write(|w| w.multi_chan_trigger().bits(1));
    }
}

const FUNCTION_HSTX: u8 = 0;

// This doesn't use the hal's `Pins` abstraction because the HAL is missing
// `FunctionHstx`.
pub unsafe fn setup_pins(pads: &PADS_BANK0, io: &IO_BANK0) {
    for pin in 12..20 {
        // TODO: should we be using hardware set/clear/xor?
        pads.gpio(pin)
            .modify(|_, w| w.ie().set_bit().od().clear_bit());
        unsafe {
            io.gpio(pin)
                .gpio_ctrl()
                .write(|w| w.funcsel().bits(FUNCTION_HSTX));
        }
        pads.gpio(pin).modify(|_, w| w.iso().clear_bit());
    }
}

// Number of TMDS expansion words prefacing video data in an active line.
const ACTIVE_SYNC_WORDS: usize = 7;

impl DviInst {
    pub fn new(timing: DviTiming) -> Self {
        let vblank_line_vsync_off = timing.make_sync_line(false);
        let vblank_line_vsync_on = timing.make_sync_line(true);

        let vactive_size = ACTIVE_SYNC_WORDS + timing.h_active_pixels as usize * BPP / 32;
        let vactive_lines = core::array::from_fn(|_| {
            let mut buf = alloc::vec![!0; vactive_size];
            buf[0] = hstx_cmd_raw_repeat(timing.h_front_porch);
            buf[1] = timing.tmds3_for_sync(false, false);
            buf[2] = hstx_cmd_raw_repeat(timing.h_sync_width);
            buf[3] = timing.tmds3_for_sync(true, false);
            buf[4] = hstx_cmd_raw_repeat(timing.h_back_porch);
            buf[5] = timing.tmds3_for_sync(false, false);
            buf[6] = hstx_cmd_tmds(timing.h_active_pixels);
            buf.into()
        });
        // The number of video lines that have been set up by the
        // time of the first interrupt.
        const INIT_TIMING_STATE: u32 = 2;
        DviInst {
            timing,
            dma_pong: false,
            timing_state: DviTimingState::new(INIT_TIMING_STATE),
            vblank_line_vsync_off,
            vblank_line_vsync_on,
            vactive_lines,
            available: [false; N_VIDEO_BUFFERS],
            scan_render: ScanRender::new(),
        }
    }

    /// Get a reference to an active video scanline, if available
    #[link_section = ".data"]
    fn active_video_line(&mut self) -> Option<&[u32]> {
        if let Some(y) = self.timing_state.v_scanline_index(&self.timing, 0) {
            let buf_ix = (y as usize / VERTICAL_REPEAT) % N_VIDEO_BUFFERS;
            if self.available[buf_ix] {
                return Some(&self.vactive_lines[buf_ix]);
            }
        }
        None
    }

    /// Determine whether a line is available to scan into video.
    ///
    /// If a video render is to be scheduled this scanline, return the
    /// scanline number and a boolean indicating whether the line buffer
    /// is available.
    ///
    /// If no video render is to be scheduled, the scanline number is
    /// `!0`.
    ///
    /// This method also updates the `available` table internally.
    #[link_section = ".data"]
    fn line_available(&mut self) -> (u32, bool) {
        if let Some(y) = self
            .timing_state
            .v_scanline_index(&self.timing, VIDEO_PIPELINE_SLACK)
        {
            if y % VERTICAL_REPEAT as u32 == 0 {
                let y = y / VERTICAL_REPEAT as u32;
                let available = self.scan_render.is_line_available(y);
                let buf_ix = y as usize % N_VIDEO_BUFFERS;
                self.available[buf_ix] = available;
                return (y, available);
            }
        }
        (!0, false)
    }

    /// Render a scanline into a video buffer.
    ///
    /// This function is called even if the corresponding line buffer is not
    /// available, so the display list can be advanced.
    #[link_section = ".data"]
    fn render_video_line(&mut self, y: u32, available: bool) {
        let buf_ix = y as usize % N_VIDEO_BUFFERS;
        let video_slice = &mut self.vactive_lines[buf_ix][ACTIVE_SYNC_WORDS..];
        self.scan_render.render_scanline(video_slice, y, available);
    }

    /// Schedule rendering of a line
    #[link_section = ".data"]
    fn schedule_line_render(&mut self) {
        let offset = VIDEO_PIPELINE_SLACK + (N_LINE_BUFS * VERTICAL_REPEAT) as u32;
        if let Some(y) = self.timing_state.v_scanline_index(&self.timing, offset) {
            if y % VERTICAL_REPEAT as u32 == 0 {
                let y_line = y / VERTICAL_REPEAT as u32;
                self.scan_render.schedule_line_render(y_line);
            }
        }
    }
}

#[link_section = ".data"]
pub fn core1_main() -> ! {
    unsafe {
        NVIC::unmask(Interrupt::DMA_IRQ_0);
    }
    loop {
        let line_ix = CORE1_QUEUE.peek_blocking();
        // Safety: exclusive access to the line buffer is granted
        // when the render is scheduled to a core.
        unsafe { render_line(line_ix) };
        CORE1_QUEUE.remove();
    }
}

// In Rust 2024, this would need to be marked unsafe, but the cortex-m-rt crate
// won't accept it. So 2021 it is.
#[link_section = ".data"]
#[interrupt]
fn DMA_IRQ_0() {
    unsafe {
        let inst = (*DVI_INST.0.get()).assume_init_mut();
        let ch_num = inst.dma_pong as usize;
        let dma = &mut Peripherals::steal().DMA;
        let ch = dma.ch(ch_num);
        inst.dma_pong = !inst.dma_pong;
        let cmds = if let Some(cmds) = inst.active_video_line() {
            cmds
        } else {
            let v_state = inst.timing_state.v_state(&inst.timing);
            match v_state {
                // TODO: should be error line, but probably little harm
                DviTimingLineState::Active => &inst.vactive_lines[0],
                DviTimingLineState::Sync => &inst.vblank_line_vsync_on[..],
                _ => &inst.vblank_line_vsync_off[..],
            }
        };
        dma.intr().write(|w| w.bits(1 << ch_num));
        ch.ch_read_addr().write(|w| w.bits(cmds.as_ptr() as u32));
        ch.ch_trans_count().write(|w| w.bits(cmds.len() as u32));
        // Possible TODO: combine line_available and render_video_line, as
        // there's not much value in having them separate. (It's likely the
        // original motivation was to have the latter maybe outside the
        // interrupt)
        let (y, available) = inst.line_available();
        if y as i32 >= 0 {
            inst.render_video_line(y, available);
        }
        inst.schedule_line_render();
        inst.timing_state.advance(&inst.timing);
    }
}
