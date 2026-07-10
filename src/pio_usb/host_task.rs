//! A task that is continuously polled to run the USB host role.

use crate::pio_usb::{
    iface::{PipeGuard, Request, Status, UserIface},
    wire::{
        CLASS_REQUEST, CLEAR_FEATURE, DEVICE_DESCRIPTOR, DEVICE_TO_HOST, GET_DESCRIPTOR,
        GET_STATUS, HOST_TO_DEVICE, HUB_DESCRIPTOR, PORT_POWER, PORT_RESET, RECIPIENT_OTHER,
        SET_ADDRESS, SET_CONFIGURATION, SET_FEATURE,
    },
};

/// Maximum number of pipes that will be allocated
const N_PIPES: usize = 16;

/// A "task" encapsulating the host side of a USB bus.
///
/// It's conceptually similar to an async task, but there is no async
/// involved here. Rather, tasks are implemented as explicit state
/// machines.
pub struct UsbTask {
    iface: UserIface,
    transfers: [Transfer; N_PIPES],
    transfers_free: u32,
    hub: HubTask,
    pipe_tasks: [PipeTask; N_PIPES],
}

#[derive(Copy, Clone)]
pub struct SetupPacket {
    pub raw: [u8; 8],
}

struct Transfer {
    ty: TransferType,
    phase: TransferPhase,
    buf: [u8; 64],
    packet_size: u16,
    target_len: u16,
    actual_len: u16,
    pipe: Option<PipeGuard>,
}

enum TransferType {
    ControlNone,
    ControlIn,
    #[expect(unused)]
    ControlOut,
    PollInterrupt,
}

enum TransferPhase {
    Idle,
    Setup,
    Data,
    Status,
    Delay,
}

#[derive(Clone, Copy, PartialEq)]
enum TickResult {
    Send,
    Done,
    Error,
}

#[derive(Default)]
struct HubTask {
    state: DeviceState,
    hub_state: HubState,
    port: u16,
    changes: u16,
    pending_resets: u16,
    resetting: u16,
    ignore_hub_events: bool,
}

// states from Figure 9-1 of USB 2.0 spec
#[derive(Clone, Copy, Default)]
enum DeviceState {
    #[default]
    Default,
    Address,
    Configured,
}

#[derive(Clone, Copy, Default)]
enum HubState {
    #[default]
    Configuring,
    PowerPort(u8),
    Polling,
    GotPoll,
}

struct PipeTask {
    state: PipeState,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum PipeState {
    Idle,
    GotPortStatus,
    DidClearFeature,
    DidReset,
    GotReset,
    AfterResetDelay,
    GotDesc,
    Address,
    AfterAddressDelay,
    Configured,
    GotPoll,
}

impl SetupPacket {
    #[link_section = ".data"]
    pub fn new(request_type: u8, request: u8, value: u16, index: u16, length: u16) -> Self {
        let mut raw = [0; 8];
        raw[0] = request_type;
        raw[1] = request;
        raw[2..4].copy_from_slice(&u16::to_le_bytes(value));
        raw[4..6].copy_from_slice(&u16::to_le_bytes(index));
        raw[6..8].copy_from_slice(&u16::to_le_bytes(length));
        SetupPacket { raw }
    }

    pub fn length(&self) -> u16 {
        u16::from_le_bytes(self.raw[6..8].try_into().unwrap())
    }
}

impl Transfer {
    const fn new() -> Self {
        Self {
            ty: TransferType::ControlNone,
            phase: TransferPhase::Idle,
            buf: [0; 64],
            packet_size: 0,
            target_len: 0,
            actual_len: 0,
            pipe: None,
        }
    }

