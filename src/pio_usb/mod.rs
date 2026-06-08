mod crc;
mod wire;

use core::{cell::UnsafeCell, mem::MaybeUninit};

use cortex_m::peripheral::NVIC;
use embedded_hal::digital::StatefulOutputPin;
use hal::{
    gpio::{
        bank0::{Gpio1, Gpio2, Gpio21},
        FunctionNull, FunctionSio, Pin, PinState, Pins, PullDown, SioOutput, ValidFunction,
    },
    pac::{interrupt, Peripherals, PIO0, RESETS},
    pio::{Buffers, PIOExt, Rx, StateMachine, Stopped, Tx, ValidStateMachine, SM0, SM1},
    timer::{Alarm, CopyableTimer0},
    Timer,
};
use pio::Instruction;
use rp235x_hal::{self as hal};

use crate::pio_usb::{
    crc::{calc_usb_crc5, update_crc_16},
    wire::{DEVICE_TO_HOST, GET_DESCRIPTOR, HOST_TO_DEVICE, SET_ADDRESS, SET_CONFIGURATION},
};

struct UsbPioWrapper<P: PIOExt>(UnsafeCell<MaybeUninit<UsbPio<P>>>);

unsafe impl<P: PIOExt> Sync for UsbPioWrapper<P> {}

static USB_PIO: UsbPioWrapper<PIO0> = UsbPioWrapper(UnsafeCell::new(MaybeUninit::uninit()));

const SYNC: u8 = 0x80;
const PID_ACK: u8 = 0xd2;
const PID_DATA0: u8 = 0xc3;
const PID_DATA1: u8 = 0x4b;
const PID_IN: u8 = 0x69;
const PID_SOF: u8 = 0xa5;
const PID_SETUP: u8 = 0x2d;

// Very low level PIO functions

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

#[inline]
#[link_section = ".data"]
fn fast_get_timer() -> u32 {
    let timer_addr = 0x400b0028;
    unsafe { core::ptr::read_volatile(timer_addr as *const u32) }
}

#[inline]
#[link_section = ".data"]
/// Schedules the alarm at timestamp.
///
/// Doesn't try to do anything fancy about detecting timestamps in the past.
unsafe fn fast_alarm_schedule(timestamp: u32) {
    let timer_addr = 0x400b0010;
    unsafe {
        core::ptr::write_volatile(timer_addr as *mut u32, timestamp);
    }
}

#[inline]
#[link_section = ".data"]
/// Schedules the alarm at timestamp.
///
/// Doesn't try to do anything fancy about detecting timestamps in the past.
unsafe fn fast_alarm_disarm() {
    let timer_addr = 0x400b0020;
    unsafe {
        core::ptr::write_volatile(timer_addr as *mut u32, 1);
    }
}

const TX_IDLE_ADDRESS: u32 = 5;

const MAX_BUF: usize = 1028;
const BUF_DURATION_US: u32 = 2;

/// First line of dispatch for interrupts
#[derive(Clone, Copy, PartialEq)]
enum StateMinor {
    TimerWait,
    Tx,
    /// Transmitting but immediately switch to Rx when packet is sent.
    TxListen,
    Rx,
    Nop,
}

struct UsbPio<PIO: PIOExt> {
    tx_sm: StateMachine<(PIO, SM0), Stopped>,
    tx: Tx<(PIO, SM0)>,
    rx_sm: StateMachine<(PIO, SM1), Stopped>,
    rx: Rx<(PIO, SM1)>,
    pio: rp235x_hal::pio::PIO<PIO>,
    #[allow(unused)]
    dp: Pin<Gpio1, PIO::PinFunction, PullDown>,
    #[allow(unused)]
    dm: Pin<Gpio2, PIO::PinFunction, PullDown>,
    debug: Pin<Gpio21, FunctionSio<SioOutput>, PullDown>,
    alarm: hal::timer::Alarm0<CopyableTimer0>,
    state: u32,
    state_minor: StateMinor,
    buf: [u8; MAX_BUF],
    buf_ix: usize,
    crc: u16,
    frame_number: u32,
    /// Timestamp (in us) of last SOF packet
    sof_timestamp: u32,
}

