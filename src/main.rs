#![no_std]
#![no_main]

extern crate alloc;

use core::{arch::global_asm, cell::UnsafeCell, mem::MaybeUninit};

use defmt_rtt as _;
use dvi::core1_main;
use panic_probe as _; // TODO: remove if you need 5kb of space, since panicking + formatting machinery is huge

use defmt::info;
use embedded_alloc::Heap;
use hal::multicore::Stack;
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
#[macro_use]
mod console;
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

#[rtic::app(device = crate::hal::pac, dispatchers = [ADC_IRQ_FIFO])]
mod app {
    use crate::console::write_string;
    use crate::dvi;
    use crate::hal::{
        self,
        dma::DMAExt,
        gpio::{bank0::Gpio7, FunctionSio, Pin, PinState, PullDown, SioOutput},
        multicore::Multicore,
        sio::Sio,
        watchdog::Watchdog,
    };
    use core::fmt::Write;
    use core::mem::MaybeUninit;
    use core::pin::pin;
    use core::sync::atomic::AtomicU32;
    use core::sync::atomic::Ordering::Relaxed;
    use cotton_usb_host::host::rp235x::{UsbShared, UsbStatics};
    use cotton_usb_host::usb_bus::{DeviceEvent, HubState, UsbBus, UsbError};
    use defmt::info;
    use futures_util::StreamExt;
    use rtic::RacyCell;
    use rtic_monotonics::rp235x::prelude::Monotonic;
    use rtic_monotonics::rp235x_timer_monotonic;

    #[shared]
    struct Shared {
        shared: &'static UsbShared,
        val: &'static AtomicU32,
    }

    #[local]
    struct Local {
        resets: hal::pac::RESETS,
        led_pin: Pin<Gpio7, FunctionSio<SioOutput>, PullDown>,
        regs: Option<hal::pac::USB>,
        dpram: Option<hal::pac::USB_DPRAM>,
    }

    rp235x_timer_monotonic!(Mono); // comment says 1MHz

    #[init]
    fn init(cx: init::Context) -> (Shared, Local) {
        info!("Program start");

        // Test allocations in different memory regions
        crate::rom();
        crate::ram();
        crate::ram_x();
        crate::ram_y();
        defmt::info!("If we have not panicked by now, memory regions probably work well");

        {
            const HEAP_SIZE: usize = 128 * 1024;
            static mut HEAP_MEM: [MaybeUninit<u8>; HEAP_SIZE] = [MaybeUninit::uninit(); HEAP_SIZE];
            unsafe { crate::HEAP.init(HEAP_MEM.as_ptr() as usize, HEAP_SIZE) }
        }

        let mut peripherals = cx.device;
        let mut resets = peripherals.RESETS;

        crate::sysinfo(&peripherals.SYSINFO);
        let mut watchdog = Watchdog::new(peripherals.WATCHDOG);
        let single_cycle_io = Sio::new(peripherals.SIO);

        let timing = crate::VGA_TIMING;

        // External high-speed crystal on the pico board is 12Mhz
        let _clocks = crate::init_clocks(
            peripherals.XOSC,
            peripherals.ROSC,
            peripherals.CLOCKS,
            peripherals.PLL_SYS,
            peripherals.PLL_USB,
            &mut resets,
            &mut watchdog,
            timing.bit_clk / crate::HSTX_MULTIPLE,
            2 / crate::HSTX_MULTIPLE,
        );
        Mono::start(peripherals.TIMER0, &resets);

        let pins = hal::gpio::Pins::new(
            peripherals.IO_BANK0,
            peripherals.PADS_BANK0,
            single_cycle_io.gpio_bank0,
            &mut resets,
        );

        // LED is pin 7 on Feather 2350 board. We don't have board crates yet for Pico 2
        let led_pin = pins.gpio7.into_push_pull_output_in_state(PinState::Low);
        let gpio_pin = pins.gpio10.into_push_pull_output_in_state(PinState::Low);

        let _dma = peripherals.DMA.split(&mut resets);

        let width = timing.h_active_pixels;

        unsafe {
            (*crate::DVI_INST.0.get()).write(crate::DviInst::new(timing, gpio_pin));
            // Maybe do more safety theater here. The problem is that pins can't
            // set the HSTX function.
            let periphs = hal::pac::Peripherals::steal();
            resets.reset().modify(|_, w| w.hstx().clear_bit());
            while resets.reset_done().read().hstx().bit_is_clear() {}
            use dvi::pinout::DviPair::*;
            // Pinout for Adafruit Feather RP2350
            //let pinout = DviPinout::new([D2, Clk, D1, D0], DviPolarity::Pos);
            // Pinout for Olimex RP2350pc
            let pinout = crate::DviPinout::new([D0, Clk, D2, D1], crate::DviPolarity::Pos);
            dvi::setup_hstx(&periphs.HSTX_CTRL, pinout);
            dvi::setup_dma(&periphs.DMA, &periphs.HSTX_FIFO);
            periphs
                .BUSCTRL
                .bus_priority()
                .write(|w| w.dma_r().set_bit().dma_w().set_bit());
            dvi::setup_pins(&periphs.PADS_BANK0, &periphs.IO_BANK0);
        }

        crate::init_display_swapcell(width);

        let mut fifo = single_cycle_io.fifo;
        let mut mc = Multicore::new(&mut peripherals.PSM, &mut peripherals.PPB, &mut fifo);
        let cores = mc.cores();
        let core1 = &mut cores[1];
        core1
            .spawn(unsafe { crate::CORE1_STACK.take().unwrap() }, move || {
                crate::core1_main()
            })
            .unwrap();

        static VAL: AtomicU32 = AtomicU32::new(0);
        static USB_SHARED: UsbShared = UsbShared::new();

        usb_task::spawn().unwrap();

        (
            Shared {
                val: &VAL,
                shared: &USB_SHARED,
            },
            Local {
                led_pin,
                resets,
                regs: Some(peripherals.USB),
                dpram: Some(peripherals.USB_DPRAM),
            },
        )
    }

