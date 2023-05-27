#![no_std]
#![no_main]

use core::{arch::global_asm, cell::RefCell};

use critical_section::Mutex;
use defmt_rtt as _;
use panic_probe as _; // TODO: remove if you need 5kb of space, since panicking + formatting machinery is huge

use cortex_m::delay::Delay;
use defmt::{dbg, info};
use embedded_hal::digital::v2::OutputPin;
use rp_pico::{
    hal::{
        dma::{Channel, DMAExt, CH0, CH1, CH2, CH3, CH4, CH5},
        gpio::PinState,
        pwm,
        sio::Sio,
        watchdog::Watchdog,
        Clock,
    },
    pac, Pins,
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
//mod framebuffer;
mod link;

global_asm! {
    "
    .section .text
    .align 4
    data_cpy_table:
     .word _scratch_x_source
     .word _scratch_x_start
     .word _scratch_x_end
     .word _scratch_y_source
     .word _scratch_y_start
     .word _scratch_y_end
     .word 0
    .global __pre_init
    .type __pre_init,%function
    .thumb_func
    __pre_init:
     push {{r4, lr}}
     ldr r4, =data_cpy_table

    1:
     ldmia r4!, {{r1-r3}}
     cmp r1, #0
     beq 2f
     bl data_cpy
     b 1b
    2:
     pop {{r4, pc}}
     data_cpy_loop:
     ldm r1!, {{r0}}
     stm r2!, {{r0}}
     data_cpy:
     cmp r2, r3
     blo data_cpy_loop
     bx lr
    "
}

// TODO: iterate on this
static DVI_INST: Mutex<
    RefCell<
        Option<
            DviInst<
                Channel<CH0>,
                Channel<CH1>,
                Channel<CH2>,
                Channel<CH3>,
                Channel<CH4>,
                Channel<CH5>,
            >,
        >,
    >,
> = Mutex::new(RefCell::new(None));

// Separate macro annotated function to make rust-analyzer fixes apply better
#[rp_pico::entry]
fn macro_entry() -> ! {
    entry();
}

fn entry() -> ! {
    info!("Program start");

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
        timing.bit_clk_khz,
    );

    let pins = Pins::new(
        peripherals.IO_BANK0,
        peripherals.PADS_BANK0,
        single_cycle_io.gpio_bank0,
        &mut peripherals.RESETS,
    );

    let mut led_pin = pins.gpio16.into_push_pull_output_in_state(PinState::Low);

    let mut delay = Delay::new(
        core_peripherals.SYST,
        dbg!(clocks.system_clock.freq().to_Hz()),
    );

    let pwm_slices = pwm::Slices::new(peripherals.PWM, &mut peripherals.RESETS);
    let dma = peripherals.DMA.split(&mut peripherals.RESETS);

    let mut serializer = DviSerializer::new(
        peripherals.PIO0,
        &mut peripherals.RESETS,
        DviDataPins {
            red_pos: pins.gpio10,
            red_neg: pins.gpio11,
            green_pos: pins.gpio12,
            green_neg: pins.gpio13,
            blue_pos: pins.gpio14,
            blue_neg: pins.gpio15,
        },
        DviClockPins {
            clock_pos: pins.gpio8,
            clock_neg: pins.gpio9,
            pwm_slice: pwm_slices.pwm4,
        },
    );

    let dma_channels = DmaChannels::new(
        dma.ch0,
        dma.ch1,
        dma.ch2,
        dma.ch3,
        dma.ch4,
        dma.ch5,
        serializer.tx0(),
        serializer.tx1(),
        serializer.tx2(),
    );

    let mut inst = DviInst::new(timing, dma_channels);

    critical_section::with(|cs| {
        inst.setup_dma();
        inst.start();
        serializer.enable();
        DVI_INST.borrow(cs).replace(Some(inst));
    });

    rom();
    ram();
    ram_x();
    ram_y();

    //unsafe { dbg!(&framebuffer::FRAMEBUFFER_16BPP as *const _) };
    //unsafe { dbg!(&framebuffer::FRAMEBUFFER_8BPP as *const _) };

    loop {
        info!("high");
        led_pin.set_high().unwrap();
        delay.delay_ms(500);
        info!("low");
        led_pin.set_low().unwrap();
        delay.delay_ms(500);
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
