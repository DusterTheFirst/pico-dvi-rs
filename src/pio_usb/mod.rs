mod crc;

use embedded_hal::delay::DelayNs;
use pio::Instruction;
use rp235x_hal::{
    gpio::{
        bank0::{Gpio1, Gpio2},
        FunctionNull, Pin, PinState, Pins, PullDown, ValidFunction,
    },
    pac::{Peripherals, PIO0, RESETS},
    pio::{Buffers, PIOExt, Rx, StateMachine, Stopped, Tx, ValidStateMachine, SM0, SM1},
    timer::TimerDevice,
    Timer,
};

use crate::pio_usb::crc::{calc_usb_crc5, update_crc_16};

// Very low level PIO functions

const SYNC: u8 = 0x80;
const PID_DATA0: u8 = 0xc3;
const PID_SOF: u8 = 0xa5;
const PID_SETUP: u8 = 0x2d;

#[inline]
#[link_section = ".data"]
fn pio_addr<S: ValidStateMachine, State>(_sm: &StateMachine<S, State>) -> *mut u32 {
    (0x50200000 + S::PIO::id() * 0x100000) as *mut u32
}

#[inline]
#[link_section = ".data"]
unsafe fn write_bitmask_set(register: *mut u32, bits: u32) {
    let alias = (register as usize + 0x2000) as *mut u32;
    core::ptr::write_volatile(alias, bits);
}

#[inline]
#[link_section = ".data"]
unsafe fn write_bitmask_clear(register: *mut u32, bits: u32) {
    let alias = (register as usize + 0x3000) as *mut u32;
    core::ptr::write_volatile(alias, bits);
}

#[inline]
#[link_section = ".data"]
unsafe fn pio_sm_start<S: ValidStateMachine, State>(sm: &StateMachine<S, State>) {
    write_bitmask_set(pio_addr(sm), 1 << S::id());
}

#[inline]
#[link_section = ".data"]
unsafe fn pio_sm_restart<S: ValidStateMachine, State>(sm: &StateMachine<S, State>) {
    write_bitmask_set(pio_addr(sm), (1 << 4) << S::id());
}

#[inline]
#[link_section = ".data"]
unsafe fn pio_sm_stop<S: ValidStateMachine, State>(sm: &StateMachine<S, State>) {
    write_bitmask_clear(pio_addr(sm), 1 << S::id());
}

#[inline]
#[link_section = ".data"]
unsafe fn pio_write_instr<PIO: PIOExt>(_pio: &rp235x_hal::pio::PIO<PIO>, n: usize, instr: u16) {
    let pio_addr = 0x50200000 + PIO::id() * 0x100000 + 0x48 + 4 * n;
    core::ptr::write_volatile(pio_addr as *mut u16, instr);
}

#[inline]
#[link_section = ".data"]
unsafe fn pio_sm_exec<S: ValidStateMachine, State>(_sm: &StateMachine<S, State>, instr: u16) {
    let pio_addr = 0x50200000 + S::PIO::id() * 0x100000 + 0xd8 + S::id() * 0x18;
    core::ptr::write_volatile(pio_addr as *mut u16, instr);
}

// TODO: set this to 4 when we reinstate J state
const TX_IDLE_ADDRESS: u32 = 4;

