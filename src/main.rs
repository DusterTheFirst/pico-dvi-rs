#![no_std]
#![no_main]

extern crate alloc;

use core::{arch::global_asm, cell::UnsafeCell, mem::MaybeUninit};

use defmt_rtt as _;
use dvi::core1_main;
use panic_probe as _; // TODO: remove if you need 5kb of space, since panicking + formatting machinery is huge

use defmt::info;
use embedded_alloc::Heap;
use hal::{
    dma::DMAExt,
    gpio::PinState,
    multicore::{Multicore, Stack},
    sio::Sio,
    watchdog::Watchdog,
};
use render::{init_display_swapcell, Palette4bppFast};
use rp235x_hal as hal;

use crate::{
    clock::init_clocks,
    dvi::{
        pinout::{DviPinout, DviPolarity},
        timing::VGA_TIMING,
        DviInst, DviOut,
    },
};

mod clock;
mod demo;
mod dvi;
mod link;
mod render;
mod scanlist;

/// The number of HSTX bits per system clock.
///
/// Ordinarily this is 2 so the system doesn't need to be overclocked, but
/// can be 1 to provide more CPU horsepower per pixel.
const HSTX_MULTIPLE: u32 = 2;

#[global_allocator]
static HEAP: Heap = Heap::empty();

global_asm! {
    include_str!("pre_init.asm"),
    options(raw)
}

/// Tell the Boot ROM about our application
#[unsafe(link_section = ".start_block")]
#[used]
pub static IMAGE_DEF: hal::block::ImageDef = hal::block::ImageDef::secure_exe();

// Perhaps there should be one struct with all this state, and
// multiple MaybeUninit fields.

struct DviInstWrapper(UnsafeCell<MaybeUninit<DviInst>>);

// Safety: access to the instance is indeed shared across threads,
// as it is initialized in the main thread and the interrupt should
// be modeled as another thread (and may be on a different core),
// but only one has access at a time.
//
// Note: this is annoying, `static mut` is more ergonomic (but less
// precise). When `SyncUnsafeCell` is stabilized, use that instead.
unsafe impl Sync for DviInstWrapper {}

static DVI_INST: DviInstWrapper = DviInstWrapper(UnsafeCell::new(MaybeUninit::uninit()));

static DVI_OUT: DviOut = DviOut::new();

static mut CORE1_STACK: Stack<1024> = Stack::new();

// Separate macro annotated function to make rust-analyzer fixes apply better
#[hal::entry]
fn macro_entry() -> ! {
    entry();
}

const PALETTE: &[u32; 16] = &[
    0x000000, 0xffffff, 0x9d9d9d, 0xe06f8b, 0xbe2633, 0x493c2b, 0xa46422, 0xeb8931, 0xf7e26b,
    0xa3ce27, 0x44891a, 0x2f484e, 0x1b2632, 0x5784, 0x31a2f2, 0xb2dcef,
];

#[link_section = ".data"]
pub static PALETTE_4BPP: Palette4bppFast = Palette4bppFast::new(PALETTE);

