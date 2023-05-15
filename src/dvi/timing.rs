//! timing information yoinked from
//! <https://github.com/Wren6991/PicoDVI/blob/51237271437e9d1eb62c97e40171fbf6ffe01ac6/software/libdvi/dvi_timing.c>

// VGA -- we do this mode properly, with a pretty comfortable clk_sys (252 MHz)
// const struct dvi_timing __dvi_const(dvi_timing_640x480p_60hz) = {
// 	.h_sync_polarity   = false,
// 	.h_front_porch     = 16,
// 	.h_sync_width      = 96,
// 	.h_back_porch      = 48,
// 	.h_active_pixels   = 640,

// 	.v_sync_polarity   = false,
// 	.v_front_porch     = 10,
// 	.v_sync_width      = 2,
// 	.v_back_porch      = 33,
// 	.v_active_lines    = 480,

// 	.bit_clk_khz       = 252000
// };

use super::tmds::{TmdsPair, TmdsSym};

// Perhaps there should be a trait with associated constants for resolution,
// to allow compile-time allocation of scanline buffers etc.
pub struct DviTiming {
    h_sync_polarity: bool,
    h_front_porch: u32,
    h_sync_width: u32,
    h_back_porch: u32,
    h_active_pixels: u32,

    v_sync_polarity: bool,
    v_front_porch: u32,
    v_sync_width: u32,
    v_back_porch: u32,
    v_active_lines: u32,

    bit_clk_khz: u32,
}

pub const VGA_TIMING: DviTiming = DviTiming {
    h_sync_polarity: false,
    h_front_porch: 16,
    h_sync_width: 96,
    h_back_porch: 48,
    h_active_pixels: 640,

    v_sync_polarity: false,
    v_front_porch: 10,
    v_sync_width: 2,
    v_back_porch: 33,
    v_active_lines: 480,

    bit_clk_khz: 252000,
};

#[derive(Default)]
struct DviTimingState {
    ctr: u32,
    state: DviTimingLineState,
}

#[derive(Clone, Copy)]
enum DviTimingLineState {
    FrontPorch,
    Sync,
    BackPorch,
    Active,
}

impl Default for DviTimingLineState {
    fn default() -> Self {
        DviTimingLineState::FrontPorch
    }
}

impl DviTiming {
    fn n_lines_for_state(&self, state: DviTimingLineState) -> u32 {
        match state {
            DviTimingLineState::FrontPorch => self.v_front_porch,
            DviTimingLineState::Sync => self.v_sync_width,
            DviTimingLineState::BackPorch => self.v_back_porch,
            DviTimingLineState::Active => self.v_active_lines,
        }
    }
}

impl DviTimingLineState {
    fn next(self) -> Self {
        use DviTimingLineState::*;
        match self {
            FrontPorch => Sync,
            Sync => BackPorch,
            BackPorch => Active,
            Active => FrontPorch,
        }
    }
}

// It would be nice to use types from `rp2040_pac`, but those aren't
// repr(transparent).
#[repr(C)]
#[derive(Default)]
struct DmaCb {
    read_addr: u32,
    write_addr: u32,
    transfer_count: u32,
    config: DmaChannelConfig,
}

// We're doing this by hand because it's not provided by rp2040-pac, as it's
// based on svd2rust (which is quite tight-assed), but would be provided by
// the hal if we were using rp_pac, which is chiptool-based.
#[repr(transparent)]
#[derive(Clone, Copy, Default)]
struct DmaChannelConfig(u32);

impl DmaChannelConfig {
    fn ring(self, ring_sel: bool, ring_size: u32) -> Self {
        let mut bits = self.0 & !0x7c0;
        bits |= (ring_sel as u32) << 10;
        bits |= ring_size << 6;
        Self(bits)
    }

    fn chain_to(self, chan: u32) -> Self {
        let mut bits = self.0 & !0x7800;
        bits |= chan << 11;
        Self(bits)
    }

    fn dreq(self, dreq: u32) -> Self {
        let mut bits = self.0 & !0x1f8000;
        bits |= dreq << 15;
        Self(bits)
    }

    fn irq_quiet(self, quiet: bool) -> Self {
        let mut bits = self.0 & !(1 << 21);
        bits |= (quiet as u32) << 21;
        Self(bits)
    }
}

impl DmaCb {
    fn set(
        &mut self,
        read_addr: &TmdsPair,
        dma_cfg: &DviLaneDmaCfg,
        transfer_count: u32,
        read_ring: u32,
        irq_on_finish: bool,
    ) {
        self.read_addr = read_addr as *const _ as u32;
        self.write_addr = dma_cfg.tx_fifo as u32;
        self.transfer_count = transfer_count;
        self.config = DmaChannelConfig::default()
            .ring(false, read_ring)
            .dreq(dma_cfg.dreq)
            .chain_to(dma_cfg.chan_ctrl)
            .irq_quiet(!irq_on_finish);
    }
}

