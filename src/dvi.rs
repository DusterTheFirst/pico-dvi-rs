pub mod dma;
pub mod serializer;
pub mod timing;
pub mod tmds;

use alloc::boxed::Box;

use crate::{pac::interrupt, render::ScanRender, DVI_INST};

use self::{
    dma::{DmaChannelList, DmaChannels},
    timing::{DviScanlineDmaList, DviTiming, DviTimingLineState, DviTimingState},
    tmds::TmdsPair,
};

/// Number of channels rendered.
///
/// This is usually 3 for RGB, but can also be 1 for grayscale, in which case
/// the TMDS buffer is output to all three channels.
pub const N_CHANNELS: usize = 3;
pub const VERTICAL_REPEAT: usize = 2;

/// The additional time (in scanlines) for the TMDS encoding routine.
///
/// If TMDS encoding can reliably happen in less than one scanline time,
/// this should be 0. If there is variance that sometimes pushes it over
/// the line, then a value of 1 may eliminate artifacts.
const TMDS_PIPELINE_SLACK: u32 = 0;

const N_TMDS_BUFFERS: usize = if TMDS_PIPELINE_SLACK > 0 && VERTICAL_REPEAT == 1 {
    3
} else {
    2
};

/// Dynamic state for DVI output.
///
/// This struct corresponds reasonably closely to `struct dvi_inst` in the
/// PicoDVI source, but with the focused role of holding state needing to
/// be accessed by the interrupt handler.
pub struct DviInst<Channels: DmaChannelList> {
    timing: DviTiming,
    timing_state: DviTimingState,
    channels: DmaChannels<Channels>,

    dma_list_vblank_sync: DviScanlineDmaList,
    dma_list_vblank_nosync: DviScanlineDmaList,
    dma_list_active: DviScanlineDmaList,
    dma_list_error: DviScanlineDmaList,

    tmds_buf: Box<[TmdsPair]>,
    scan_render: ScanRender,
}

impl<Channels: DmaChannelList> DviInst<Channels> {
    pub fn new(timing: DviTiming, channels: DmaChannels<Channels>) -> Self {
        let buf_size = timing.horizontal_words() as usize * N_CHANNELS * N_TMDS_BUFFERS;
        let buf = alloc::vec![TmdsPair::encode_balanced_approx(0); buf_size];
        DviInst {
            timing,
            timing_state: Default::default(),
            channels,
            dma_list_vblank_sync: Default::default(),
            dma_list_vblank_nosync: Default::default(),
            dma_list_active: Default::default(),
            dma_list_error: Default::default(),
            tmds_buf: buf.into(),
            scan_render: ScanRender::new(),
        }
    }

    pub fn setup_dma(&mut self) {
        self.dma_list_vblank_sync.setup_scanline(
            &self.timing,
            &self.channels,
            DviTimingLineState::Sync,
            false,
        );
        self.dma_list_vblank_nosync.setup_scanline(
            &self.timing,
            &self.channels,
            DviTimingLineState::FrontPorch,
            false,
        );
        self.dma_list_active.setup_scanline(
            &self.timing,
            &self.channels,
            DviTimingLineState::Active,
            true,
        );
        self.dma_list_error.setup_scanline(
            &self.timing,
            &self.channels,
            DviTimingLineState::Active,
            false,
        );
    }

    // Note: does not start serializer
    pub fn start(&mut self) {
        self.channels.load_op(&self.dma_list_vblank_nosync);
        self.channels.start();
    }

    #[link_section = ".data"]
    fn update_scanline(&mut self) {
        if let Some(y) = self.timing_state.v_scanline_index(&self.timing, 0) {
            let buf_ix = (y as usize / VERTICAL_REPEAT) % N_TMDS_BUFFERS;
            let stride = self.timing.horizontal_words() as usize * N_CHANNELS * buf_ix;
            let buf = unsafe { self.tmds_buf.as_ptr().add(stride) };
            let channel_stride = if N_CHANNELS == 1 {
                0
            } else {
                self.timing.horizontal_words()
            };
            self.dma_list_active.update_scanline(buf, channel_stride);
        }
    }

    #[link_section = ".data"]
    fn render(&mut self) {
        if let Some(y) = self
            .timing_state
            .v_scanline_index(&self.timing, TMDS_PIPELINE_SLACK)
        {
            if y % VERTICAL_REPEAT as u32 == 0 {
                let y = y / VERTICAL_REPEAT as u32;
                let buf_ix = y as usize % N_TMDS_BUFFERS;
                let line_size = self.timing.horizontal_words() as usize * N_CHANNELS;
                let line_start = line_size * buf_ix;
                let tmds_slice = &mut self.tmds_buf[line_start..][..line_size];
                self.scan_render.render_scanline(tmds_slice, y);
            }
        }
    }
}

#[link_section = ".data"]
#[interrupt]
fn DMA_IRQ_0() {
    // Safety: interrupts are enabled (and thus the interrupt handler is
    // called) only after the instance has been initialized. After
    // initialization, the interrupt handles has exclusive access.
    let inst = unsafe { (*DVI_INST.0.get()).assume_init_mut() };
    let _ = inst.channels.check_int();
    inst.timing_state.advance(&inst.timing);
    // wait for all three channels to load their last op
    inst.channels.wait_for_load(inst.timing.horizontal_words());
    inst.update_scanline();
    match inst.timing_state.v_state(&inst.timing) {
        DviTimingLineState::Active => inst.channels.load_op(&inst.dma_list_active),
        DviTimingLineState::Sync => inst.channels.load_op(&inst.dma_list_vblank_sync),
        _ => inst.channels.load_op(&inst.dma_list_vblank_nosync),
    }
    inst.render();
}
