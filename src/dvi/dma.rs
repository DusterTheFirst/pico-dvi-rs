//! DMA related functions
//!
//! The PicoDVI source does not have a separate file for DMA; it's mostly
//! split between dvi and dvi_timing.

use rp_pico::hal::{
    dma::SingleChannel,
    pio::{Tx, ValidStateMachine},
};

use super::timing::DviScanlineDmaList;

pub struct DviLaneDmaCfg<Ch0, Ch1>
where
    Ch0: SingleChannel,
    Ch1: SingleChannel,
{
    control_channel: Ch0,
    data_channel: Ch1,
    tx_fifo: u32,
    dreq: u8,
}

impl<Ch0, Ch1> DviLaneDmaCfg<Ch0, Ch1>
where
    Ch0: SingleChannel,
    Ch1: SingleChannel,
{
    fn new<SM: ValidStateMachine>(ch0: Ch0, ch1: Ch1, tx: &Tx<SM>) -> Self {
        DviLaneDmaCfg {
            control_channel: ch0,
            data_channel: ch1,
            tx_fifo: tx.fifo_address() as u32,
            dreq: tx.dreq_value(),
        }
    }

    fn load_op(&mut self, cfg: &[DmaControlBlock]) {
        let ch = self.control_channel.ch();
        unsafe {
            ch.ch_read_addr.write(|w| w.bits(cfg.as_ptr() as u32));
            let write_addr = self.data_channel.ch().ch_read_addr.as_ptr();
            ch.ch_write_addr.write(|w| w.bits(write_addr as u32));
            let cfg = DmaChannelConfig::default()
                .chain_to(self.control_channel.id())
                .ring(true, 4)
                .read_increment(true)
                .write_increment(true);
            ch.ch_trans_count.write(|w| w.bits(4));
            ch.ch_al1_ctrl.write(|w| w.bits(cfg.0));
        }
    }

    fn wait_for_load(&self, n_words: u32) {
        unsafe {
            // CH{id}_DBG_TCR register, not exposed by HAL
            let tcr = (0x5000_0804 + 0x40 * self.data_channel.id() as u32) as *mut u32;
            while tcr.read_volatile() != n_words {
                core::hint::spin_loop()
            }
        }
    }
}

// 6 free DMA channels for use by the DVI system
//
// Implement this marker trait on a zero-sized type
// to simplify passing all 6 DMA channels to DMA types.
pub trait DmaChannelList {
    type Ch0: SingleChannel;
    type Ch1: SingleChannel;
    type Ch2: SingleChannel;
    type Ch3: SingleChannel;
    type Ch4: SingleChannel;
    type Ch5: SingleChannel;
}

pub struct DmaChannels<Channels: DmaChannelList> {
    pub lane0: DviLaneDmaCfg<Channels::Ch0, Channels::Ch1>,
    pub lane1: DviLaneDmaCfg<Channels::Ch2, Channels::Ch3>,
    pub lane2: DviLaneDmaCfg<Channels::Ch4, Channels::Ch5>,
}

impl<Channels: DmaChannelList> DmaChannels<Channels> {
    pub fn new<SM0: ValidStateMachine, SM1: ValidStateMachine, SM2: ValidStateMachine>(
        channels: (
            Channels::Ch0,
            Channels::Ch1,
            Channels::Ch2,
            Channels::Ch3,
            Channels::Ch4,
            Channels::Ch5,
        ),
        serializer_tx: (&Tx<SM0>, &Tx<SM1>, &Tx<SM2>),
    ) -> Self {
        DmaChannels {
            lane0: DviLaneDmaCfg::new(channels.0, channels.1, serializer_tx.0),
            lane1: DviLaneDmaCfg::new(channels.2, channels.3, serializer_tx.1),
            lane2: DviLaneDmaCfg::new(channels.4, channels.5, serializer_tx.2),
        }
    }

    pub fn load_op(&mut self, dma_list: &DviScanlineDmaList) {
        self.lane0.load_op(dma_list.lane(0));
        self.lane1.load_op(dma_list.lane(1));
        self.lane2.load_op(dma_list.lane(2));
    }