    fn tick(&mut self) -> TickResult {
        let pipe = self.pipe.as_mut().unwrap();
        if pipe.status == Status::Success {
            match self.phase {
                TransferPhase::Setup => match self.ty {
                    TransferType::ControlIn => {
                        self.phase = TransferPhase::Data;
                        pipe.req = Request::In;
                        TickResult::Send
                    }
                    TransferType::ControlNone => {
                        self.phase = TransferPhase::Status;
                        pipe.len = 0;
                        pipe.req = Request::In;
                        TickResult::Send
                    }
                    _ => todo!(),
                },
                TransferPhase::Data => match self.ty {
                    TransferType::ControlIn => {
                        let len = pipe.len.min(self.target_len - self.actual_len) as usize;
                        self.buf[self.actual_len as usize..][..len]
                            .copy_from_slice(&pipe.buf[0..len]);
                        self.actual_len += len as u16;
                        if self.actual_len == self.target_len || (len as u16) < self.packet_size {
                            console!("IN {:02x?}", &self.buf[0..self.actual_len as usize]);
                            self.phase = TransferPhase::Status;
                            pipe.len = 0;
                            pipe.toggle = 1;
                            pipe.req = Request::Out;
                        }
                        TickResult::Send
                    }
                    TransferType::PollInterrupt => {
                        let len = pipe.len as usize;
                        self.buf[..len].copy_from_slice(&pipe.buf[..len]);
                        self.actual_len = len as u16;
                        TickResult::Done
                    }
                    _ => todo!(),
                },
                TransferPhase::Status | TransferPhase::Delay => {
                    self.phase = TransferPhase::Idle;
                    //console!("status done");
                    TickResult::Done
                }
                _ => todo!(),
            }
        } else {
            TickResult::Error
        }
    }

    fn setup(&mut self, setup: SetupPacket) {
        if let Some(pipe) = &mut self.pipe {
            let len = setup.length();
            pipe.setup(setup);
            self.phase = TransferPhase::Setup;
            self.ty = if len == 0 {
                TransferType::ControlNone
            } else {
                TransferType::ControlIn
            };
            self.actual_len = 0;
            self.target_len = len;
        }
    }

    fn delay(&mut self, delay_ms: u16) {
        if let Some(pipe) = &mut self.pipe {
            pipe.timer = delay_ms;
            pipe.req = Request::Delay;
            self.phase = TransferPhase::Delay;
        }
    }
}

impl UsbTask {
    // Safety: only call this once.
    pub unsafe fn new() -> Self {
        let mut iface = UserIface::new();
        let mut pipe = iface.pipe(0);
        let setup = SetupPacket::new(
            DEVICE_TO_HOST,
            GET_DESCRIPTOR,
            (DEVICE_DESCRIPTOR as u16) << 8,
            0,
            0x12,
        );
        let mut transfers = [const { Transfer::new() }; N_PIPES];
        transfers[0].ty = TransferType::ControlIn;
        transfers[0].phase = TransferPhase::Setup;
        // TODO: packet size should be derived from device descriptor
        transfers[0].packet_size = 64;
        transfers[0].target_len = 18;
        transfers[0].actual_len = 0;
        pipe.addr = 0;
        pipe.setup(setup);
        iface.send_req(pipe);
        let hub = HubTask::default();
        let pipe_tasks = [const { PipeTask::new() }; N_PIPES];
        Self {
            iface,
            transfers,
            transfers_free: 1u32.unbounded_shl(N_PIPES as u32).wrapping_sub(2),
            hub,
            pipe_tasks,
        }
    }

    /// Poll the USB bus, advancing the state machine.
    ///
    /// This function should be called periodically. There is currently no waker
    /// mechanism in place. Generally the timing is fairly relaxed, but excessive
    /// delays may cause failures (for example, 50ms for set address commands).
    ///
    /// It will probably evolve to return a result, basically a stream of events.
    pub fn poll(&mut self) {
        let mut bits = self.iface.poll();
        while bits != 0 {
            let ix = bits.trailing_zeros() as usize;
            let pipe = self.iface.pipe(ix);
            if pipe.status != Status::Success {
                console!("pipe {ix} status {:?}", pipe.status);
            }
            self.transfers[ix].pipe = Some(pipe);
            //console!("polled {ix}, status = {:?}", pipe.status);
            let mut res = self.transfers[ix].tick();
            if res == TickResult::Done {
                if ix == 0 {
                    self.hub_tick();
                    res = TickResult::Send;
                } else {
                    res = self.pipe_tick(ix);
                }
            }
            match res {
                TickResult::Send => {
                    if let Some(pipe) = self.transfers[ix].pipe.take() {
                        self.iface.send_req(pipe);
                    }
                }
                TickResult::Done => {
                    self.free_transfer(ix);
                    console!("pipe {ix} done");
                }
                _ => (),
            }
            bits &= bits - 1;
        }
    }

