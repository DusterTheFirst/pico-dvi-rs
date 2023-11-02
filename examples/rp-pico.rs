#![no_std]
#![no_main]

use rp_pico::{
    hal::{gpio::PinState, pwm, sio::Sio},
    Pins, XOSC_CRYSTAL_FREQ,
};

use pico_dvi_rs::{
    core0_main,
    dvi::serializer::{DviClockPins, DviDataPins},
};

// Separate macro annotated function to make rust-analyzer fixes apply better
#[rp_pico::entry]
fn macro_entry() -> ! {
    entry();
}

fn entry() -> ! {
    core0_main::<XOSC_CRYSTAL_FREQ, _, _, _, _, _, _, _, _, _, _, _>(
        |sio, io_bank0, pads_bank0, pwm, resets| {
            let single_cycle_io = Sio::new(sio);

            let pins = Pins::new(io_bank0, pads_bank0, single_cycle_io.gpio_bank0, resets);

            let led_pin = pins.led.into_push_pull_output_in_state(PinState::Low);

            let pwm_slices = pwm::Slices::new(pwm, resets);

            (
                led_pin,
                DviDataPins {
                    // 0
                    blue_pos: pins.gpio12.into_function(),
                    blue_neg: pins.gpio13.into_function(),
                    // 1
                    green_pos: pins.gpio10.into_function(),
                    green_neg: pins.gpio11.into_function(),
                    // 2
                    red_pos: pins.gpio16.into_function(),
                    red_neg: pins.gpio17.into_function(),
                },
                DviClockPins {
                    clock_pos: pins.gpio14.into_function(),
                    clock_neg: pins.gpio15.into_function(),
                    pwm_slice: pwm_slices.pwm7,
                },
                single_cycle_io.fifo,
            )
        },
    );
}