    /// Enable interrupts and start the DMA transfers
    pub fn start(&mut self) {
        self.lane0.data_channel.enable_irq0();
        let mut mask = 0;
        mask |= 1 << self.lane0.control_channel.id();
        mask |= 1 << self.lane1.control_channel.id();
        mask |= 1 << self.lane2.control_channel.id();
        // TODO: bludgeon rp2040-hal, or whichever crate it is that's supposed to
        // be in charge of such things, into doing this the "right" way.
        unsafe {
            let multi_chan_trigger: *mut u32 = 0x5000_0430 as *mut _;
            multi_chan_trigger.write_volatile(mask);
        }
    }

    pub fn wait_for_load(&self, n_words: u32) {
        self.lane0.wait_for_load(n_words);
        self.lane1.wait_for_load(n_words);
        self.lane2.wait_for_load(n_words);
    }

    pub fn check_int(&mut self) -> bool {
        self.lane0.data_channel.check_irq0()
    }
}

/// DMA control block.
///
/// This is a small chunk of memory transferred by the control DMA channel
/// into the control registers of the data channel.
#[repr(C)]
#[derive(Default)]
pub struct DmaControlBlock {
    read_addr: u32,
    write_addr: u32,
    transfer_count: u32,
    config: DmaChannelConfig,
}

impl DmaControlBlock {
    pub fn set<T, Ch0, Ch1>(
        &mut self,
        read_addr: *const T,
        dma_cfg: &DviLaneDmaCfg<Ch0, Ch1>,
        transfer_count: u32,
        read_ring: u32,
        irq_on_finish: bool,
    ) where
        Ch0: SingleChannel,
        Ch1: SingleChannel,
    {
        self.read_addr = read_addr as u32;
        self.write_addr = dma_cfg.tx_fifo;
        self.transfer_count = transfer_count;
        self.config = DmaChannelConfig::default()
            .ring(false, read_ring)
            .data_request(dma_cfg.dreq)
            .chain_to(dma_cfg.control_channel.id())
            .irq_quiet(!irq_on_finish);
    }

    pub fn update_buf<T>(&mut self, buf: *const T) {
        self.read_addr = buf as u32;
    }
}

// We're doing this by hand because it's not provided by rp2040-pac, as it's
// based on svd2rust (which is quite tight-assed), but would be provided by
// the hal if we were using rp_pac, which is chiptool-based.
//
// Another note: the caller *must* set `chain_to`, as the default points to
// channel zero. Setting it to the same channel disables the function.
#[repr(transparent)]
#[derive(Clone, Copy)]
struct DmaChannelConfig(u32);

impl Default for DmaChannelConfig {
    fn default() -> Self {
        let mut bits = 0;
        bits |= 1 << 0; // enable
        bits |= 2 << 2; // data size = 32 bits
        Self(bits).read_increment(true).data_request(0x3f)
    }
}

impl DmaChannelConfig {
    fn read_increment(self, incr: bool) -> Self {
        let mut bits = self.0 & !(1 << 4);
        bits |= (incr as u32) << 4;
        Self(bits)
    }

    fn write_increment(self, incr: bool) -> Self {
        let mut bits = self.0 & !(1 << 5);
        bits |= (incr as u32) << 5;
        Self(bits)
    }

    fn ring(self, ring_sel: bool, ring_size: u32) -> Self {
        let mut bits = self.0 & !0x7c0;
        bits |= (ring_sel as u32) << 10;
        bits |= ring_size << 6;
        Self(bits)
    }

    fn chain_to(self, chan: u8) -> Self {
        let mut bits = self.0 & !0x7800;
        bits |= (chan as u32) << 11;
        Self(bits)
    }

    fn data_request(self, dreq: u8) -> Self {
        let mut bits = self.0 & !0x1f8000;
        bits |= (dreq as u32) << 15;
        Self(bits)
    }

    fn irq_quiet(self, quiet: bool) -> Self {
        let mut bits = self.0 & !(1 << 21);
        bits |= (quiet as u32) << 21;
        Self(bits)
    }
}