    fn alloc_transfer(&mut self) -> usize {
        let ix = self.transfers_free.trailing_zeros() as usize;
        // TODO: handle failure
        self.transfers[ix].pipe = Some(self.iface.pipe(ix));
        ix
    }

    fn free_transfer(&mut self, ix: usize) {
        if let Some(pipe) = self.transfers[ix].pipe.take() {
            self.iface.release_pipe(pipe);
        }
        self.transfers_free |= 1 << ix;
    }

    fn hub_tick(&mut self) {
        let transfer = &mut self.transfers[0];
        match self.hub.hub_state {
            HubState::Configuring => match self.hub.state {
                // maybe reduce duplication between this and device setup
                DeviceState::Default => {
                    let setup = SetupPacket::new(HOST_TO_DEVICE, SET_ADDRESS, 1, 0, 0);
                    transfer.setup(setup);
                    self.hub.state = DeviceState::Address;
                }
                DeviceState::Address => {
                    // TODO: should implement 2ms delay here, per 9.2.6.3
                    let setup = SetupPacket::new(HOST_TO_DEVICE, SET_CONFIGURATION, 1, 0, 0);
                    transfer.pipe.as_mut().unwrap().addr = 1;
                    transfer.setup(setup);
                    self.hub.state = DeviceState::Configured;
                }
                DeviceState::Configured => {
                    let setup = SetupPacket::new(
                        DEVICE_TO_HOST | CLASS_REQUEST,
                        GET_DESCRIPTOR,
                        (HUB_DESCRIPTOR as u16) << 8,
                        0,
                        9,
                    );
                    transfer.setup(setup);
                    self.hub.hub_state = HubState::PowerPort(1);
                }
            },
            HubState::PowerPort(port) => {
                if port == 1 {
                    console!("hub desc {:02x?}", &transfer.buf[0..9]);
                }
                let setup = SetupPacket::new(
                    HOST_TO_DEVICE | CLASS_REQUEST | RECIPIENT_OTHER,
                    SET_FEATURE,
                    PORT_POWER,
                    port.into(),
                    0,
                );
                transfer.setup(setup);
                // TODO: iterate through ports, though probably not necessary for CH334F
                self.hub.hub_state = HubState::Polling;
            }
            HubState::Polling => {
                if let Some(pipe) = &mut transfer.pipe {
                    pipe.ep = 1;
                    pipe.req = Request::In;
                }
                transfer.ty = TransferType::PollInterrupt;
                transfer.phase = TransferPhase::Data;
                transfer.target_len = 1;
                self.hub.hub_state = HubState::GotPoll;
            }
            HubState::GotPoll => {
                // spawn a task to query port status
                if !self.hub.ignore_hub_events {
                    let port = transfer.buf[0].trailing_zeros();
                    self.hub.port = port as u16;
                    let new_transfer_ix = self.alloc_transfer();
                    console!("investigating port {port}, new_transfer_ix = {new_transfer_ix}");
                    let setup = SetupPacket::new(
                        DEVICE_TO_HOST | CLASS_REQUEST | RECIPIENT_OTHER,
                        GET_STATUS,
                        0,
                        self.hub.port,
                        4,
                    );
                    self.transfers[new_transfer_ix].setup(setup);
                    if let Some(mut pipe) = self.transfers[new_transfer_ix].pipe.take() {
                        pipe.addr = 1;
                        self.iface.send_req(pipe);
                    }
                    self.pipe_tasks[new_transfer_ix].state = PipeState::GotPortStatus;
                    self.hub.ignore_hub_events = true;
                }
            }
        }
    }

