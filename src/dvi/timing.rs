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

struct DviTiming {
    ctr: u32, // FIXME: what is?
    state: DviTimingState,
}

enum DviTimingState {
    FrontPorch,
    Sync,
    BackPorch,
    Active,
    Count,
}
