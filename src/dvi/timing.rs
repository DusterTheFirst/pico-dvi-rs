//! timing information yoinked from
//! <https://github.com/Wren6991/PicoDVI/blob/51237271437e9d1eb62c97e40171fbf6ffe01ac6/software/libdvi/dvi_timing.c>

use fugit::KilohertzU32;

#[cfg(feature = "audio")]
use crate::dvi::data_island::{DataPacket, TERC4_SYMBOLS};

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
pub const SYNC_LINE_ONLY_WORDS: usize = 3 + SYNC_TRAILING_RAW;
pub const SYNC_DATA_ISLAND_LEN: usize = 56;

#[link_section = ".data"]
static TMDS_CTRL: [u32; 4] = [0x354, 0xab, 0x154, 0x2ab];

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
        let h_bit = (h_sync == self.h_sync_polarity) as usize;
        let v_bit = (v_sync == self.v_sync_polarity) as usize;
        let tmds_lane_0 = TMDS_CTRL[h_bit + (v_bit << 1)];
        let tmds_lane_1 = TMDS_CTRL[0];
        let tmds_lane_2 = TMDS_CTRL[0];
        tmds_lane_0 | (tmds_lane_1 << 10) | (tmds_lane_2 << 20)
    }

    pub fn make_sync_pulse(&self, v_sync: bool) -> [u32; SYNC_LINE_WORDS] {
        let h_sync_off = self.tmds3_for_sync(false, v_sync);
        let mut line = [h_sync_off; SYNC_LINE_WORDS];
        line[0] = hstx_cmd_raw_repeat(self.h_front_porch);
        // line[1] is already h_sync_off
        line[2] = hstx_cmd_raw_repeat(self.h_sync_width);
        line[3] = self.tmds3_for_sync(true, v_sync);
        const TAIL: u32 = SYNC_TRAILING_RAW as u32;
        line[4] = hstx_cmd_raw_repeat(self.h_back_porch - TAIL);
        // line[5] is already h_sync_off
        line[6] = hstx_cmd_raw(TAIL);
        line
    }

    pub fn make_sync_line_only(&self, v_sync: bool) -> [u32; SYNC_LINE_ONLY_WORDS] {
        let h_sync_off = self.tmds3_for_sync(false, v_sync);
        let mut line = [h_sync_off; SYNC_LINE_ONLY_WORDS];
        const TAIL: u32 = SYNC_TRAILING_RAW as u32;
        line[0] = hstx_cmd_raw_repeat(self.h_active_pixels - TAIL);
        // line[1] is already h_sync_off
        line[2] = hstx_cmd_raw(TAIL);
        line
    }

    pub fn init_data_island(&self, line: &mut [u32; SYNC_DATA_ISLAND_LEN]) {
        line[0] = hstx_cmd_raw_repeat(self.h_front_porch - 8);
        line[2] = hstx_cmd_raw_repeat(8);
        line[4] = hstx_cmd_raw(36);
        line[41] = hstx_cmd_raw_repeat(self.h_sync_width - 36);
        line[43] = hstx_cmd_raw_repeat(self.h_back_porch - 10);
        line[45] = hstx_cmd_raw(10);
    }

    const VIDEO_GUARD: u32 = 0x2cc | (0x133 << 10) | (0x2cc << 20);
    #[cfg(feature = "audio")]
    #[link_section = ".data"]
    pub fn encode_data_island(
        &self,
        line: &mut [u32; SYNC_DATA_ISLAND_LEN],
        state: DviTimingLineState,
        packet: &DataPacket,
    ) {
        let v_sync = matches!(state, DviTimingLineState::Sync);
        let h_bit = self.h_sync_polarity as u8;
        let v_bit = (v_sync == self.v_sync_polarity) as u8;
        let hv = h_bit + (v_bit << 1);
        packet.encode(hv, &mut line[5..41]);
        let sync_off = self.tmds3_for_sync(false, v_sync);
        let sync_on = self.tmds3_for_sync(true, v_sync);
        const CTRL_MASK: u32 = TMDS_CTRL[0] ^ TMDS_CTRL[1];
        let vid_preamble = sync_off ^ (CTRL_MASK << 10);
        let data_preamble = vid_preamble ^ (CTRL_MASK << 20);
        line[1] = sync_off;
        line[3] = data_preamble;
        line[42] = sync_on;
        line[44] = sync_off;
        match state {
            DviTimingLineState::Active => {
                line[46..54].fill(vid_preamble);
                line[54..56].fill(Self::VIDEO_GUARD);
            }
            _ => {
                line[46..56].fill(sync_off);
            }
        }
    }

    #[cfg(feature = "audio")]
    #[link_section = ".data"]
    pub fn encode_data_island_empty(
        &self,
        line: &mut [u32; SYNC_DATA_ISLAND_LEN],
        state: DviTimingLineState,
    ) {
        let v_sync = matches!(state, DviTimingLineState::Sync);
        let h_bit = self.h_sync_polarity as u8;
        let v_bit = (v_sync == self.v_sync_polarity) as u8;
        let hv = (h_bit + (v_bit << 1)) as usize;
        let gb = TERC4_SYMBOLS[hv + 12] as u32 | (0x133 << 10) | (0x133 << 20);
        line[5..7].fill(gb);
        line[7] = TERC4_SYMBOLS[hv] as u32 | (0x29c << 10) | (0x29c << 20);
        line[8..39].fill(TERC4_SYMBOLS[hv + 8] as u32 | (0x29c << 10) | (0x29c << 20));
        line[39..41].fill(gb);
        let sync_off = self.tmds3_for_sync(false, v_sync);
        let sync_on = self.tmds3_for_sync(true, v_sync);
        const CTRL_MASK: u32 = TMDS_CTRL[0] ^ TMDS_CTRL[1];
        let vid_preamble = sync_off ^ (CTRL_MASK << 10);
        let data_preamble = vid_preamble ^ (CTRL_MASK << 20);
        line[1] = sync_off;
        line[3] = data_preamble;
        line[42] = sync_on;
        line[44] = sync_off;
        match state {
            DviTimingLineState::Active => {
                line[46..54].fill(vid_preamble);
                line[54..56].fill(Self::VIDEO_GUARD);
            }
            _ => {
                line[46..56].fill(sync_off);
            }
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

    pub fn v_ctr(&self) -> u32 {
        self.v_ctr
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