struct UsbPio<PIO: PIOExt> {
    tx_sm: StateMachine<(PIO, SM0), Stopped>,
    tx: Tx<(PIO, SM0)>,
    rx_sm: StateMachine<(PIO, SM1), Stopped>,
    rx: Rx<(PIO, SM1)>,
    pio: rp235x_hal::pio::PIO<PIO>,
    eop_irq: i32,
    #[allow(unused)]
    dp: Pin<Gpio1, PIO::PinFunction, PullDown>,
    #[allow(unused)]
    dm: Pin<Gpio2, PIO::PinFunction, PullDown>,
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
            // 132MHz / 48MHz
            .clock_divisor_fixed_point(2, 192)
            .pull_threshold(8)
            .autopull(true)
            .buffers(Buffers::OnlyTx)
            .build(sm0);
        let rx_program = pio_proc::pio_file!("src/pio_usb/usb_rx.pio");
        let eop_irq = rx_program.public_defines.IRQ_RX_EOP;
        let rx_installed = pio.install(&rx_program.program).unwrap();
        let (rx_sm, rx, _) = rp235x_hal::pio::PIOBuilder::from_installed_program(rx_installed)
            .in_pin_base(dp.id().num)
            .jmp_pin(dm.id().num)
            .in_count(1)
            // 132MHz / 120MHz
            .clock_divisor_fixed_point(1, 26)
            .push_threshold(8)
            .autopush(true)
            .buffers(Buffers::OnlyRx)
            .build(sm1);
        Self {
            tx_sm,
            tx,
            rx_sm,
            rx,
            pio,
            eop_irq,
            dp,
            dm,
        }
    }

    #[link_section = ".data"]
    fn setup_tx(&mut self, n_bytes: usize) {
        // Prime the state machine for transmit
        unsafe {
            pio_sm_stop(&self.tx_sm);
            pio_write_instr(
                &self.pio, 9, 0x0105, // jmp bit_zero [1]
            );
            pio_sm_exec(&self.tx_sm, 0xe001); // set pins, LINE_STATE_J
            pio_sm_exec(&self.tx_sm, 0xe083); // set pindirs, 3
            pio_sm_restart(&self.tx_sm);
            pio_sm_exec(&self.tx_sm, 0x6020); // out x, 32
            self.tx.write(n_bytes as u32 * 8);
            pio_sm_exec(&self.tx_sm, 0x60a1); // out pc, 1
        }
    }

    /// Transmit a 2-byte handshake packet.
    ///
    /// This method might go away, subsumed by `tx_packet`.
    #[link_section = ".data"]
    #[allow(unused)]
    fn tx_handshake(&mut self, pid: u8) {
        self.setup_tx(2);
        self.tx.write(SYNC as u32);
        self.tx.write(pid as u32);
        self.tx.write(0xff);
        unsafe {
            pio_sm_start(&self.tx_sm);
        }
        while self.tx_sm.instruction_address() != TX_IDLE_ADDRESS {}
    }

    #[link_section = ".data"]
    fn tx_with_crc5(&mut self, pid: u8, data: u16) {
        let crc = calc_usb_crc5(data);
        let packet = [SYNC, pid, data as u8, (crc << 3) | (data >> 8) as u8];
        self.tx_packet(&packet);
    }

    #[link_section = ".data"]
    #[allow(unused)]
    fn tx_token(&mut self, pid: u8, addr: u8, ep_num: u8) {
        let data = ((ep_num as u16 & 0xf) << 7) | (addr as u16 & 0x7f);
        self.tx_with_crc5(pid, data);
    }

    /// Transmit a data packet.
    ///
    /// We calcuate CRC on the fly.
    #[link_section = ".data"]
    #[allow(unused)]
    fn tx_data(&mut self, pid: u8, data: &[u8]) {
        self.setup_tx(data.len() + 4);
        self.tx.write(SYNC as u32);
        self.tx.write(pid as u32);
        let mut i = 0;
        let mut crc = 0xffff;
        while i < data.len() {
            let data = data[i];
            if self.tx.write(data as u32) {
                crc = update_crc_16(crc, data);
                i += 1;
            } else {
                break;
            }
        }
        unsafe {
            pio_sm_start(&self.tx_sm);
        }
        while i < data.len() {
            let data = data[i];
            if self.tx.write(data as u32) {
                crc = update_crc_16(crc, data);
                i += 1;
            }
        }
        crc ^= 0xffff;
        while !self.tx.write(crc as u8 as u32) {}
        while !self.tx.write((crc >> 8) as u32) {}
        self.finish_tx();
    }

    #[link_section = ".data"]
    fn finish_tx(&mut self) {
        while !self.tx.write(0xff) {}
        while self.tx_sm.instruction_address() != TX_IDLE_ADDRESS {}
    }

    /// Transmit a packet.
    ///
    /// The packet must include SYNC (0x80) and any CRC.
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
        self.finish_tx();
    }

    #[link_section = ".data"]
    fn bus_reset(&mut self, reset: bool) {
        unsafe {
            pio_sm_stop(&self.tx_sm);
        }
        if reset {
            self.tx_sm.exec_instruction(Instruction {
                operands: pio::InstructionOperands::SET {
                    destination: pio::SetDestination::PINS,
                    data: 0,
                },
                delay: 0,
                side_set: None,
            });
        }
        self.tx_sm.exec_instruction(Instruction {
            operands: pio::InstructionOperands::SET {
                destination: pio::SetDestination::PINDIRS,
                data: if reset { 3 } else { 0 },
            },
            delay: 0,
            side_set: None,
        });
    }

    #[link_section = ".data"]
    fn prepare_rx(&mut self) {
        unsafe {
            pio_write_instr(
                &self.pio, 9, 0x25a0, // wait 1 pin 0 [5]
            );
            pio_sm_exec(&self.rx_sm, 0x0009); // jmp start
            pio_sm_exec(&self.rx_sm, 0xa0eb); // mov osr, !null
            pio_sm_start(&self.rx_sm);
        }
    }

    /// Set input enable on data pins.
    ///
    /// Potentially useful for Erratum E9 mitigation.
    #[link_section = ".data"]
    #[allow(unused)]
    fn enable_inputs(&mut self, en: bool) {
        if !en {
            unsafe {
                pio_sm_exec(&self.tx_sm, 0xe080); // set pindirs, 0
            }
        }
        self.dp.set_input_enable(en);
        self.dm.set_input_enable(en);
    }
}