    #[idle(local = [led_pin], shared = [&val])]
    fn idle(cx: idle::Context) -> ! {
        crate::console::display_console();
    }

    async fn rtic_delay(ms: usize) {
        Mono::delay(<Mono as rtic_monotonics::Monotonic>::Duration::millis(
            ms as u64,
        ))
        .await
    }

    #[task(local = [resets, regs, dpram], shared = [&shared], priority = 2)]
    async fn usb_task(cx: usb_task::Context) {
        console!("starting usb");
        static USB_STATICS: RacyCell<UsbStatics> = RacyCell::new(UsbStatics::new());
        let statics = unsafe { &mut *USB_STATICS.get_mut() };
        let driver = cotton_usb_host::host::rp235x::Rp235xHostController::new(
            cx.local.resets,
            cx.local.regs.take().unwrap(),
            cx.local.dpram.take().unwrap(),
            cx.shared.shared,
            statics,
        );
        let hub_state = HubState::default();
        let stack = UsbBus::new(driver);
        let mut p = pin!(stack.device_events(&hub_state, rtic_delay));

        loop {
            let device = p.next().await;

            if let Some(DeviceEvent::Connect(device, info)) = device {
                console!("DeviceEvent::Connect(_, _)");
            } else if let Some(DeviceEvent::HubConnect(d)) = device {
                console!("DeviceEvent::HubConnect(addr = {})", d.address());
            } else if let Some(DeviceEvent::Disconnect(bitset)) = device {
                let mut s = alloc::string::String::new();
                for addr in bitset.iter() {
                    let sep = if s.is_empty() { "" } else { ", " };
                    _ = write!(&mut s, "{sep}{addr}");
                }
                console!("DeviceEvent::Disconnect({s})");
            } else if let Some(DeviceEvent::EnumerationError(addr, port, e)) = device {
                console!(
                    "DeviceEvent::EnumerationError({addr}, {port}, {})",
                    format_usb_error(&e)
                );
            } else if let Some(DeviceEvent::None) = device {
                console!("DeviceEvent::None");
            } else if device.is_none() {
                console!("device stream next = None");
                rtic_delay(500).await;
            } else {
                console!("unhandled case (should be unreachable)");
            }
        }
    }

    // we should probably just figure out how to extract strings from defmt
    fn format_usb_error(e: &UsbError) -> &str {
        match e {
            UsbError::Stall => "Stall",
            UsbError::Timeout => "Timeout",
            UsbError::Overflow => "Overflow",
            UsbError::BitStuffError => "BitStuffError",
            UsbError::CrcError => "CrcError",
            UsbError::DataSeqError => "DataSeqError",
            UsbError::BufferTooSmall => "BufferTooSmall",
            UsbError::AllPipesInUse => "AllPipesInUse",
            UsbError::ProtocolError => "ProtocolError",
            UsbError::TooManyDevices => "TooManyDevices",
            UsbError::NoSuchEndpoint => "NoSuchEndPoint",
            _ => "[unhandled UsbError]",
        }
    }

    #[task(binds = USBCTRL_IRQ, shared = [&shared], priority = 2)]
    fn usb_interrupt(cx: usb_interrupt::Context) {
        cx.shared.shared.on_irq();
    }
}

const PALETTE: &[u32; 16] = &[
    0x000000, 0xffffff, 0x9d9d9d, 0xe06f8b, 0xbe2633, 0x493c2b, 0xa46422, 0xeb8931, 0xf7e26b,
    0xa3ce27, 0x44891a, 0x2f484e, 0x1b2632, 0x5784, 0x31a2f2, 0xb2dcef,
];

#[link_section = ".data"]
pub static PALETTE_4BPP: Palette4bppFast = Palette4bppFast::new(PALETTE);

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
