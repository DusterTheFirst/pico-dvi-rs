mod crc;
mod host_task;
mod iface;
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
use rp235x_hal as hal;

use crate::pio_usb::wire::{pid_data_toggle, PID_PRE};

use self::{
    crc::{calc_usb_crc5, update_crc_16},
    iface::{IntIface, Request, Status},
    wire::{pid_data, PID_ACK, PID_DATA0, PID_IN, PID_NAK, PID_OUT, PID_SETUP, PID_SOF, SYNC},
};

pub use self::host_task::UsbTask;

struct UsbPioWrapper<P: PIOExt>(UnsafeCell<MaybeUninit<UsbPio<P>>>);

unsafe impl<P: PIOExt> Sync for UsbPioWrapper<P> {}

static USB_PIO: UsbPioWrapper<PIO0> = UsbPioWrapper(UnsafeCell::new(MaybeUninit::uninit()));

const TIMEOUT_DELAY_US: u32 = 3;
const TIMEOUT_DELAY_US_LOW_SPEED: u32 = 16;

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

const TX_IDLE_ADDRESS: u32 = 4;

const MAX_BUF: usize = 1028;

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
    #[allow(unused)]
    debug: Pin<Gpio21, FunctionSio<SioOutput>, PullDown>,
    alarm: hal::timer::Alarm0<CopyableTimer0>,
    state: u32,
    buf: [u8; MAX_BUF],
    buf_ix: usize,
    frame_number: u32,
    /// Timestamp (in us) of last SOF packet
    sof_timestamp: u32,
    int_iface: IntIface,
    held_pipes: u32,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum RxResult {
    Ok,
    CrcMismatch,
    Timeout,
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
            buf: [0u8; MAX_BUF],
            buf_ix: 0,
            frame_number: 0,
            sof_timestamp: 0,
            int_iface: IntIface::default(),
            held_pipes: 0,
        }
    }

    #[link_section = ".data"]
    fn setup_tx_raw(&mut self, n_bytes: usize) {
        // Prime the state machine for transmit
        unsafe {
            pio_sm_stop(&self.tx_sm);
            pio_write_instr(&self.pio, 9, 0x0105); // jmp bit_zero [1]
            pio_sm_exec(&self.tx_sm, 0xe001); // set pins, LINE_STATE_J
            pio_sm_exec(&self.tx_sm, 0xe083); // set pindirs, 3
            pio_sm_restart(&self.tx_sm);
            pio_sm_exec(&self.tx_sm, 0x6020); // out x, 32
            self.tx.write(n_bytes as u32 * 8);
            pio_sm_exec(&self.tx_sm, 0x60a1); // out pc, 1
        }
    }

    #[link_section = ".data"]
    fn setup_tx(&mut self, n_bytes: usize, low_speed: bool) {
        if low_speed {
            unsafe {
                pio_write_instr(&self.pio, 2, 0xe701); // set pins, J [7]
                pio_write_instr(&self.pio, 4, 0xe083); // set pindirs, 3
            }
            self.tx_handshake(PID_PRE, false);
            unsafe {
                pio_write_instr(&self.pio, 2, 0xe700); // set pins, SE0 [7]
                pio_write_instr(&self.pio, 4, 0xe080); // set pindirs, 0
            }
            self.tx_sm.clock_divisor_fixed_point(22, 0);
            self.rx_sm.clock_divisor_fixed_point(8, 205);
        } else {
            self.tx_sm.clock_divisor_fixed_point(2, 192);
            self.rx_sm.clock_divisor_fixed_point(1, 26);
        }
        self.setup_tx_raw(n_bytes);
    }

    /// Transmit a 2-byte handshake packet.
    ///
    /// This method might go away, subsumed by `tx_packet`.
    #[link_section = ".data"]
    fn tx_handshake(&mut self, pid: u8, low_speed: bool) {
        self.setup_tx(2, low_speed);
        self.tx.write(SYNC as u32);
        self.tx.write(pid as u32);
        self.tx.write(0xff);
        unsafe {
            pio_sm_start(&self.tx_sm);
        }
        while self.tx_sm.instruction_address() != TX_IDLE_ADDRESS {}
    }

    #[link_section = ".data"]
    fn tx_with_crc5(&mut self, pid: u8, data: u16, low_speed: bool) {
        let crc = calc_usb_crc5(data);
        let packet = [SYNC, pid, data as u8, (crc << 3) | (data >> 8) as u8];
        self.tx_packet(&packet, low_speed);
    }

    #[link_section = ".data"]
    fn tx_token(&mut self, pid: u8, addr: u8, ep_num: u8, low_speed: bool) {
        let data = ((ep_num as u16 & 0xf) << 7) | (addr as u16 & 0x7f);
        self.tx_with_crc5(pid, data, low_speed);
    }

    #[link_section = ".data"]
    fn tx_pre_token(&mut self) {
        unsafe {
            pio_write_instr(&self.pio, 2, 0xe701); // set pins, J [7]
            pio_write_instr(&self.pio, 4, 0xe083); // set pindirs, 3
        }
        self.tx_handshake(PID_PRE, false);
        unsafe {
            pio_write_instr(&self.pio, 2, 0xe700); // set pins, SE0 [7]
            pio_write_instr(&self.pio, 4, 0xe080); // set pindirs, 0
        }
        self.debug.toggle();
        self.tx_sm.clock_divisor_fixed_point(22, 0);
    }

    /// Transmit a data packet.
    ///
    /// We calcuate CRC on the fly.
    #[link_section = ".data"]
    fn tx_data(&mut self, pid: u8, data: &[u8], low_speed: bool) {
        self.setup_tx(data.len() + 4, low_speed);
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
    fn tx_packet(&mut self, packet: &[u8], low_speed: bool) {
        self.setup_tx(packet.len(), low_speed);
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

    /// Receive a packet.
    ///
    /// On `Ok` result, the received packet is in the buffer, with `buf_ix` equal to the length.
    #[link_section = ".data"]
    fn rx_packet(&mut self, low_speed: bool) -> RxResult {
        self.prepare_rx();
        let timeout = if low_speed {
            TIMEOUT_DELAY_US_LOW_SPEED
        } else {
            TIMEOUT_DELAY_US
        };
        let mut last_timestamp = fast_get_timer();
        let mut ix = 0;
        let mut result = RxResult::Ok;
        let mut crc = 0xffff;
        loop {
            let irq = self.pio.get_irq_raw();
            let ix_start = ix;
            while let Some(data) = self.rx.read() {
                let byte = (data >> 24) as u8;
                self.buf[ix] = byte;
                if ix >= 4 {
                    crc = update_crc_16(crc, self.buf[ix - 2]);
                }
                ix += 1;
            }
            if irq != 0 {
                break;
            }
            let timestamp = fast_get_timer();
            if ix > ix_start {
                last_timestamp = timestamp;
            } else if timestamp.wrapping_sub(last_timestamp) >= timeout {
                result = RxResult::Timeout;
                break;
            }
        }
        if ix >= 4 {
            crc = !crc;
            if self.buf[ix - 2] != crc as u8 || self.buf[ix - 1] != (crc >> 8) as u8 {
                result = RxResult::CrcMismatch;
            }
            //console!("crc fail");
        }
        unsafe {
            pio_sm_stop(&self.rx_sm);
            pio_sm_restart(&self.rx_sm);
        }
        self.pio.clear_irq(2);
        self.buf_ix = ix;
        result
    }

    /// Expect a single byte handshake.
    ///
    /// Returns `None` on timeout, malformed packed, or wrong packet size.
    #[link_section = ".data"]
    fn rx_handshake(&mut self, low_speed: bool) -> Option<u8> {
        self.prepare_rx();
        let last_timestamp = fast_get_timer();
        let mut ix = 0;
        let mut ok = true;
        let mut pid = 0;
        loop {
            let irq = self.pio.get_irq_raw();
            if let Some(data) = self.rx.read() {
                let byte = (data >> 24) as u8;
                match ix {
                    0 => ok = byte == SYNC,
                    1 => pid = byte,
                    _ => ok = false,
                }
                ix += 1;
            }
            let timestamp = fast_get_timer();
            let timeout = if low_speed {
                TIMEOUT_DELAY_US_LOW_SPEED
            } else {
                TIMEOUT_DELAY_US
            };
            if timestamp.wrapping_sub(last_timestamp) >= timeout {
                ok = false;
                break;
            }
            if irq != 0 {
                break;
            }
        }
        unsafe {
            pio_sm_stop(&self.rx_sm);
            pio_sm_restart(&self.rx_sm);
        }
        self.pio.clear_irq(2);
        if ok {
            Some(pid)
        } else {
            None
        }
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
            pio_write_instr(&self.pio, 9, 0x25a0); // wait 1 pin 0 [5]
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

    #[link_section = ".data"]
    fn wait_next_sof(&mut self) {
        self.sof_timestamp = self.sof_timestamp.wrapping_add(1000);
        self.frame_number = self.frame_number.wrapping_add(1);
        unsafe {
            fast_alarm_schedule(self.sof_timestamp);
        }
        self.state = 2;
    }

    #[link_section = ".data"]
    fn handle_requests(&mut self) {
        let mut reqs = self.int_iface.poll() | self.held_pipes;
        self.held_pipes = 0;
        while reqs != 0 {
            let pipe_ix = reqs.trailing_zeros() as usize;
            reqs &= reqs - 1;
            unsafe {
                let mut pipe = self.int_iface.pipe(pipe_ix);
                match pipe.req {
                    Request::Setup => {
                        self.tx_token(PID_SETUP, pipe.addr, pipe.ep, pipe.low_speed);
                        self.tx_data(PID_DATA0, &pipe.buf[0..8], pipe.low_speed);
                        let handshake = self.rx_handshake(pipe.low_speed);
                        if handshake == Some(PID_ACK) {
                            pipe.toggle = 1;
                            pipe.status = Status::Success;
                        } else {
                            // TODO: this always retries, we should have a retry count.
                            //console!("timeout");
                            //self.debug.toggle();
                            self.held_pipes |= 1 << pipe_ix;
                            continue;
                        }
                    }
                    Request::In => {
                        self.tx_token(PID_IN, pipe.addr, pipe.ep, pipe.low_speed);
                        match self.rx_packet(pipe.low_speed) {
                            RxResult::Ok => {
                                let expected_pid = pid_data(pipe.toggle);
                                if self.buf[1] == expected_pid && self.buf_ix >= 4 {
                                    self.tx_handshake(PID_ACK, pipe.low_speed);
                                    let len = self.buf_ix - 4;
                                    // This copy could be avoided by having rx_packet decode into the
                                    // provided buf. But that's a bit tricky.
                                    pipe.buf[0..len].copy_from_slice(&self.buf[2..][..len]);
                                    pipe.len = len as u16;
                                    pipe.toggle ^= 1;
                                    pipe.status = Status::Success;
                                } else if self.buf[1] == PID_NAK {
                                    // This retries indefinitely, which is reasonable for interrupt
                                    // endpoints, but other requests should eventually fail.
                                    //
                                    // A bunch of other refinements make sense:
                                    //   * retry interval other than 1ms
                                    //   * a cancel mechanism
                                    self.held_pipes |= 1 << pipe_ix;
                                    continue;
                                } else if self.buf[1] == pid_data_toggle(expected_pid) {
                                    // if wrong data pid, then our last ack got lost, send ack
                                    // again but retry transaction.
                                    self.tx_handshake(PID_ACK, pipe.low_speed);
                                    self.held_pipes |= 1 << pipe_ix;
                                    continue;
                                } else {
                                    //console!("pid = {:02x}", self.buf[1]);
                                    pipe.status = Status::Error;
                                }
                            }
                            _ => {
                                pipe.status = Status::Error;
                            }
                        }
                    }
                    Request::Out => {
                        self.tx_token(PID_OUT, pipe.addr, pipe.ep, pipe.low_speed);
                        let pid = pid_data(pipe.toggle);
                        self.tx_data(pid, &pipe.buf[..pipe.len as usize], pipe.low_speed);
                        let handshake = self.rx_handshake(pipe.low_speed);
                        if handshake == Some(PID_ACK) {
                            pipe.toggle = 1;
                            pipe.status = Status::Success;
                        } else {
                            // TODO: retry, handle other errors
                            pipe.status = Status::Error;
                        }
                    }
                    Request::Delay => {
                        pipe.timer -= 1;
                        if pipe.timer == 0 {
                            pipe.status = Status::Success;
                        } else {
                            self.held_pipes |= 1 << pipe_ix;
                            continue;
                        }
                    }
                    _ => (),
                }
                self.int_iface.send_response(pipe);
            }
        }
    }
}

#[allow(unused)]
fn blocking_delay_us(delay: u32) {
    let start = fast_get_timer();
    while fast_get_timer().wrapping_sub(start) <= delay {}
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
        (*USB_PIO.0.get()).write(usb_pio);
        NVIC::unmask(crate::hal::pac::Interrupt::TIMER0_IRQ_0);
    }
}

#[link_section = ".data"]
#[interrupt]
fn TIMER0_IRQ_0() {
    let usb_pio = unsafe { (*USB_PIO.0.get()).assume_init_mut() };
    usb_pio.alarm.clear_interrupt();
    match usb_pio.state {
        0 => {
            usb_pio.bus_reset(true);
            console!("start bus reset {}", fast_get_timer());
            _ = usb_pio.alarm.schedule(fugit::MicrosDurationU32::millis(12));
            usb_pio.state = 1;
        }
        1 => {
            usb_pio.bus_reset(false);
            console!("stop bus reset {}", fast_get_timer());
            _ = usb_pio.alarm.schedule(fugit::MicrosDurationU32::millis(1));
            usb_pio.state = 2;
        }
        2 => {
            if usb_pio.frame_number == 0 {
                usb_pio.sof_timestamp = fast_get_timer();
            }
            usb_pio.tx_with_crc5(PID_SOF, usb_pio.frame_number as u16 & 0x7ff, false);
            usb_pio.handle_requests();
            usb_pio.wait_next_sof();
        }
        /*
        2 => {
            if usb_pio.frame_number == 0 {
                usb_pio.sof_timestamp = fast_get_timer();
            }
            usb_pio.tx_with_crc5(PID_SOF, usb_pio.frame_number as u16 & 0x7ff);
            match usb_pio.frame_number {
                0 => {
                    let setup = SetupPacket::new(
                        DEVICE_TO_HOST,
                        GET_DESCRIPTOR,
                        (DEVICE_DESCRIPTOR as u16) << 8,
                        0,
                        0x12,
                    );
                    let rx_result = usb_pio.control_transfer_in(setup, 0);
                    if rx_result != Some(PID_ACK) {
                        console!("rx get desc {rx_result:x?}");
                    }
                    let setup = SetupPacket::new(HOST_TO_DEVICE, SET_ADDRESS, 1, 0, 0);
                    let rx_result = usb_pio.control_transfer_none(setup, 0);
                    if rx_result != Some(PID_ACK) {
                        console!("rx set addr {rx_result:x?}");
                    }
                }
                3 => {
                    let setup = SetupPacket::new(HOST_TO_DEVICE, SET_CONFIGURATION, 1, 0, 0);
                    let rx_result = usb_pio.control_transfer_none(setup, 1);
                    if rx_result != Some(PID_ACK) {
                        console!("rx set conf {rx_result:x?}");
                    }

                    for port in 1..=4 {
                        let setup = SetupPacket::new(
                            HOST_TO_DEVICE | CLASS_REQUEST | RECIPIENT_OTHER,
                            SET_FEATURE,
                            PORT_POWER,
                            port,
                            0,
                        );
                        let rx_result = usb_pio.control_transfer_none(setup, 1);
                        if rx_result != Some(PID_ACK) {
                            console!("rx set port power {rx_result:x?}");
                        }
                    }

                    let setup = SetupPacket::new(
                        DEVICE_TO_HOST | CLASS_REQUEST,
                        GET_DESCRIPTOR,
                        (HUB_DESCRIPTOR as u16) << 8,
                        0,
                        9,
                    );
                    let rx_result = usb_pio.control_transfer_none(setup, 1);
                    if rx_result == Some(PID_ACK) {
                        console!("rx_result = {rx_result:x?} buf_ix = {}", usb_pio.buf_ix);
                        for i in 0..usb_pio.buf_ix {
                            console!("data: {:02x}", usb_pio.buf[i]);
                        }
                    } else {
                        console!("rx fail {rx_result:?}");
                    }
                }
                _ => {
                    if usb_pio.frame_number >= 100 {
                        usb_pio.tx_token(PID_IN, 1, 1);
                        let rx_result = usb_pio.rx_packet();
                        if rx_result == RxResult::Ok && usb_pio.buf[1] != PID_NAK {
                            usb_pio.tx_handshake(PID_ACK);
                            let mut packet = usb_pio.buf[2] as u16;
                            if usb_pio.buf_ix >= 6 {
                                packet |= (usb_pio.buf[3] as u16) << 8;
                            }
                            usb_pio.handle_hub_packet(packet);
                        }
                        usb_pio.tick_bus();
                    }
                }
            }
            usb_pio.wait_next_sof();
        }
        */
        _ => (),
    }
}
