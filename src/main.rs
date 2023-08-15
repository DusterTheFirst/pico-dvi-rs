#![no_std]
#![no_main]

extern crate alloc;

use core::{arch::global_asm, cell::UnsafeCell, mem::MaybeUninit};

use defmt_rtt as _;
use dvi::dma::DmaChannelList;
use panic_probe as _; // TODO: remove if you need 5kb of space, since panicking + formatting machinery is huge

use cortex_m::{delay::Delay, peripheral::NVIC};
use defmt::{dbg, info};
use embedded_alloc::Heap;
use embedded_hal::digital::v2::ToggleableOutputPin;
use rp_pico::{
    hal::{
        dma::{Channel, DMAExt, CH0, CH1, CH2, CH3, CH4, CH5},
        gpio::PinState,
        pwm,
        sio::Sio,
        watchdog::Watchdog,
        Clock,
    },
    pac::{self, Interrupt},
    Pins,
};

use crate::{
    clock::init_clocks,
    dvi::{
        dma::DmaChannels,
        serializer::{DviClockPins, DviDataPins, DviSerializer},
        timing::VGA_TIMING,
        DviInst,
    },
};

mod clock;
mod dvi;
mod link;
mod render;
mod scanlist;

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
unsafe impl Sync for DviInstWrapper {}

static DVI_INST: DviInstWrapper = DviInstWrapper(UnsafeCell::new(MaybeUninit::uninit()));

// Separate macro annotated function to make rust-analyzer fixes apply better
#[rp_pico::entry]
fn macro_entry() -> ! {
    entry();
}

fn entry() -> ! {
    info!("Program start");
    {
        const HEAP_SIZE: usize = 64 * 1024;
        static mut HEAP_MEM: [MaybeUninit<u8>; HEAP_SIZE] = [MaybeUninit::uninit(); HEAP_SIZE];
        unsafe { HEAP.init(HEAP_MEM.as_ptr() as usize, HEAP_SIZE) }
    }

    let mut peripherals = pac::Peripherals::take().unwrap();
    let core_peripherals = pac::CorePeripherals::take().unwrap();

    sysinfo(&peripherals.SYSINFO);

    let mut watchdog = Watchdog::new(peripherals.WATCHDOG);
    let single_cycle_io = Sio::new(peripherals.SIO);

    let timing = VGA_TIMING;

    // External high-speed crystal on the pico board is 12Mhz
    let clocks = init_clocks(
        peripherals.XOSC,
        peripherals.ROSC,
        peripherals.CLOCKS,
        peripherals.PLL_SYS,
        peripherals.PLL_USB,
        &mut peripherals.RESETS,
        &mut watchdog,
        timing.bit_clk,
    );

    let pins = Pins::new(
        peripherals.IO_BANK0,
        peripherals.PADS_BANK0,
        single_cycle_io.gpio_bank0,
        &mut peripherals.RESETS,
    );

    let mut led_pin = pins.led.into_push_pull_output_in_state(PinState::Low);

    let mut delay = Delay::new(
        core_peripherals.SYST,
        dbg!(clocks.system_clock.freq().to_Hz()),
    );

    let pwm_slices = pwm::Slices::new(peripherals.PWM, &mut peripherals.RESETS);
    let dma = peripherals.DMA.split(&mut peripherals.RESETS);

    let (data_pins, clock_pins) = {
        (
            DviDataPins {
                // 0
                blue_pos: pins.gpio12.into_mode(),
                blue_neg: pins.gpio13.into_mode(),
                // 1
                green_pos: pins.gpio10.into_mode(),
                green_neg: pins.gpio11.into_mode(),
                // 2
                red_pos: pins.gpio16.into_mode(),
                red_neg: pins.gpio17.into_mode(),
            },
            DviClockPins {
                clock_pos: pins.gpio14,
                clock_neg: pins.gpio15,
                pwm_slice: pwm_slices.pwm7,
            },
        )
    };

    let mut serializer = DviSerializer::new(
        peripherals.PIO0,
        &mut peripherals.RESETS,
        data_pins,
        clock_pins,
    );

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
    // Safety: we pass ownership of DVI_INST to the DMA_IRQ_0 handler.
    // For this to be safe, no references to DVI_INST can be used after this unmask
    unsafe {
        NVIC::unmask(Interrupt::DMA_IRQ_0);
    }
    serializer.wait_fifos_full();
    serializer.enable();

    rom();
    ram();
    ram_x();
    ram_y();

    //unsafe { dbg!(&framebuffer::FRAMEBUFFER_16BPP as *const _) };
    //unsafe { dbg!(&framebuffer::FRAMEBUFFER_8BPP as *const _) };

    loop {
        led_pin.toggle().unwrap();
        delay.delay_ms(250);
    }
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
