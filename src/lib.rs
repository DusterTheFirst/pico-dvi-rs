#![no_std]
#![no_main]

extern crate alloc;

use core::{arch::global_asm, cell::UnsafeCell, mem::MaybeUninit};

use defmt_rtt as _;
use panic_probe as _; // TODO: remove if you need 5kb of space, since panicking + formatting machinery is huge

use cortex_m::peripheral::NVIC;
use defmt::{dbg, info};
use dvi::dma::DmaChannelList;
use embedded_alloc::Heap;
use rp2040_hal::{
    dma::{Channel, DMAExt, CH0, CH1, CH2, CH3, CH4, CH5},
    gpio::{FunctionSioOutput, Pin, PinId, PullDown},
    multicore::{Multicore, Stack},
    pac::{self, interrupt, Interrupt},
    pwm::{self, ValidPwmOutputPin},
    sio::SioFifo,
    watchdog::Watchdog,
};

use crate::{
    clock::init_clocks,
    dvi::{
        core1_main,
        dma::DmaChannels,
        serializer::{DviClockPins, DviDataPins, DviSerializer},
        timing::VGA_TIMING,
        DviInst,
    },
    render::{init_4bpp_palette, init_display_swapcell, render_line, GLOBAL_PALETTE},
};

pub mod clock;
pub mod demo;
pub mod dvi;
pub mod link;
pub mod render;
pub mod scanlist;

#[global_allocator]
static HEAP: Heap = Heap::empty();

global_asm! {
    include_str!("pre_init.asm"),
    options(raw)
}

struct DviChannels;
impl DmaChannelList for DviChannels {
    type Ch0 = Channel<CH0>;
    type Ch1 = Channel<CH1>;
    type Ch2 = Channel<CH2>;
    type Ch3 = Channel<CH3>;
    type Ch4 = Channel<CH4>;
    type Ch5 = Channel<CH5>;
}

struct DviInstWrapper(UnsafeCell<MaybeUninit<DviInst<DviChannels>>>);

// Safety: access to the instance is indeed shared across threads,
// as it is initialized in the main thread and the interrupt should
// be modeled as another thread (and may be on a different core),
// but only one has access at a time.
//
// Note: this is annoying, `static mut` is more ergonomic (but less
// precise). When `SyncUnsafeCell` is stabilized, use that instead.
unsafe impl Sync for DviInstWrapper {}

static DVI_INST: DviInstWrapper = DviInstWrapper(UnsafeCell::new(MaybeUninit::uninit()));

static mut CORE1_STACK: Stack<256> = Stack::new();

static mut FIFO: MaybeUninit<SioFifo> = MaybeUninit::uninit();

const PALETTE: &[u32] = &[
    0x0, 0xffffff, 0x9d9d9d, 0xe06f8b, 0xbe2633, 0x493c2b, 0xa46422, 0xeb8931, 0xf7e26b, 0xa3ce27,
    0x44891a, 0x2f484e, 0x1b2632, 0x5784, 0x31a2f2, 0xb2dcef,
];

pub fn core0_main<
    const XOSC_CRYSTAL_FREQ: u32,
    F,
    LED,
    RedPos,
    RedNeg,
    GreenPos,
    GreenNeg,
    BluePos,
    BlueNeg,
    SliceId,
    ClockPos,
    ClockNeg,