const DVI_SYNC_LANE_CHUNKS: usize = 4;
const DVI_NOSYNC_LANE_CHUNKS: usize = 2;

#[derive(Default)]
struct DmaScanlineDmaList {
    l0: [DmaCb; DVI_SYNC_LANE_CHUNKS],
    l1: [DmaCb; DVI_NOSYNC_LANE_CHUNKS],
    l2: [DmaCb; DVI_NOSYNC_LANE_CHUNKS],
}

struct DviLaneDmaCfg {
    chan_ctrl: u32,
    chan_data: u32,
    tx_fifo: *mut u8,
    dreq: u32,
}

impl DmaScanlineDmaList {
    fn lane(&self, i: usize) -> &[DmaCb] {
        match i {
            0 => &self.l0,
            1 => &self.l1,
            _ => &self.l2,
        }
    }

    fn lane_mut(&mut self, i: usize) -> &mut [DmaCb] {
        match i {
            0 => &mut self.l0,
            1 => &mut self.l1,
            _ => &mut self.l2,
        }
    }

    fn setup_scanline_for_vblank(
        &mut self,
        t: &DviTiming,
        dma_cfg: &[DviLaneDmaCfg],
        vsync_asserted: bool,
    ) {
        for (i, dma_cfg) in dma_cfg.iter().enumerate() {
            let lane = self.lane_mut(i);
            if i == 0 {
                let vsync = t.v_sync_polarity == vsync_asserted;
                let sym_hsync_off = get_ctrl_sym(vsync, !t.h_sync_polarity);
                let sym_hsync_on = get_ctrl_sym(vsync, t.h_sync_polarity);
                lane[0].set(sym_hsync_off, dma_cfg, t.h_front_porch / 2, 2, false);
                lane[1].set(sym_hsync_on, dma_cfg, t.h_sync_width / 2, 2, false);
                lane[2].set(sym_hsync_off, dma_cfg, t.h_back_porch / 2, 2, true);
                lane[3].set(sym_hsync_off, dma_cfg, t.h_active_pixels / 2, 2, false);
            } else {
                let inactive = t.h_front_porch + t.h_sync_width + t.h_back_porch;
                let sym_no_sync = get_ctrl_sym(false, false);
                lane[0].set(sym_no_sync, dma_cfg, inactive / 2, 2, false);
                lane[1].set(sym_no_sync, dma_cfg, t.h_active_pixels / 2, 2, false);
            }
        }
    }

    // TODO: add tmdsbuf: Option<&[TmdsPair]>
    fn setup_scanline_for_active(&mut self, t: &DviTiming, dma_cfg: &[DviLaneDmaCfg]) {
        for (i, dma_cfg) in dma_cfg.iter().enumerate() {
            let lane = self.lane_mut(i);
            let sym_no_sync = get_ctrl_sym(false, false);
            let active_lane;
            if i == 0 {
                let sym_hsync_off = get_ctrl_sym(!t.v_sync_polarity, !t.h_sync_polarity);
                let sym_hsync_on = get_ctrl_sym(!t.v_sync_polarity, t.h_sync_polarity);
                lane[0].set(sym_hsync_off, dma_cfg, t.h_front_porch / 2, 2, false);
                lane[1].set(sym_hsync_on, dma_cfg, t.h_sync_width / 2, 2, false);
                lane[2].set(sym_hsync_off, dma_cfg, t.h_back_porch / 2, 2, true);
                active_lane = 3;
            } else {
                let inactive = t.h_front_porch + t.h_sync_width + t.h_back_porch;
                lane[0].set(sym_no_sync, dma_cfg, inactive / 2, 2, false);
                active_lane = 1;
            }
            lane[active_lane].set(
                &EMPTY_SCANLINE_TMDS[i],
                dma_cfg,
                t.h_active_pixels / 2,
                2,
                false,
            );
        }
    }
}

const DVI_CTRL_SYMS: [TmdsPair; 4] = [
    TmdsPair::double(TmdsSym::C0),
    TmdsPair::double(TmdsSym::C1),
    TmdsPair::double(TmdsSym::C2),
    TmdsPair::double(TmdsSym::C3),
];

fn get_ctrl_sym(vsync: bool, hsync: bool) -> &'static TmdsPair {
    &DVI_CTRL_SYMS[((vsync as usize) << 1) | (hsync as usize)]
}

const EMPTY_SCANLINE_TMDS: [TmdsPair; 3] = [
    TmdsPair::encode_balanced_approx(0x00),
    TmdsPair::encode_balanced_approx(0x00),
    TmdsPair::encode_balanced_approx(0xfe),
];
