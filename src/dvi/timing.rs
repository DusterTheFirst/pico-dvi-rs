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

use rp_pico::hal::dma::SingleChannel;

use super::{
    dma::{DmaCb, DmaChannels, DviLaneDmaCfg},
    tmds::{TmdsPair, TmdsSym},
};

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

    pub bit_clk_khz: u32,
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
pub struct DviTimingState {
    v_ctr: u32,
    v_state: DviTimingLineState,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DviTimingLineState {
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

impl DviTimingState {
    pub fn advance(&mut self, timing: &DviTiming) {
        self.v_ctr += 1;
        if self.v_ctr == timing.n_lines_for_state(self.v_state) {
            self.v_state = self.v_state.next();
            self.v_ctr = 0;
        }
    }

    pub fn v_state(&self) -> DviTimingLineState {
        self.v_state
    }
}

const DVI_SYNC_LANE_CHUNKS: usize = 4;
const DVI_NOSYNC_LANE_CHUNKS: usize = 2;

#[derive(Default)]
pub struct DviScanlineDmaList {
    l0: [DmaCb; DVI_SYNC_LANE_CHUNKS],
    l1: [DmaCb; DVI_NOSYNC_LANE_CHUNKS],
    l2: [DmaCb; DVI_NOSYNC_LANE_CHUNKS],
}

impl DviScanlineDmaList {
    pub fn lane(&self, i: usize) -> &[DmaCb] {
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

    fn setup_lane_0<Ch0, Ch1>(
        &mut self,
        t: &DviTiming,
        dma_cfg: &DviLaneDmaCfg<Ch0, Ch1>,
        line_state: DviTimingLineState,
    ) where
        Ch0: SingleChannel,
        Ch1: SingleChannel,
    {
        let vsync = (line_state == DviTimingLineState::Sync) == t.v_sync_polarity;
        let sym_hsync_off = get_ctrl_sym(vsync, !t.h_sync_polarity);
        let sym_hsync_on = get_ctrl_sym(vsync, t.h_sync_polarity);
        let lane = &mut self.l0;
        lane[0].set(sym_hsync_off, dma_cfg, t.h_front_porch / 2, 2, false);
        lane[1].set(sym_hsync_on, dma_cfg, t.h_sync_width / 2, 2, false);
        lane[2].set(sym_hsync_off, dma_cfg, t.h_back_porch / 2, 2, true);
        let sym = match line_state {
            DviTimingLineState::Active => &EMPTY_SCANLINE_TMDS[0],
            _ => sym_hsync_off,
        };
        lane[3].set(sym, dma_cfg, t.h_active_pixels / 2, 2, false);
    }

    fn setup_lane_12<Ch0, Ch1>(
        &mut self,
        lane_number: usize,
        t: &DviTiming,
        dma_cfg: &DviLaneDmaCfg<Ch0, Ch1>,
        line_state: DviTimingLineState,
    ) where
        Ch0: SingleChannel,
        Ch1: SingleChannel,
    {
        let sym_no_sync = get_ctrl_sym(false, false);
        let lane = self.lane_mut(lane_number);
        let inactive = t.h_front_porch + t.h_sync_width + t.h_back_porch;
        lane[0].set(sym_no_sync, dma_cfg, inactive / 2, 2, false);
        let sym = match line_state {
            DviTimingLineState::Active => &EMPTY_SCANLINE_TMDS[lane_number],
            _ => sym_no_sync,
        };
        lane[1].set(sym, dma_cfg, t.h_active_pixels / 2, 2, false);
    }

    pub fn setup_scanline<Ch0, Ch1, Ch2, Ch3, Ch4, Ch5>(
        &mut self,
        t: &DviTiming,
        dma_cfg: &DmaChannels<Ch0, Ch1, Ch2, Ch3, Ch4, Ch5>,
        line_state: DviTimingLineState,
    ) where
        Ch0: SingleChannel,
        Ch1: SingleChannel,
        Ch2: SingleChannel,
        Ch3: SingleChannel,
        Ch4: SingleChannel,
        Ch5: SingleChannel,
    {
        self.setup_lane_0(t, &dma_cfg.lane0, line_state);
        self.setup_lane_12(1, t, &dma_cfg.lane1, line_state);
        self.setup_lane_12(2, t, &dma_cfg.lane2, line_state);
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