#[link_section = ".data"]
pub fn do_pio_experiment(pins: Pins, pio: PIO0, mut timer: Timer<impl TimerDevice>) {
    let _led = pins.gpio29.into_push_pull_output_in_state(PinState::Low);
    let _usb_host_5v_power = pins.gpio11.into_push_pull_output_in_state(PinState::High);
    let usb_host_data_plus = pins.gpio1;
    let usb_host_data_minus = pins.gpio2;
    let mut resets = unsafe { Peripherals::steal().RESETS };
    let mut usb_pio = UsbPio::new(pio, usb_host_data_plus, usb_host_data_minus, &mut resets);

    timer.delay_ms(12);
    for _ in 0..1 {
        //usb_pio.enable_inputs(false);
        timer.delay_ms(1);
        usb_pio.bus_reset(true);
        timer.delay_ms(3);
        usb_pio.bus_reset(false);
        timer.delay_us(1000);
        //usb_pio.enable_inputs(true);
    }
    usb_pio.tx_with_crc5(PID_SOF, 1);
    usb_pio.tx_token(PID_SETUP, 0, 0);
    let setup = [0x80, 0x06, 0x00, 0x01, 0x00, 0x00, 0x12, 0x00];
    usb_pio.tx_data(PID_DATA0, &setup);
    usb_pio.prepare_rx();

    // So apparently the memclr running in xip is a long enough delay,
    // but we do this anyway.
    timer.delay_us(2);
    let mut a = [0u32; 16];
    let mut i = 0;
    while i < a.len() {
        if let Some(d) = usb_pio.rx.read() {
            a[i] = d >> 24;
            i += 1;
        } else {
            break;
        }
    }
    for d in &a[..i] {
        console!("data: {d:x}");
    }
    console!("end of data, interrupts = {:x}", usb_pio.pio.get_irq_raw());
    console!("tx_sm instr = {}", usb_pio.tx_sm.instruction_address());
    console!("rx_sm instr = {}", usb_pio.rx_sm.instruction_address());
    unsafe {
        pio_sm_stop(&usb_pio.rx_sm);
    }
    usb_pio.pio.clear_irq(1 << usb_pio.eop_irq);
}
