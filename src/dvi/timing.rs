//! timing information yoinked from
//! <https://github.com/Wren6991/PicoDVI/blob/51237271437e9d1eb62c97e40171fbf6ffe01ac6/software/libdvi/dvi_timing.c>

use fugit::KilohertzU32;

use super::{hstx_cmd_raw, hstx_cmd_raw_repeat};

// Perhaps there should be a trait with associated constants for resolution,
// to allow compile-time allocation of scanline buffers etc.
pub struct DviTiming {
    pub h_sync_polarity: bool,
    pub h_front_porch: u32,
    pub h_sync_width: u32,
    pub h_back_porch: u32,
    pub h_active_pixels: u32,

    pub v_sync_polarity: bool,
    pub v_front_porch: u32,
    pub v_sync_width: u32,
    pub v_back_porch: u32,
    pub v_active_lines: u32,

    pub bit_clk: KilohertzU32,
}

// Number of trailing sync words to encode as raw
const SYNC_TRAILING_RAW: usize = 8;
pub const SYNC_LINE_WORDS: usize = 7 + SYNC_TRAILING_RAW;

impl DviTiming {
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

    pub fn tmds3_for_sync(&self, h_sync: bool, v_sync: bool) -> u32 {
        const TMDS_CTRL: [u32; 4] = [0x354, 0xab, 0x154, 0x2ab];
        let h_bit = (h_sync == self.h_sync_polarity) as usize;
        let v_bit = (v_sync == self.v_sync_polarity) as usize;
        let tmds_lane_0 = TMDS_CTRL[h_bit + (v_bit << 1)];
        let tmds_lane_1 = TMDS_CTRL[0];
        let tmds_lane_2 = TMDS_CTRL[0];
        tmds_lane_0 | (tmds_lane_1 << 10) | (tmds_lane_2 << 20)
    }

    pub fn make_sync_line(&self, v_sync: bool) -> [u32; SYNC_LINE_WORDS] {
        let h_sync_off = self.tmds3_for_sync(false, v_sync);
        let mut line = [h_sync_off; SYNC_LINE_WORDS];
        line[0] = hstx_cmd_raw_repeat(self.h_front_porch);
        // line[1] is already h_sync_off
        line[2] = h_sync_off;
        line[2] = hstx_cmd_raw_repeat(self.h_sync_width);
        line[3] = self.tmds3_for_sync(true, v_sync);
        const TAIL: u32 = SYNC_TRAILING_RAW as u32;
        line[4] = hstx_cmd_raw_repeat(self.h_back_porch + self.h_active_pixels - TAIL);
        // line[5] is already h_sync_off
        line[6] = hstx_cmd_raw(TAIL);
        line
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

pub struct DviTimingState {
    v_ctr: u32,
}

impl DviTimingState {
    pub fn new(init_value: u32) -> Self {
        DviTimingState { v_ctr: init_value }
    }

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