fn entry() -> ! {
    info!("Program start");

    // Test allocations in different memory regions
    rom();
    ram();
    ram_x();
    ram_y();
    defmt::info!("If we have not panicked by now, memory regions probably work well");

    {
        const HEAP_SIZE: usize = 128 * 1024;
        static mut HEAP_MEM: [MaybeUninit<u8>; HEAP_SIZE] = [MaybeUninit::uninit(); HEAP_SIZE];
        unsafe { HEAP.init(HEAP_MEM.as_ptr() as usize, HEAP_SIZE) }
    }

    let mut peripherals = hal::pac::Peripherals::take().unwrap();
    //let core_peripherals = pac::CorePeripherals::take().unwrap();

    sysinfo(&peripherals.SYSINFO);

    let mut watchdog = Watchdog::new(peripherals.WATCHDOG);
    let single_cycle_io = Sio::new(peripherals.SIO);

    let timing = VGA_TIMING;

    // External high-speed crystal on the pico board is 12Mhz
    let _clocks = init_clocks(
        peripherals.XOSC,
        peripherals.ROSC,
        peripherals.CLOCKS,
        peripherals.PLL_SYS,
        peripherals.PLL_USB,
        &mut peripherals.RESETS,
        &mut watchdog,
        timing.bit_clk / HSTX_MULTIPLE,
        2 / HSTX_MULTIPLE,
    );

    let pins = hal::gpio::Pins::new(
        peripherals.IO_BANK0,
        peripherals.PADS_BANK0,
        single_cycle_io.gpio_bank0,
        &mut peripherals.RESETS,
    );

    // LED is pin 7 on Feather 2350 board. We don't have board crates yet for Pico 2
    let led_pin = pins.gpio7.into_push_pull_output_in_state(PinState::Low);
    let gpio_pin = pins.gpio10.into_push_pull_output_in_state(PinState::Low);

    let _dma = peripherals.DMA.split(&mut peripherals.RESETS);

    let width = timing.h_active_pixels;

    unsafe {
        (*DVI_INST.0.get()).write(DviInst::new(timing, gpio_pin));
        // Maybe do more safety theater here. The problem is that pins can't
        // set the HSTX function.
        let periphs = hal::pac::Peripherals::steal();
        periphs.RESETS.reset().modify(|_, w| w.hstx().clear_bit());
        while periphs.RESETS.reset_done().read().hstx().bit_is_clear() {}
        use dvi::pinout::DviPair::*;
        // Pinout for Adafruit Feather RP2350
        let pinout = DviPinout::new([D2, Clk, D1, D0], DviPolarity::Pos);
        // Pinout for Olimex RP2350pc
        //let pinout = DviPinout::new([D0, Clk, D2, D1], DviPolarity::Pos);
        dvi::setup_hstx(&periphs.HSTX_CTRL, pinout);
        dvi::setup_dma(&periphs.DMA, &periphs.HSTX_FIFO);
        periphs
            .BUSCTRL
            .bus_priority()
            .write(|w| w.dma_r().set_bit().dma_w().set_bit());
        dvi::setup_pins(&periphs.PADS_BANK0, &periphs.IO_BANK0);
    }

    init_display_swapcell(width);

    let mut fifo = single_cycle_io.fifo;
    let mut mc = Multicore::new(&mut peripherals.PSM, &mut peripherals.PPB, &mut fifo);
    let cores = mc.cores();
    let core1 = &mut cores[1];
    core1
        .spawn(unsafe { CORE1_STACK.take().unwrap() }, move || core1_main())
        .unwrap();

    demo::demo(led_pin);
}

fn sysinfo(sysinfo: &hal::pac::SYSINFO) {
    let is_fpga = sysinfo.platform().read().fpga().bit();
    let is_asic = sysinfo.platform().read().asic().bit();
    let git_hash = sysinfo.gitref_rp2350().read().bits();
    let manufacturer = sysinfo.chip_id().read().manufacturer().bits();
    let part = sysinfo.chip_id().read().part().bits();
    let revision = sysinfo.chip_id().read().revision().bits();

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
    let ptr = rom as fn() as *const ();
    defmt::assert!(
        (0x10000100..0x20000000).contains(&(ptr as u32)),
        "rom fn is placed at {} which is not in FLASH",
        ptr
    );
}

// This function will be placed in ram
#[link_section = link!(ram, ram)]
#[inline(never)]
fn ram() {
    let ptr = ram as fn() as *const ();
    defmt::assert!(
        (0x20000000..0x20080000).contains(&(ptr as u32)),
        "ram fn is placed at {} which is not in RAM",
        ptr
    );
}

// This function will be placed in ram
#[link_section = link!(scratch x, ram_x)]
#[inline(never)]
fn ram_x() {
    let ptr = ram_x as fn() as *const ();
    defmt::assert!(
        (0x20080000..0x20081000).contains(&(ptr as u32)),
        "ram_x fn is placed at {} which is not in SRAM4",
        ptr
    );
}

// This function will be placed in ram
#[link_section = link!(scratch y, ram_y)]
#[inline(never)]
fn ram_y() {
    let ptr = ram_y as fn() as *const ();
    defmt::assert!(
        (0x20081000..0x20082000).contains(&(ptr as u32)),
        "ram_y fn is placed at {} which is not in SRAM5",
        ptr
    );
}

/// Program metadata for `picotool info`
#[unsafe(link_section = ".bi_entries")]
#[used]
pub static PICOTOOL_ENTRIES: [hal::binary_info::EntryAddr; 5] = [
    hal::binary_info::rp_cargo_bin_name!(),
    hal::binary_info::rp_cargo_version!(),
    hal::binary_info::rp_program_description!(c"Pico DVI"),
    hal::binary_info::rp_cargo_homepage_url!(),
    hal::binary_info::rp_program_build_attribute!(),
];
