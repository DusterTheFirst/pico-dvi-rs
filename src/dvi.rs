pub mod dma;
pub mod encode;
pub mod serializer;
pub mod timing;
pub mod tmds;

use rp_pico::hal::{
    dma::SingleChannel,
    gpio::{bank0, PinId},
    pio,
    pwm::{self, ValidPwmOutputPin},
};

use crate::pac::interrupt;

use self::{
    dma::DmaChannels,
    serializer::DviSerializer,
    timing::{DviScanlineDmaList, DviTiming, DviTimingLineState},
};

pub struct Dvi<
    PIO,
    SliceId,
    Pos,
    Neg,
    RedPos,
    RedNeg,
    GreenPos,
    GreenNeg,
    BluePos,
    BlueNeg,
    Ch0,
    Ch1,
    Ch2,
    Ch3,
    Ch4,
    Ch5,
> where
    PIO: pio::PIOExt,
    SliceId: pwm::SliceId,
    Pos: PinId + bank0::BankPinId + ValidPwmOutputPin<SliceId, pwm::A>,
    Neg: PinId + bank0::BankPinId + ValidPwmOutputPin<SliceId, pwm::B>,
    RedPos: PinId,
    RedNeg: PinId,
    GreenPos: PinId,
    GreenNeg: PinId,
    BluePos: PinId,
    BlueNeg: PinId,
    Ch0: SingleChannel,
    Ch1: SingleChannel,
    Ch2: SingleChannel,
    Ch3: SingleChannel,
    Ch4: SingleChannel,
    Ch5: SingleChannel,
{
    timing: DviTiming,
    serializer:
        DviSerializer<PIO, SliceId, Pos, Neg, RedPos, RedNeg, GreenPos, GreenNeg, BluePos, BlueNeg>,
    dma_channels: DmaChannels<Ch0, Ch1, Ch2, Ch3, Ch4, Ch5>,

    dma_list_vblank_sync: DviScanlineDmaList,
    dma_list_vblank_nosync: DviScanlineDmaList,
    // TODO: active
    dma_list_error: DviScanlineDmaList,
}

impl<
        PIO,
        SliceId,
        Pos,
        Neg,
        RedPos,
        RedNeg,
        GreenPos,
        GreenNeg,
        BluePos,
        BlueNeg,
        Ch0,
        Ch1,
        Ch2,
        Ch3,
        Ch4,
        Ch5,
    >
    Dvi<
        PIO,
        SliceId,
        Pos,
        Neg,
        RedPos,
        RedNeg,
        GreenPos,
        GreenNeg,
        BluePos,
        BlueNeg,
        Ch0,
        Ch1,
        Ch2,
        Ch3,
        Ch4,
        Ch5,
    >
where
    PIO: pio::PIOExt,
    SliceId: pwm::SliceId,
    Pos: PinId + bank0::BankPinId + ValidPwmOutputPin<SliceId, pwm::A>,
    Neg: PinId + bank0::BankPinId + ValidPwmOutputPin<SliceId, pwm::B>,
    RedPos: PinId,
    RedNeg: PinId,
    GreenPos: PinId,
    GreenNeg: PinId,
    BluePos: PinId,
    BlueNeg: PinId,
    Ch0: SingleChannel,
    Ch1: SingleChannel,
    Ch2: SingleChannel,
    Ch3: SingleChannel,
    Ch4: SingleChannel,
    Ch5: SingleChannel,
{
    pub fn new(
        timing: DviTiming,
        serializer: DviSerializer<
            PIO,
            SliceId,
            Pos,
            Neg,
            RedPos,
            RedNeg,
            GreenPos,
            GreenNeg,
            BluePos,
            BlueNeg,
        >,
        dma_channels: DmaChannels<Ch0, Ch1, Ch2, Ch3, Ch4, Ch5>,
    ) -> Self {
        let mut dvi = Dvi {
            timing,
            serializer,
            dma_list_vblank_sync: Default::default(),
            dma_list_vblank_nosync: Default::default(),
            dma_list_error: Default::default(),
            dma_channels,
        };

        dvi.dma_list_vblank_sync.setup_scanline(
            &dvi.timing,
            &dvi.dma_channels,
            DviTimingLineState::Sync,
        );
        dvi.dma_list_vblank_nosync.setup_scanline(
            &dvi.timing,
            &dvi.dma_channels,
            DviTimingLineState::FrontPorch,
        );
        dvi.dma_list_error.setup_scanline(
            &dvi.timing,
            &dvi.dma_channels,
            DviTimingLineState::Active,
        );
        dvi
    }

    pub fn start(&mut self) {
        self.dma_channels.load_op(&self.dma_list_vblank_nosync);
        // TODO: start DMA channels
        // TODO: wait for tx fifos full
        self.serializer.enable();
    }
}

#[interrupt]
fn DMA_IRQ_0() {}
