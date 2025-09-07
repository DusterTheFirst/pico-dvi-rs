#[cfg(feature = "audio")]
mod data_island;
pub mod pinout;
pub mod timing;

use alloc::boxed::Box;
use core::{
    cell::UnsafeCell,
    mem::MaybeUninit,
    sync::atomic::{
        AtomicBool,
        Ordering::{Acquire, Relaxed, Release},
    },
};
use embedded_hal::digital::StatefulOutputPin;
use rp235x_hal::gpio::{bank0::Gpio10, FunctionSio, Pin, PullDown, SioOutput};

#[cfg(feature = "audio")]
use crate::dvi::{data_island::DataPacket, timing::SYNC_DATA_ISLAND_LEN};

use crate::{
    hal::pac::{
        interrupt, Interrupt, Peripherals, DMA, HSTX_CTRL, HSTX_FIFO, IO_BANK0, PADS_BANK0,
    },
    render::{Queue, ScanRender},
    DVI_OUT,
};
use cortex_m::peripheral::NVIC;
use pinout::DviPinout;
use timing::{
    DviTiming, DviTimingLineState, DviTimingState, SYNC_LINE_ONLY_WORDS, SYNC_LINE_WORDS,
};

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

pub struct DviOut {
    line_queue: Queue<LINE_QUEUE_SIZE>,
    line_lent: [AtomicBool; N_VIDEO_BUFFERS],
    video_lines: [UnsafeCell<MaybeUninit<Box<[u32]>>>; N_VIDEO_BUFFERS],
    // TODO: DviInst should go in here.
}

pub struct DviInst {
    timing: DviTiming,
    dma_pong: bool,
    timing_state: DviTimingState,

    // New state
    sync_pulse_vsync_off: [u32; SYNC_LINE_WORDS],
    sync_pulse_vsync_on: [u32; SYNC_LINE_WORDS],
    sync_line_only_vsync_off: [u32; SYNC_LINE_ONLY_WORDS],
    sync_line_only_vsync_on: [u32; SYNC_LINE_ONLY_WORDS],
    err_line: [u32; SYNC_LINE_ONLY_WORDS],

    #[cfg(feature = "audio")]
    data_island_sync: [u32; SYNC_DATA_ISLAND_LEN],
    audio_buf: [[i16; 2]; 4],
    audio_ix: usize,
    frame_count: i32,

    missed: [bool; N_VIDEO_BUFFERS],
}

pub struct LineGuard<'a> {
    dvi_out: &'a DviOut,
    buf_ix: usize,
}

const LINE_QUEUE_SIZE: usize = (N_VIDEO_BUFFERS + 1).next_power_of_two();

impl DviOut {
    pub const fn new() -> Self {
        Self {
            line_queue: Queue::new(),
            line_lent: [const { AtomicBool::new(false) }; N_VIDEO_BUFFERS],
            video_lines: [const { UnsafeCell::new(MaybeUninit::uninit()) }; N_VIDEO_BUFFERS],
        }
    }

    pub fn get_line(&self) -> (u32, LineGuard) {
        let line_ix = self.line_queue.take_blocking();
        let buf_ix = line_ix as usize % N_VIDEO_BUFFERS;
        let guard = LineGuard {
            dvi_out: self,
            buf_ix,
        };
        (line_ix, guard)
    }
}

impl Drop for LineGuard<'_> {
    fn drop(&mut self) {
        self.dvi_out.line_lent[self.buf_ix].store(false, Release);
    }
}

impl LineGuard<'_> {
    fn buf_mut(&mut self) -> &mut [u32] {
        unsafe {
            let line = (*self.dvi_out.video_lines[self.buf_ix].get()).assume_init_mut();
            &mut line[1..]
        }
    }
}

unsafe impl Sync for DviOut {}

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

#[allow(unused)]
const fn hstx_cmd_nop() -> u32 {
    0xf << 12
}