    fn pipe_tick(&mut self, ix: usize) -> TickResult {
        let transfer = &mut self.transfers[ix];
        let task = &mut self.pipe_tasks[ix];
        match task.state {
            PipeState::GotPortStatus => {
                let status = u16::from_le_bytes(transfer.buf[0..2].try_into().unwrap());
                self.hub.changes = u16::from_le_bytes(transfer.buf[2..4].try_into().unwrap());
                if status & self.hub.changes & 1 != 0 {
                    self.hub.pending_resets |= 1 << self.hub.port;
                }
                let change = self.hub.changes.trailing_zeros();
                console!(
                    "status {status:04x} changes {:04x}, change = {change}",
                    self.hub.changes
                );
                let setup = SetupPacket::new(
                    HOST_TO_DEVICE | CLASS_REQUEST | RECIPIENT_OTHER,
                    CLEAR_FEATURE,
                    change as u16 + 16,
                    self.hub.port,
                    0,
                );
                transfer.setup(setup);
                self.hub.changes &= self.hub.changes - 1;
                task.state = if change == 4 {
                    PipeState::GotReset
                } else {
                    PipeState::DidClearFeature
                };
            }
            PipeState::DidClearFeature => {
                console!("did clear feature");
                // TODO: handle more changes
                if self.hub.changes == 0 {
                    self.hub.ignore_hub_events = false;
                }
                if self.hub.pending_resets != 0 {
                    self.hub.resetting = self.hub.pending_resets.trailing_zeros() as u16;
                    let setup = SetupPacket::new(
                        HOST_TO_DEVICE | CLASS_REQUEST | RECIPIENT_OTHER,
                        SET_FEATURE,
                        PORT_RESET,
                        self.hub.resetting,
                        0,
                    );
                    transfer.setup(setup);
                    task.state = PipeState::DidReset;
                    self.hub.pending_resets &= self.hub.pending_resets - 1;
                } else {
                    return TickResult::Done;
                }
            }
            PipeState::DidReset => {
                console!("did reset");
                return TickResult::Done;
            }
            PipeState::GotReset => {
                console!("got reset");
                // It's possible we can tighten this; 7.1.7.5 dictates 50ms total, but we've
                // had some delay from the hub. No harm done though.
                transfer.delay(50);
                task.state = PipeState::AfterResetDelay;
            }
            PipeState::AfterResetDelay => {
                console!("got reset delay");
                // should probably check that port status enabled flag is set
                // Device has been reset, is now in default state, with address 0 (see Figure 9-1)
                let setup = SetupPacket::new(
                    DEVICE_TO_HOST,
                    GET_DESCRIPTOR,
                    (DEVICE_DESCRIPTOR as u16) << 8,
                    0,
                    0x8,
                );
                transfer.packet_size = 8;
                transfer.setup(setup);
                if let Some(pipe) = &mut transfer.pipe {
                    pipe.addr = 0;
                }
                task.state = PipeState::GotDesc;
            }
            PipeState::GotDesc => {
                console!("device desc {:02x?}", &transfer.buf[0..8]);
                transfer.packet_size = transfer.buf[7] as u16;
                // probably should get full descriptor here, but we cut corners
                // TODO: allocate address, for now hardcode as 2
                let setup = SetupPacket::new(HOST_TO_DEVICE, SET_ADDRESS, 2, 0, 0);
                transfer.setup(setup);
                task.state = PipeState::Address;
                // the user task is crashing intermittently here for unknown reasons
            }
            PipeState::Address => {
                console!("address");
                transfer.delay(100);
                task.state = PipeState::AfterAddressDelay;
            }
            PipeState::AfterAddressDelay => {
                transfer.pipe.as_mut().unwrap().addr = 2;
                let setup = SetupPacket::new(HOST_TO_DEVICE, SET_CONFIGURATION, 1, 0, 0);
                transfer.setup(setup);
                task.state = PipeState::Configured;
            }
            PipeState::Configured => {
                if let Some(pipe) = &mut transfer.pipe {
                    pipe.ep = 1;
                    pipe.req = Request::In;
                }
                transfer.ty = TransferType::PollInterrupt;
                transfer.phase = TransferPhase::Data;
                transfer.target_len = 8;
                task.state = PipeState::GotPoll;
            }
            PipeState::GotPoll => {
                console!("report {:02x?}", &transfer.buf[0..8]);
            }
            _ => return TickResult::Error, // shouldn't happen
        }
        TickResult::Send
    }
}

impl PipeTask {
    const fn new() -> Self {
        Self {
            state: PipeState::Idle,
        }
    }
}
