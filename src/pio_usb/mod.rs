use cortex_m::peripheral::SYST;
use embedded_hal::{
    delay::DelayNs,
    digital::{InputPin, OutputPin},
};
use pio::Instruction;
use rp235x_hal::{
    gpio::{FunctionPio0, Pin, PinState, Pins},
    pac::{Peripherals, PIO0},
    pio::{Buffers, PIOExt, PinDir},
    timer::TimerDevice,
    Timer,
};

#[link_section = ".data"]
pub fn do_pio_experiment(pins: Pins, pio: PIO0, mut timer: Timer<impl TimerDevice>) {
    let ticks0 = SYST::get_current();
    let led = pins.gpio29.into_push_pull_output_in_state(PinState::Low);
    let usb_host_5v_power = pins.gpio11.into_push_pull_output_in_state(PinState::High);
    let mut usb_host_data_plus = pins.gpio1.into_push_pull_output_in_state(PinState::High);
    let mut usb_host_data_minus = pins.gpio2.into_push_pull_output_in_state(PinState::Low);
    timer.delay_ms(3);
    _ = usb_host_data_plus.set_low();
    _ = usb_host_data_minus.set_low();
    timer.delay_ms(3);
    _ = usb_host_data_plus.set_high();
    timer.delay_ms(3);
    let mut usb_host_data_plus = usb_host_data_plus.into_pull_down_input();
    let mut usb_host_data_minus = usb_host_data_minus.into_pull_down_input();
    timer.delay_ms(1);
    let dp = usb_host_data_plus.is_high().unwrap();
    let dm = usb_host_data_minus.is_high().unwrap();
    console!("dp = {dp}, dm = {dm} ticks0 = {}", timer.get_counter_low());

    let usb_tx_program = pio_proc::pio_file!("src/pio_usb/usb_tx.pio");
    let mut resets = unsafe { Peripherals::steal().RESETS };
    let (mut pio, sm0, sm1, _, _) = pio.split(&mut resets);

    let installed = pio.install(&usb_tx_program.program).unwrap();
    //let installed = pio.install(&usb_tx_program.program).unwrap();
    let usb_host_data_plus: Pin<_, FunctionPio0, _> = usb_host_data_plus.into_function();
    let usb_host_data_minus: Pin<_, FunctionPio0, _> = usb_host_data_minus.into_function();
    //led.set_input_override(rp235x_hal::gpio::InputOverride::Invert);
    let (mut sm, mut rx0, mut tx) = rp235x_hal::pio::PIOBuilder::from_installed_program(installed)
        .set_pins(usb_host_data_plus.id().num, 2)
        .out_pins(usb_host_data_plus.id().num, 2)
        .in_pin_base(usb_host_data_plus.id().num)
        //.clock_divisor_fixed_point(0, 0)
        .pull_threshold(8)
        .autopull(true)
        .push_threshold(32)
        .autopush(true)
        //.buffers(Buffers::OnlyTx)
        .build(sm0);
    //sm.set_pindirs([(usb_host_data_plus.id().num, PinDir::Output)]);
    sm.exec_instruction(Instruction {
        operands: pio::InstructionOperands::SET {
            destination: pio::SetDestination::PINDIRS,
            data: 3,
        },
        delay: 0,
        side_set: None,
    });
    //sm.set_pins([(usb_host_data_plus.id().num, rp235x_hal::pio::PinState::Low)]);
    sm.exec_instruction(Instruction {
        operands: pio::InstructionOperands::JMP {
            condition: pio::JmpCondition::Always,
            address: 10,
        },
        delay: 0,
        side_set: None,
    });

    let rx_program = pio_proc::pio_file!("src/pio_usb/usb_rx.pio");
    let rx_installed = pio.install(&rx_program.program).unwrap();
    let (rx_sm, mut rx, _) = rp235x_hal::pio::PIOBuilder::from_installed_program(rx_installed)
        .in_pin_base(usb_host_data_plus.id().num)
        //.clock_divisor_fixed_point(0, 0)
        .push_threshold(32)
        .autopush(true)
        .buffers(Buffers::OnlyRx)
        .build(sm1);
    tx.write(16);
    tx.write(0x80);
    tx.write(0xff);
    tx.write(0xff);
    tx.write(0xff);

    sm.with(rx_sm).start();
    let mut a = [0u32; 16];
    let mut i = 0;
    loop {
        if let Some(d) = rx.read() {
            a[i] = d;
            i += 1;
            if i == a.len() {
                break;
            }
        }
    }
    while let Some(d) = rx0.read() {
        console!("rx0 {d:x}");
    }
    for d in a {
        let mut s = alloc::string::String::new();
        for i in 0..16 {
            let sym = (d >> (i * 2)) & 3;
            s.push(['0', 'J', 'K', '1'][sym as usize]);
            if i % 4 == 3 {
                s.push(' ');
            }
        }
        console!("data: {s} {d:x}");
    }
}