#[inline(never)]
pub unsafe fn setup_hstx(hstx: &HSTX_CTRL, pinout: DviPinout) {
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
        hstx.bit0().write(|w| w.bits(pinout.cfg_bits(0)));
        hstx.bit1().write(|w| w.bits(pinout.cfg_bits(1)));
        hstx.bit2().write(|w| w.bits(pinout.cfg_bits(2)));
        hstx.bit3().write(|w| w.bits(pinout.cfg_bits(3)));
        hstx.bit4().write(|w| w.bits(pinout.cfg_bits(4)));
        hstx.bit5().write(|w| w.bits(pinout.cfg_bits(5)));
        hstx.bit6().write(|w| w.bits(pinout.cfg_bits(6)));
        hstx.bit7().write(|w| w.bits(pinout.cfg_bits(7)));
    }
}

const DREQ_HSTX: u8 = 52;

#[inline(never)]
pub unsafe fn setup_dma(dma: &DMA, hstx_fifo: &HSTX_FIFO) {
    let inst = (*DVI_INST.0.get()).assume_init_mut();
    unsafe {
        for i in 0..2 {
            let cmds = if i == 0 {
                &inst.sync_pulse_vsync_off[..]
            } else {
                &inst.sync_line_only_vsync_off[..]
            };
            let ch = dma.ch(i);
            ch.ch_read_addr().write(|w| w.bits(cmds.as_ptr() as u32));
            ch.ch_write_addr()
                .write(|w| w.bits(hstx_fifo.fifo().as_ptr() as u32));
            ch.ch_trans_count().write(|w| w.bits(cmds.len() as u32));
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

impl DviInst {
    pub fn new(timing: DviTiming) -> Self {
        let sync_pulse_vsync_off = timing.make_sync_pulse(false);
        let sync_pulse_vsync_on = timing.make_sync_pulse(true);
        let sync_line_only_vsync_off = timing.make_sync_line_only(false);
        let sync_line_only_vsync_on = timing.make_sync_line_only(true);
        let mut err_line = [0x7c007c00; SYNC_LINE_ONLY_WORDS];
        // TODO: correct logic for constants
        const TAIL: u32 = 16;
        err_line[0] = hstx_cmd_tmds_repeat(timing.h_active_pixels - TAIL);
        err_line[2] = hstx_cmd_tmds(TAIL);

        let vline_size = 1 + timing.h_active_pixels as usize * BPP / 32;
        for line in &DVI_OUT.video_lines {
            let mut buf = alloc::vec![!0; vline_size];
            buf[0] = hstx_cmd_tmds(timing.h_active_pixels);
            unsafe {
                (*line.get()).write(buf.into());
            }
        }

        #[cfg(feature = "audio")]
        let mut data_island_sync = [0; SYNC_DATA_ISLAND_LEN];
        #[cfg(feature = "audio")]
        timing.init_data_island(&mut data_island_sync);

        // The number of video lines that have been set up by the
        // time of the first interrupt.
        const INIT_TIMING_STATE: u32 = 1;
        DviInst {
            timing,
            dma_pong: false,
            timing_state: DviTimingState::new(INIT_TIMING_STATE),
            sync_pulse_vsync_off,
            sync_pulse_vsync_on,
            sync_line_only_vsync_off,
            sync_line_only_vsync_on,
            #[cfg(feature = "audio")]
            data_island_sync,
            err_line,
            audio_buf: Default::default(),
            audio_ix: 0,
            frame_count: 0,
            missed: [false; N_VIDEO_BUFFERS],
        }
    }

    #[cfg(feature = "audio")]
    #[link_section = ".data"]
    /// Return true if audio buffer is updated
    fn do_audio(&mut self, packet: &mut DataPacket) -> bool {
        let y = self.timing_state.v_ctr();
        // Strategy here is to encode audio on even scanlines. It's not clear this is
        // fully spec-compliant, but we'll see.
        let samples_this_scanline = ((0b10100 >> (y % 5)) & 1) + 1;
        for i in 0..samples_this_scanline {
            // 60Hz triangle wave
            let t = y as f32 / 525.0;
            let audio = (65536. * ((t - 0.5).abs() - 0.25)) as i16;
            self.audio_buf[self.audio_ix + i] = [audio, audio];
        }
        self.audio_ix += samples_this_scanline;
        if y % 2 == 0 {
            packet.set_audio(&self.audio_buf[..self.audio_ix], &mut self.frame_count);
            self.audio_ix = 0;
        } else {
            match y {
                1 => packet.set_audio_info_frame(44_100),
                3 => packet.set_avi_info_frame(
                    data_island::ScanInfo::Underscan,
                    data_island::PixelFormat::Rgb,
                    data_island::Colorimetry::Itu601,
                    data_island::PictureAspectRatio::Ratio4_3,
                    data_island::ActiveFormatAspectRatio::SameAsPar,
                    data_island::QuantizationRange::Full,
                    data_island::VideoCode::Code640x480P60,
                ),
                5 => packet.set_audio_clock_regeneration(28000, 6272),
                _ => return false,
            }
        }

        true
    }
}

#[link_section = ".data"]
pub fn core1_main() -> ! {
    let mut scan_render = ScanRender::new();
    unsafe {
        NVIC::unmask(Interrupt::DMA_IRQ_0);
        let dma = &Peripherals::steal().DMA;
        start_dma(dma);
    }
    loop {
        let (y, mut guard) = DVI_OUT.get_line();
        let line = guard.buf_mut();
        scan_render.render_scanline(line, y);
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
        dma.intr().write(|w| w.bits(1 << ch_num));
        let ch = dma.ch(ch_num);
        inst.dma_pong = !inst.dma_pong;
        if inst.dma_pong {
            // interrupt at end of sync pulse, set up next sync pulse
            #[cfg(feature = "audio")]
            let mut pack = MaybeUninit::uninit();
            #[cfg(feature = "audio")]
            data_island::clear_data_packet(&mut pack);
            #[cfg(feature = "audio")]
            let packet = pack.assume_init_mut();
            #[cfg(feature = "audio")]
            let is_audio = inst.do_audio(packet);
            let state = inst.timing_state.v_state(&inst.timing);
            let cmds = match state {
                #[cfg(feature = "audio")]
                _ if is_audio => {
                    inst.timing
                        .encode_data_island(&mut inst.data_island_sync, state, packet);
                    &inst.data_island_sync[..]
                }
                #[cfg(feature = "audio")]
                DviTimingLineState::Active => {
                    inst.timing
                        .encode_data_island_empty(&mut inst.data_island_sync, state);
                    &inst.data_island_sync[..]
                }
                DviTimingLineState::Sync => &inst.sync_pulse_vsync_on[..],
                _ => &inst.sync_pulse_vsync_off[..],
            };
            ch.ch_read_addr().write(|w| w.bits(cmds.as_ptr() as u32));
            ch.ch_trans_count().write(|w| w.bits(cmds.len() as u32));
        } else {
            // interrupt at end of line, set up next line
            let cmds = match inst.timing_state.v_state(&inst.timing) {
                DviTimingLineState::Active => {
                    // TODO: could be optimized
                    let y = inst
                        .timing_state
                        .v_scanline_index(&inst.timing, 0)
                        .unwrap_or_default();
                    let buf_ix = (y as usize / VERTICAL_REPEAT) % N_VIDEO_BUFFERS;
                    if inst.missed[buf_ix] || DVI_OUT.line_lent[buf_ix].load(Acquire) {
                        &inst.err_line[..]
                    } else {
                        (*DVI_OUT.video_lines[buf_ix].get()).assume_init_ref()
                    }
                }
                DviTimingLineState::Sync => &inst.sync_line_only_vsync_on[..],
                _ => &inst.sync_line_only_vsync_off[..],
            };
            ch.ch_read_addr().write(|w| w.bits(cmds.as_ptr() as u32));
            ch.ch_trans_count().write(|w| w.bits(cmds.len() as u32));
            let offset = VIDEO_PIPELINE_SLACK + ((N_VIDEO_BUFFERS - 1) * VERTICAL_REPEAT) as u32;
            if let Some(y) = inst.timing_state.v_scanline_index(&inst.timing, offset) {
                if y as usize % VERTICAL_REPEAT == 0 {
                    let y_scaled = y / VERTICAL_REPEAT as u32;
                    let buf_ix = y_scaled as usize % N_VIDEO_BUFFERS;
                    let missed = DVI_OUT.line_lent[buf_ix].load(Acquire);
                    if !missed {
                        DVI_OUT.line_queue.push_unchecked(y_scaled);
                        DVI_OUT.line_lent[buf_ix].store(true, Relaxed);
                    }
                    inst.missed[buf_ix] = missed;
                }
            }
            inst.timing_state.advance(&inst.timing);
        }
    }
}
