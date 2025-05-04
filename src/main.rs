#![no_std]
#![no_main]

extern crate alloc;

use core::{arch::global_asm, cell::UnsafeCell, mem::MaybeUninit};

use defmt_rtt as _;
use panic_probe as _; // TODO: remove if you need 5kb of space, since panicking + formatting machinery is huge

use cortex_m::peripheral::NVIC;
use cortex_m_rt::interrupt;
use defmt::info;
use embedded_alloc::Heap;
use embedded_hal::{delay::DelayNs, digital::OutputPin};
use hal::{
    dma::{Channel, DMAExt, CH0, CH1, CH2, CH3, CH4, CH5},
    gpio::PinState,
    multicore::{Multicore, Stack},
    pwm,
    sio::{Sio, SioFifo},
    watchdog::Watchdog,
};
use rp235x_hal as hal;

use hal::pac::Interrupt;

use crate::{
    clock::init_clocks,
    dvi::{timing::VGA_TIMING, DviInst},
};

mod clock;
//mod demo;
mod dvi;
mod link;
//mod render;
//mod scanlist;

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

static mut CORE1_STACK: Stack<256> = Stack::new();

static mut FIFO: MaybeUninit<SioFifo> = MaybeUninit::uninit();

// Separate macro annotated function to make rust-analyzer fixes apply better
#[hal::entry]
fn macro_entry() -> ! {
    entry();
}

const PALETTE: &[u32; 16] = &[
    0x000000, 0xffffff, 0x9d9d9d, 0xe06f8b, 0xbe2633, 0x493c2b, 0xa46422, 0xeb8931, 0xf7e26b,
    0xa3ce27, 0x44891a, 0x2f484e, 0x1b2632, 0x5784, 0x31a2f2, 0xb2dcef,
];

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
    let clocks = init_clocks(
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
    let mut led_pin = pins.gpio7.into_push_pull_output_in_state(PinState::Low);

    let pwm_slices = pwm::Slices::new(peripherals.PWM, &mut peripherals.RESETS);
    let dma = peripherals.DMA.split(&mut peripherals.RESETS);

    /*
    {
        // Safety: the DMA_IRQ_0 handler is not enabled yet. We have exclusive access to this static.
        let inst = unsafe { (*DVI_INST.0.get()).write(DviInst::new(timing, dma_channels)) };
        inst.setup_dma();
        inst.start();
    }
    let mut fifo = single_cycle_io.fifo;
    let mut mc = Multicore::new(&mut peripherals.PSM, &mut peripherals.PPB, &mut fifo);
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
        FIFO = MaybeUninit::new(fifo);
        NVIC::unmask(Interrupt::SIO_IRQ_PROC0);
    }
    */

    let mut timer = hal::Timer::new_timer0(peripherals.TIMER0, &mut peripherals.RESETS, &clocks);

    unsafe {
        (*DVI_INST.0.get()).write(DviInst::new(timing));
        // Maybe do more safety theater here. The problem is that pins can't
        // set the HSTX function.
        let periphs = hal::pac::Peripherals::steal();
        periphs.RESETS.reset().modify(|_, w| w.hstx().clear_bit());
        while periphs.RESETS.reset_done().read().hstx().bit_is_clear() {}
        dvi::setup_hstx(&periphs.HSTX_CTRL);
        dvi::setup_dma(&periphs.DMA, &periphs.HSTX_FIFO);
        periphs
            .BUSCTRL
            .bus_priority()
            .write(|w| w.dma_r().set_bit().dma_w().set_bit());
        dvi::setup_pins(&periphs.PADS_BANK0, &periphs.IO_BANK0);
        cortex_m::peripheral::NVIC::unmask(Interrupt::DMA_IRQ_0);
        dvi::start_dma(&periphs.DMA);
    }

    loop {
        led_pin.set_high().unwrap();
        timer.delay_ms(500);
        led_pin.set_low().unwrap();
        timer.delay_ms(500);
    }
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

/*
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
*/

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