impl<PIO: PIOExt> UsbPio<PIO> {
    // TODO: make more pin-agile
    fn new(
        pio: PIO,
        dp: Pin<Gpio1, FunctionNull, PullDown>,
        dm: Pin<Gpio2, FunctionNull, PullDown>,
        debug: Pin<Gpio21, FunctionSio<SioOutput>, PullDown>,
        alarm: hal::timer::Alarm0<CopyableTimer0>,
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
        let usb_tx_program = pio::pio_file!("src/pio_usb/usb_tx.pio");
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
        let rx_program = pio::pio_file!("src/pio_usb/usb_rx.pio");
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
            dp,
            dm,
            debug,
            alarm,
            state: 0,
            state_minor: StateMinor::TimerWait,
            buf: [0u8; MAX_BUF],
            buf_ix: 0,
            crc: 0,
            frame_number: 0,
            sof_timestamp: 0,
        }
    }

    #[link_section = ".data"]
    fn setup_tx(&mut self, n_bytes: usize) {
        // Prime the state machine for transmit
        unsafe {
            pio_sm_stop(&self.tx_sm);
            pio_write_instr(
                &self.pio, 9, 0x0188, // jmp y-- nostuff [1]
            );
            pio_write_instr(
                &self.pio, 10, 0x0106, // jmp bit_zero [1]
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
        self.state_minor = StateMinor::Tx;
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
        self.state_minor = StateMinor::Tx;
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
            pio_write_instr(
                &self.pio, 10, 0x4061, // in null, 1
            );
            pio_sm_exec(&self.rx_sm, 0x0009); // jmp start
            pio_sm_exec(&self.rx_sm, 0xa0eb); // mov osr, !null
            pio_sm_start(&self.rx_sm);
        }
        self.buf_ix = 0;
        self.crc = 0xffff;
    }

    #[link_section = ".data"]
    fn crc_ok(&self) -> bool {
        let crc = !self.crc;
        let ix = self.buf_ix;
        ix >= 4 && self.buf[ix - 2] == crc as u8 && self.buf[ix - 1] == (crc >> 8) as u8
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

    #[link_section = ".data"]
    fn wait_next_sof(&mut self) {
        self.sof_timestamp = self.sof_timestamp.wrapping_add(1000);
        self.frame_number = self.frame_number.wrapping_add(1);
        unsafe {
            fast_alarm_schedule(self.sof_timestamp);
        }
        self.state = 12;
        self.state_minor = StateMinor::TimerWait;
    }

    #[link_section = ".data"]
    fn run_major(&mut self) {
        match self.state {
            0 => {
                self.bus_reset(true);
                console!("start bus reset {}", fast_get_timer());
                _ = self.alarm.schedule(fugit::MicrosDurationU32::millis(12));
                self.state_minor = StateMinor::TimerWait;
            }
            1 => {
                self.bus_reset(false);
                console!("stop bus reset {}", fast_get_timer());
                _ = self.alarm.schedule(fugit::MicrosDurationU32::millis(1));
                self.state_minor = StateMinor::TimerWait;
            }
            2 => {
                self.debug.toggle();
                self.tx_with_crc5(PID_SOF, 0);
                self.sof_timestamp = fast_get_timer();
                self.frame_number = 1;
            }
            3 => {
                self.debug.toggle();
                self.tx_token(PID_SETUP, 0, 0);
            }
            4 => {
                let setup = [
                    DEVICE_TO_HOST,
                    GET_DESCRIPTOR,
                    0x00,
                    0x01,
                    0x00,
                    0x00,
                    0x12,
                    0x00,
                ];
                self.tx_data(PID_DATA0, &setup);
                self.state_minor = StateMinor::TxListen;
            }
            5 => {
                // result of handshake is in buf (should be 80 d2)
                self.debug.toggle();
                self.tx_token(PID_IN, 0, 0);
                self.state_minor = StateMinor::TxListen;
            }
            6 => {
                let ok = self.crc_ok();
                if ok {
                    self.tx_handshake(PID_ACK);
                } else {
                    console!("crc fail");
                }
            }
            7 => {
                self.debug.toggle();
                self.tx_token(PID_SETUP, 0, 0);
            }
            8 => {
                let set_addr = [HOST_TO_DEVICE, SET_ADDRESS, 1, 0, 0, 0, 0, 0];
                self.tx_data(PID_DATA0, &set_addr);
                self.state_minor = StateMinor::TxListen;
            }
            9 => {
                // got ack from set_address
                self.tx_token(PID_IN, 0, 0);
                self.state_minor = StateMinor::TxListen;
            }
            10 => {
                self.tx_handshake(PID_ACK);
            }
            11 => {
                self.wait_next_sof();
                return;
            }
            12 => {
                // SOF handler
                self.tx_with_crc5(PID_SOF, self.frame_number as u16 & 0x7ff);
                if self.frame_number != 3 {
                    self.wait_next_sof();
                    return;
                }
            }
            13 => {
                self.tx_token(PID_SETUP, 1, 0);
            }
            14 => {
                let set_config = [HOST_TO_DEVICE, SET_CONFIGURATION, 1, 0, 0, 0, 0, 0];
                self.tx_data(PID_DATA0, &set_config);
                self.state_minor = StateMinor::TxListen;
            }
            15 => {
                console!("buf ix = {}, crc={:4x}", self.buf_ix, !self.crc);
                for i in 0..self.buf_ix {
                    console!("data: {:x}", self.buf[i]);
                }
            }
            _ => (),
        }
        self.state += 1;
    }
}

#[link_section = ".data"]
pub fn do_pio_experiment(pins: Pins, pio: PIO0, mut timer: Timer<CopyableTimer0>) {
    let _led = pins.gpio29.into_push_pull_output_in_state(PinState::Low);
    let _usb_host_5v_power = pins.gpio11.into_push_pull_output_in_state(PinState::High);
    let usb_host_data_plus = pins.gpio1;
    let usb_host_data_minus = pins.gpio2;
    let usb_debug = pins.gpio21.into_push_pull_output_in_state(PinState::Low);
    let mut alarm = timer.alarm_0().unwrap();
    _ = alarm.schedule(fugit::MicrosDurationU32::millis(12));
    alarm.enable_interrupt();
    let mut resets = unsafe { Peripherals::steal().RESETS };
    let usb_pio = UsbPio::new(
        pio,
        usb_host_data_plus,
        usb_host_data_minus,
        usb_debug,
        alarm,
        &mut resets,
    );

    unsafe {
        usb_pio.pio.irq0().enable_sm_interrupt(0);
        usb_pio.pio.irq0().enable_sm_interrupt(1);
        (*USB_PIO.0.get()).write(usb_pio);
        NVIC::unmask(crate::hal::pac::Interrupt::TIMER0_IRQ_0);
        NVIC::unmask(crate::hal::pac::Interrupt::PIO0_IRQ_0);
    }
}

#[link_section = ".data"]
#[interrupt]
fn TIMER0_IRQ_0() {
    let usb_pio = unsafe { (*USB_PIO.0.get()).assume_init_mut() };
    usb_pio.alarm.clear_interrupt();
    match usb_pio.state_minor {
        StateMinor::Rx => {
            // receiving a packet
            let mut ix = usb_pio.buf_ix;
            while let Some(data) = usb_pio.rx.read() {
                let byte = (data >> 24) as u8;
                usb_pio.buf[ix] = byte;
                if ix >= 4 {
                    usb_pio.crc = update_crc_16(usb_pio.crc, usb_pio.buf[ix - 2]);
                }
                ix += 1;
            }
            // timeout logic; if ix = 0, we didn't get a packet
            usb_pio.buf_ix = ix;
            usb_pio.debug.toggle();
            unsafe {
                fast_alarm_schedule(fast_get_timer().wrapping_add(BUF_DURATION_US));
            }
        }
        StateMinor::TimerWait => {
            usb_pio.run_major();
        }
        // TODO: stream tx
        _ => (),
    }
}

#[link_section = ".data"]
#[interrupt]
fn PIO0_IRQ_0() {
    let usb_pio = unsafe { (*USB_PIO.0.get()).assume_init_mut() };
    match usb_pio.state_minor {
        StateMinor::Tx => {
            usb_pio.debug.toggle();
            while usb_pio.tx_sm.instruction_address() != TX_IDLE_ADDRESS {}
            usb_pio.pio.clear_irq(1);
            usb_pio.state_minor = StateMinor::Nop;
            usb_pio.run_major();
        }
        StateMinor::TxListen => {
            // sent packet, waiting for rx
            while usb_pio.tx_sm.instruction_address() != TX_IDLE_ADDRESS {}
            usb_pio.prepare_rx();
            usb_pio.pio.clear_irq(1);
            usb_pio.debug.toggle();
            unsafe {
                fast_alarm_schedule(fast_get_timer().wrapping_add(BUF_DURATION_US));
            }
            usb_pio.state_minor = StateMinor::Rx;
        }
        StateMinor::Rx => {
            // got packet
            usb_pio.debug.toggle();
            unsafe {
                pio_sm_stop(&usb_pio.rx_sm);
                pio_sm_restart(&usb_pio.rx_sm);
                fast_alarm_disarm();
            }
            usb_pio.pio.clear_irq(2);
            let mut ix = usb_pio.buf_ix;
            while let Some(data) = usb_pio.rx.read() {
                let byte = (data >> 24) as u8;
                usb_pio.buf[ix] = byte;
                if ix >= 4 {
                    usb_pio.crc = update_crc_16(usb_pio.crc, usb_pio.buf[ix - 2]);
                }
                ix += 1;
            }
            usb_pio.buf_ix = ix;
            usb_pio.state_minor = StateMinor::Nop;
            usb_pio.run_major();
        }
        _ => (),
    }
}
