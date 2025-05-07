pub mod timing;

use crate::hal::pac::{interrupt, Peripherals, DMA, HSTX_CTRL, HSTX_FIFO, IO_BANK0, PADS_BANK0};
use alloc::boxed::Box;
use timing::{DviTiming, DviTimingLineState, DviTimingState, SYNC_LINE_WORDS};

use crate::DVI_INST;

/// Bits per pixel
pub const BPP: usize = 16;

pub struct DviInst {
    timing: DviTiming,
    dma_pong: bool,
    timing_state: DviTimingState,
    vblank_line_vsync_off: [u32; SYNC_LINE_WORDS],
    vblank_line_vsync_on: [u32; SYNC_LINE_WORDS],
    vactive_lines: [Box<[u32]>; 2],
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

impl DviInst {
    pub fn new(timing: DviTiming) -> Self {
        let vblank_line_vsync_off = timing.make_sync_line(false);
        let vblank_line_vsync_on = timing.make_sync_line(true);

        let vactive_size = 8 + timing.h_active_pixels as usize * BPP / 32;
        let vactive_lines = core::array::from_fn(|_| {
            let mut buf = alloc::vec![!0; vactive_size];
            buf[0] = hstx_cmd_raw_repeat(timing.h_front_porch);
            buf[1] = timing.tmds3_for_sync(false, false);
            buf[2] = hstx_cmd_nop();
            buf[3] = hstx_cmd_raw_repeat(timing.h_sync_width);
            buf[4] = timing.tmds3_for_sync(true, false);
            buf[5] = hstx_cmd_raw_repeat(timing.h_back_porch);
            buf[6] = timing.tmds3_for_sync(false, false);
            buf[7] = hstx_cmd_tmds(timing.h_active_pixels);
            buf.into()
        });
        DviInst {
            timing,
            dma_pong: false,
            timing_state: DviTimingState::new(2),
            vblank_line_vsync_off,
            vblank_line_vsync_on,
            vactive_lines,
        }
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
        let v_state = inst.timing_state.v_state(&inst.timing);
        let cmds = match v_state {
            DviTimingLineState::Active => &inst.vactive_lines[ch_num],
            DviTimingLineState::Sync => &inst.vblank_line_vsync_on[..],
            _ => &inst.vblank_line_vsync_off[..],
        };
        dma.intr().write(|w| w.bits(1 << ch_num));
        ch.ch_read_addr().write(|w| w.bits(cmds.as_ptr() as u32));
        ch.ch_trans_count().write(|w| w.bits(cmds.len() as u32));
        inst.timing_state.advance(&inst.timing);
    }
}
