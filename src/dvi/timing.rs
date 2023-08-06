//! timing information yoinked from
//! <https://github.com/Wren6991/PicoDVI/blob/51237271437e9d1eb62c97e40171fbf6ffe01ac6/software/libdvi/dvi_timing.c>

use fugit::KilohertzU32;
use rp_pico::hal::dma::SingleChannel;

use super::{
    dma::{DmaChannels, DmaControlBlock, DviLaneDmaCfg},
    tmds::{TmdsPair, TmdsSymbol},
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

    pub bit_clk: KilohertzU32,
}

impl DviTiming {
    pub fn horizontal_words(&self) -> u32 {
        self.h_active_pixels / 2
    }

    fn total_lines(&self) -> u32 {
        self.v_front_porch + self.v_sync_width + self.v_back_porch + self.v_active_lines
    }

    fn state_for_v_count(&self, v_count: u32) -> DviTimingLineState {
        let mut y = v_count;
        if y < self.v_front_porch {
            return DviTimingLineState::FrontPorch;
        }
        y -= self.v_front_porch;
        if y < self.v_sync_width {
            return DviTimingLineState::Sync;
        }
        y -= self.v_sync_width;
        if y < self.v_back_porch {
            DviTimingLineState::BackPorch
        } else {
            DviTimingLineState::Active
        }
    }
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

    bit_clk: KilohertzU32::kHz(252000),
};

#[derive(Default)]
pub struct DviTimingState {
    v_ctr: u32,
}

impl DviTimingState {
    pub fn advance(&mut self, timing: &DviTiming) {
        self.v_ctr += 1;
        if self.v_ctr == timing.total_lines() {
            self.v_ctr = 0;
        }
    }

    pub fn v_state(&self, timing: &DviTiming) -> DviTimingLineState {
        timing.state_for_v_count(self.v_ctr)
    }

    pub fn v_scanline_index(&self, timing: &DviTiming, offset: u32) -> Option<u32> {
        let inactive = timing.v_front_porch + timing.v_sync_width + timing.v_back_porch;
        let y = (self.v_ctr + offset).checked_sub(inactive)?;
        if y < timing.v_active_lines {
            Some(y)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum DviTimingLineState {
    #[default]
    FrontPorch,
    Sync,
    BackPorch,
    Active,
}

const DVI_SYNC_LANE_CHUNKS: usize = 4;
const DVI_LANE_CHUNKS: usize = 2;

#[derive(Default)]
pub struct DviScanlineDmaList {
    l0: [DmaControlBlock; DVI_SYNC_LANE_CHUNKS],
    l1: [DmaControlBlock; DVI_LANE_CHUNKS],
    l2: [DmaControlBlock; DVI_LANE_CHUNKS],
}

impl DviScanlineDmaList {
    pub fn lane(&self, i: usize) -> &[DmaControlBlock] {
        match i {
            0 => &self.l0,
            1 => &self.l1,
            _ => &self.l2,
        }
    }

    fn lane_mut(&mut self, i: usize) -> &mut [DmaControlBlock] {
        match i {
            0 => &mut self.l0,
            1 => &mut self.l1,
            _ => &mut self.l2,
        }
    }

    fn setup_lane_0<Ch0, Ch1>(
        &mut self,
        timing: &DviTiming,
        dma_cfg: &DviLaneDmaCfg<Ch0, Ch1>,
        line_state: DviTimingLineState,
        has_data: bool,
    ) where
        Ch0: SingleChannel,
        Ch1: SingleChannel,
    {
        let vsync = (line_state == DviTimingLineState::Sync) == timing.v_sync_polarity;
        let symbol_hsync_off = get_ctrl_symbol(vsync, !timing.h_sync_polarity);
        let symbol_hsync_on = get_ctrl_symbol(vsync, timing.h_sync_polarity);
        let lane = &mut self.l0;
        lane[0].set(
            symbol_hsync_off,
            dma_cfg,
            timing.h_front_porch / 2,
            2,
            false,
        );
        lane[1].set(symbol_hsync_on, dma_cfg, timing.h_sync_width / 2, 2, false);
        lane[2].set(symbol_hsync_off, dma_cfg, timing.h_back_porch / 2, 2, true);
        let read_ring = if has_data { 0 } else { 2 };
        let symbol = match line_state {
            DviTimingLineState::Active => &EMPTY_SCANLINE_TMDS[0],
            _ => symbol_hsync_off,
        };
        lane[3].set(
            symbol,
            dma_cfg,
            timing.h_active_pixels / 2,
            read_ring,
            false,
        );
    }

    fn setup_lane_12<Ch0, Ch1>(
        &mut self,
        lane_number: usize,
        timing: &DviTiming,
        dma_cfg: &DviLaneDmaCfg<Ch0, Ch1>,
        line_state: DviTimingLineState,
        has_data: bool,
    ) where
        Ch0: SingleChannel,
        Ch1: SingleChannel,
    {
        let symbol_no_sync = get_ctrl_symbol(false, false);

        let lane = self.lane_mut(lane_number);
        let inactive = timing.h_front_porch + timing.h_sync_width + timing.h_back_porch;
        lane[0].set(symbol_no_sync, dma_cfg, inactive / 2, 2, false);
        let read_ring = if has_data { 0 } else { 2 };
        let sym = match line_state {
            DviTimingLineState::Active => &EMPTY_SCANLINE_TMDS[lane_number],
            _ => symbol_no_sync,
        };
        lane[1].set(sym, dma_cfg, timing.h_active_pixels / 2, read_ring, false);
    }

    pub fn setup_scanline<Ch0, Ch1, Ch2, Ch3, Ch4, Ch5>(
        &mut self,
        t: &DviTiming,
        dma_cfg: &DmaChannels<Ch0, Ch1, Ch2, Ch3, Ch4, Ch5>,
        line_state: DviTimingLineState,
        has_data: bool,
    ) where
        Ch0: SingleChannel,
        Ch1: SingleChannel,
        Ch2: SingleChannel,
        Ch3: SingleChannel,
        Ch4: SingleChannel,
        Ch5: SingleChannel,
    {
        self.setup_lane_0(t, &dma_cfg.lane0, line_state, has_data);
        self.setup_lane_12(1, t, &dma_cfg.lane1, line_state, has_data);
        self.setup_lane_12(2, t, &dma_cfg.lane2, line_state, has_data);
    }

    pub fn update_scanline(&mut self, buf: *const TmdsPair, stride: u32) {
        unsafe {
            self.l0[3].update_buf(buf);
            self.l1[1].update_buf(buf.add(stride as usize));
            self.l2[1].update_buf(buf.add(stride as usize * 2));
        }
    }
}

#[link_section = ".data"]
static DVI_CTRL_SYMBOLS: [TmdsPair; 4] = [
    TmdsPair::double(TmdsSymbol::C0),
    TmdsPair::double(TmdsSymbol::C1),
    TmdsPair::double(TmdsSymbol::C2),
    TmdsPair::double(TmdsSymbol::C3),
];

fn get_ctrl_symbol(vsync: bool, hsync: bool) -> &'static TmdsPair {
    &DVI_CTRL_SYMBOLS[((vsync as usize) << 1) | (hsync as usize)]
}

#[link_section = ".data"]
static EMPTY_SCANLINE_TMDS: [TmdsPair; 3] = [
    TmdsPair::encode_balanced_approx(0x00), // Blue
    TmdsPair::encode_balanced_approx(0x00), // Green
    TmdsPair::encode_balanced_approx(0xff), // Red
];
