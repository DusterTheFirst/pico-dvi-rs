pub mod dma;
pub mod encode;
pub mod serializer;
pub mod timing;
pub mod tmds;

use rp_pico::hal::dma::SingleChannel;

use crate::{pac::interrupt, DVI_INST};

use self::{
    dma::DmaChannels,
    timing::{DviScanlineDmaList, DviTiming, DviTimingLineState, DviTimingState},
};

/// Dynamic state for DVI output.
///
/// This struct corresponds reasonably closely to `struct dvi_inst` in the
/// PicoDVI source, but with the focused role of holding state needing to
/// be accessed by the interrupt handler.
pub struct DviInst<Ch0, Ch1, Ch2, Ch3, Ch4, Ch5>
where
    Ch0: SingleChannel,
    Ch1: SingleChannel,
    Ch2: SingleChannel,
    Ch3: SingleChannel,
    Ch4: SingleChannel,
    Ch5: SingleChannel,
{
    timing: DviTiming,
    timing_state: DviTimingState,
    channels: DmaChannels<Ch0, Ch1, Ch2, Ch3, Ch4, Ch5>,

    dma_list_vblank_sync: DviScanlineDmaList,
    dma_list_vblank_nosync: DviScanlineDmaList,
    // TODO: active
    dma_list_error: DviScanlineDmaList,
}

impl<Ch0, Ch1, Ch2, Ch3, Ch4, Ch5> DviInst<Ch0, Ch1, Ch2, Ch3, Ch4, Ch5>
where
    Ch0: SingleChannel,
    Ch1: SingleChannel,
    Ch2: SingleChannel,
    Ch3: SingleChannel,
    Ch4: SingleChannel,
    Ch5: SingleChannel,
{
    pub fn new(timing: DviTiming, channels: DmaChannels<Ch0, Ch1, Ch2, Ch3, Ch4, Ch5>) -> Self {
        DviInst {
            timing,
            timing_state: Default::default(),
            channels,
            dma_list_vblank_sync: Default::default(),
            dma_list_vblank_nosync: Default::default(),
            // TODO: active
            dma_list_error: Default::default(),
        }
    }

    pub fn setup_dma(&mut self) {
        self.dma_list_vblank_sync.setup_scanline(
            &self.timing,
            &self.channels,
            DviTimingLineState::Sync,
        );
        self.dma_list_vblank_nosync.setup_scanline(
            &self.timing,
            &self.channels,
            DviTimingLineState::FrontPorch,
        );
        self.dma_list_error.setup_scanline(
            &self.timing,
            &self.channels,
            DviTimingLineState::Active,
        );
    }

    // Note: does not start serializer
    pub fn start(&mut self) {
        self.channels.load_op(&self.dma_list_vblank_nosync);
        self.channels.start();
    }
}

#[link_section = ".data"]
#[interrupt]
fn DMA_IRQ_0() {
    critical_section::with(|cs| {
        let mut guard = DVI_INST.borrow_ref_mut(cs);
        let inst = guard.as_mut().unwrap();
        let _ = inst.channels.check_int();
        inst.timing_state.advance(&inst.timing);
        // wait for all three channels to load their last op
        inst.channels.wait_for_load(inst.timing.horiz_words());
        match inst.timing_state.v_state() {
            DviTimingLineState::Active => {
                inst.channels.load_op(&inst.dma_list_error);
            }
            DviTimingLineState::Sync => inst.channels.load_op(&inst.dma_list_vblank_sync),
            _ => inst.channels.load_op(&inst.dma_list_vblank_nosync),
        }
    })
}