>(
    pins: F,
) -> !
where
    F: FnOnce(
        pac::SIO,
        pac::IO_BANK0,
        pac::PADS_BANK0,
        pac::PWM,
        &mut pac::RESETS,
    ) -> (
        Pin<LED, FunctionSioOutput, PullDown>,
        DviDataPins<RedPos, RedNeg, GreenPos, GreenNeg, BluePos, BlueNeg>,
        DviClockPins<SliceId, ClockPos, ClockNeg>,
        SioFifo,
    ),
    LED: PinId,
    RedPos: PinId + Send + 'static,
    RedNeg: PinId + Send + 'static,
    GreenPos: PinId + Send + 'static,
    GreenNeg: PinId + Send + 'static,
    BluePos: PinId + Send + 'static,
    BlueNeg: PinId + Send + 'static,
    SliceId: pwm::SliceId + Send + 'static,
    ClockPos: PinId + ValidPwmOutputPin<SliceId, pwm::A> + Send + 'static,
    ClockNeg: PinId + ValidPwmOutputPin<SliceId, pwm::B> + Send + 'static,
{
    info!("Program start");
    {
        const HEAP_SIZE: usize = 128 * 1024;
        static mut HEAP_MEM: [MaybeUninit<u8>; HEAP_SIZE] = [MaybeUninit::uninit(); HEAP_SIZE];
        unsafe { HEAP.init(HEAP_MEM.as_ptr() as usize, HEAP_SIZE) }
    }

    let mut peripherals = pac::Peripherals::take().unwrap();
    //let core_peripherals = pac::CorePeripherals::take().unwrap();

    sysinfo(&peripherals.SYSINFO);

    let mut watchdog = Watchdog::new(peripherals.WATCHDOG);

    let timing = VGA_TIMING;

    // External high-speed crystal on the pico board is 12Mhz
    let _clocks = init_clocks::<XOSC_CRYSTAL_FREQ>(
        peripherals.XOSC,
        peripherals.ROSC,
        peripherals.CLOCKS,
        peripherals.PLL_SYS,
        peripherals.PLL_USB,
        &mut peripherals.RESETS,
        &mut watchdog,
        timing.bit_clk,
    );

    let (led_pin, data_pins, clock_pins, mut sio_fifo) = pins(
        peripherals.SIO,
        peripherals.IO_BANK0,
        peripherals.PADS_BANK0,
        peripherals.PWM,
        &mut peripherals.RESETS,
    );

    let serializer = DviSerializer::new(
        peripherals.PIO0,
        &mut peripherals.RESETS,
        data_pins,
        clock_pins,
    );

    let dma = peripherals.DMA.split(&mut peripherals.RESETS);
    let dma_channels = DmaChannels::new(
        (dma.ch0, dma.ch1, dma.ch2, dma.ch3, dma.ch4, dma.ch5),
        serializer.tx(),
    );

    {
        // Safety: the DMA_IRQ_0 handler is not enabled yet. We have exclusive access to this static.
        let inst = unsafe { (*DVI_INST.0.get()).write(DviInst::new(timing, dma_channels)) };
        inst.setup_dma();
        inst.start();
    }

    let mut mc = Multicore::new(&mut peripherals.PSM, &mut peripherals.PPB, &mut sio_fifo);
    let cores = mc.cores();
    let core1 = &mut cores[1];
    core1
        .spawn(unsafe { &mut CORE1_STACK.mem }, move || {
            core1_main(serializer)
        })
        .unwrap();
    // Safety: enable interrupt for fifo to receive line render requests.
    // Transfer ownership of this end of the fifo to the interrupt handler.
    unsafe {
        FIFO = MaybeUninit::new(sio_fifo);
        NVIC::unmask(Interrupt::SIO_IRQ_PROC0);
    }
    init_display_swapcell(640);

    rom();
    ram();
    ram_x();
    ram_y();

    unsafe {
        init_4bpp_palette(&mut GLOBAL_PALETTE, PALETTE);
    }

    demo::demo(led_pin);
}

fn sysinfo(sysinfo: &pac::SYSINFO) {
    let is_fpga = sysinfo.platform.read().fpga().bit();
    let is_asic = sysinfo.platform.read().asic().bit();
    let git_hash = sysinfo.gitref_rp2040.read().bits();
    let manufacturer = sysinfo.chip_id.read().manufacturer().bits();
    let part = sysinfo.chip_id.read().part().bits();
    let revision = sysinfo.chip_id.read().revision().bits();

    info!(
        "SYSINFO
platform:
    FPGA: {=bool}
    ASIC: {=bool}
gitref_rp2040: {=u32:x}
chip_id:
    manufacturer: {=u16:X}
    part:         {=u16}
    revision:     {=u8}",
        is_fpga, is_asic, git_hash, manufacturer, part, revision
    );
}

// Functions and statics are placed in rom by default
fn rom() {
    dbg!(rom as fn() as *const ());
}

// This function will be placed in ram
#[link_section = link!(ram, ram)]
fn ram() {
    dbg!(ram as fn() as *const ());
}

// This function will be placed in ram
#[link_section = link!(scratch x, ram_x)]
fn ram_x() {
    dbg!(ram_x as fn() as *const ());
}

// This function will be placed in ram
#[link_section = link!(scratch y, ram_y)]
fn ram_y() {
    dbg!(ram_y as fn() as *const ());
}

/// Called by the system only when core 1 is overloaded and can't handle all the rendering work, and requests core 0 to render one scan line worth of content.
#[link_section = ".data"]
#[interrupt]
fn SIO_IRQ_PROC0() {
    // Safety: this interrupt handler has exclusive access to this
    // end of the fifo.
    let fifo = unsafe { FIFO.assume_init_mut() };
    while let Some(line_ix) = fifo.read() {
        // Safety: exclusive access to the line buffer is granted
        // when the render is scheduled to a core.
        unsafe { render_line(line_ix) };
    }
}
