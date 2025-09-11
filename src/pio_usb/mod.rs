use cortex_m::peripheral::SYST;
use embedded_hal::{
    delay::DelayNs,
    digital::{InputPin, OutputPin},
};
use pio::Instruction;
use rp235x_hal::{
    gpio::{
        bank0::{Gpio1, Gpio2},
        FunctionNull, FunctionPio0, Pin, PinState, Pins, PullDown, ValidFunction,
    },
    pac::{Peripherals, PIO0, RESETS},
    pio::{Buffers, PIOExt, Rx, StateMachine, Stopped, Tx, ValidStateMachine, SM0, SM1},
    timer::TimerDevice,
    Timer,
};

// Very low level PIO functions

#[inline]
unsafe fn write_bitmask_set(register: *mut u32, bits: u32) {
    let alias = (register as usize + 0x2000) as *mut u32;
    core::ptr::write_volatile(alias, bits);
}

#[inline]
unsafe fn write_bitmask_clear(register: *mut u32, bits: u32) {
    let alias = (register as usize + 0x3000) as *mut u32;
    core::ptr::write_volatile(alias, bits);
}

#[inline]
unsafe fn pio_sm_start<S: ValidStateMachine, State>(_sm: &StateMachine<S, State>) {
    let pio_addr = 0x50200000 + S::PIO::id() * 0x100000;
    //write_bitmask_set(pio_addr as *mut u32, 1 << S::id());
    // HACK: start sm0 and sm1
    write_bitmask_set(pio_addr as *mut u32, 3);
}

#[inline]
unsafe fn pio_sm_stop<S: ValidStateMachine, State>(_sm: &StateMachine<S, State>) {
    let pio_addr = 0x50200000 + S::PIO::id() * 0x100000;
    write_bitmask_clear(pio_addr as *mut u32, 1 << S::id());
}

const TX_IDLE_ADDRESS: u32 = 4;

struct UsbPio<PIO: PIOExt> {
    tx_sm: StateMachine<(PIO, SM0), Stopped>,
    tx: Tx<(PIO, SM0)>,
    rx_sm: StateMachine<(PIO, SM1), Stopped>,
    rx: Rx<(PIO, SM1)>,
}

impl<PIO: PIOExt> UsbPio<PIO> {
    // TODO: make more pin-agile
    fn new(
        pio: PIO,
        dp: Pin<Gpio1, FunctionNull, PullDown>,
        dm: Pin<Gpio2, FunctionNull, PullDown>,
        resets: &mut RESETS,
    ) -> Self
    where
        Gpio1: ValidFunction<PIO::PinFunction>,
        Gpio2: ValidFunction<PIO::PinFunction>,
    {
        //
        let mut dp: Pin<_, PIO::PinFunction, _> = dp.into_function();
        let mut dm: Pin<_, PIO::PinFunction, _> = dm.into_function();
        dp.set_input_override(rp235x_hal::gpio::InputOverride::Invert);
        dm.set_input_override(rp235x_hal::gpio::InputOverride::Invert);
        let (mut pio, sm0, sm1, _, _) = pio.split(resets);
        let usb_tx_program = pio_proc::pio_file!("src/pio_usb/usb_tx.pio");
        let tx_installed = pio.install(&usb_tx_program.program).unwrap();
        let (tx_sm, _, tx) = rp235x_hal::pio::PIOBuilder::from_installed_program(tx_installed)
            .set_pins(dp.id().num, 2)
            .out_pins(dp.id().num, 2)
            .in_pin_base(dp.id().num)
            .clock_divisor_fixed_point(1, 0)
            .pull_threshold(8)
            .autopull(true)
            .buffers(Buffers::OnlyTx)
            .build(sm0);
        let rx_program = pio_proc::pio_file!("src/pio_usb/usb_rx.pio");
        let rx_installed = pio.install(&rx_program.program).unwrap();
        let (rx_sm, rx, _) = rp235x_hal::pio::PIOBuilder::from_installed_program(rx_installed)
            .in_pin_base(dp.id().num)
            .jmp_pin(dm.id().num)
            .clock_divisor_fixed_point(1, 0)
            .push_threshold(32)
            .autopush(true)
            .buffers(Buffers::OnlyRx)
            .build(sm1);
        Self {
            tx_sm,
            tx,
            rx_sm,
            rx,
        }
    }

    #[link_section = ".data"]
    fn setup_tx(&mut self, n_bytes: usize) {
        // Prime the state machine for transmit
        const LINE_STATE_J: u8 = 1;
        self.tx_sm.exec_instruction(Instruction {
            operands: pio::InstructionOperands::SET {
                destination: pio::SetDestination::PINS,
                data: LINE_STATE_J,
            },
            delay: 0,
            side_set: None,
        });
        self.tx_sm.exec_instruction(Instruction {
            operands: pio::InstructionOperands::SET {
                destination: pio::SetDestination::PINDIRS,
                data: 3,
            },
            delay: 0,
            side_set: None,
        });
        self.tx_sm.exec_instruction(Instruction {
            operands: pio::InstructionOperands::OUT {
                destination: pio::OutDestination::X,
                bit_count: 32,
            },
            delay: 0,
            side_set: None,
        });
        self.tx.write(n_bytes as u32 * 8);
        self.tx_sm.exec_instruction(Instruction {
            operands: pio::InstructionOperands::OUT {
                destination: pio::OutDestination::PC,
                bit_count: 1,
            },
            delay: 0,
            side_set: None,
        });
    }

    #[link_section = ".data"]
    fn tx_handshake(&mut self, pid: u8) {
        self.setup_tx(2);
        self.tx.write(0x80);
        self.tx.write(pid as u32);
        self.tx.write(0xff);
        unsafe {
            pio_sm_start(&self.tx_sm);
        }
        while self.tx_sm.instruction_address() != TX_IDLE_ADDRESS {}
        unsafe {
            pio_sm_stop(&self.tx_sm);
        }
    }

    #[link_section = ".data"]
    fn tx_packet(&mut self, packet: &[u8]) {
        self.setup_tx(packet.len());
        let mut i = 0;
        while i < packet.len() {
            if self.tx.write(packet[i] as u32) {
                i += 1;
            } else {
                break;
            }
        }
        unsafe {
            pio_sm_start(&self.tx_sm);
        }
        while i < packet.len() {
            if self.tx.write(packet[i] as u32) {
                i += 1;
            }
        }
        while !self.tx.write(0xff) {}
        while self.tx_sm.instruction_address() != TX_IDLE_ADDRESS {}
        unsafe {
            pio_sm_stop(&self.tx_sm);
        }
    }

    fn prime_rx(&mut self) {
        // still hacky and for debugging
        const LINE_STATE_J: u8 = 1;
        self.tx_sm.exec_instruction(Instruction {
            operands: pio::InstructionOperands::SET {
                destination: pio::SetDestination::PINS,
                data: LINE_STATE_J,
            },
            delay: 0,
            side_set: None,
        });
        self.tx_sm.exec_instruction(Instruction {
            operands: pio::InstructionOperands::SET {
                destination: pio::SetDestination::PINDIRS,
                data: 3,
            },
            delay: 0,
            side_set: None,
        });

        self.rx_sm.exec_instruction(Instruction {
            operands: pio::InstructionOperands::WAIT {
                polarity: 0,
                source: pio::WaitSource::PIN,
                index: 0,
                relative: false,
            },
            delay: 0,
            side_set: None,
        });
    }

    // TODO: transmission with crc
}

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

    let usb_host_data_plus = usb_host_data_plus.into_pull_down_disabled();
    let usb_host_data_minus = usb_host_data_minus.into_pull_down_disabled();
    let mut resets = unsafe { Peripherals::steal().RESETS };
    let mut usb_pio = UsbPio::new(pio, usb_host_data_plus, usb_host_data_minus, &mut resets);

    usb_pio.prime_rx();
    usb_pio.tx_handshake(0xff);

    let mut a = [0u32; 16];
    let mut i = 0;
    loop {
        if let Some(d) = usb_pio.rx.read() {
            a[i] = d;
            i += 1;
            if i == a.len() {
                break;
            }
        }
    }
    for d in a {
        let mut s = alloc::string::String::new();
        for i in 0..16 {
            let sym = (!d >> (i * 2)) & 3;
            s.push(['0', 'J', 'K', '1'][sym as usize]);
            if i % 4 == 3 {
                s.push(' ');
            }
        }
        console!("data: {s} {d:x}");
    }
}
