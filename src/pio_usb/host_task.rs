//! A task that is continuously polled to run the USB host role.

use crate::pio_usb::{
    iface::{Pipe, PipeGuard, Request, Status, UserIface},
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
    hub: HubTask,
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
    ControlOut,
    PollInterrupt,
}

enum TransferPhase {
    Idle,
    Setup,
    Data,
    Status,
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
    // This is a hack that will go away when we stop overloading the interrupt pipe
    save_toggle: u8,
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
    GotPortStatus,
    DidClearFeature,
    GotReset,
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
    const fn default() -> Self {
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
                TransferPhase::Status => {
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
        let mut transfers = [const { Transfer::default() }; N_PIPES];
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
        Self {
            iface,
            transfers,
            hub,
        }
    }

    pub fn poll(&mut self) {
        let mut bits = self.iface.poll();
        while bits != 0 {
            let ix = bits.trailing_zeros() as usize;
            self.transfers[ix].pipe = Some(self.iface.pipe(ix));
            //console!("polled {ix}, status = {:?}", pipe.status);
            let mut res = self.transfers[ix].tick();
            if res == TickResult::Done && ix == 0 {
                self.hub_tick();
                res = TickResult::Send;
            }
            match res {
                TickResult::Send => {
                    if let Some(pipe) = self.transfers[ix].pipe.take() {
                        self.iface.send_req(pipe);
                    }
                }
                TickResult::Done => {
                    console!("pipe {ix} done");
                }
                _ => (),
            }
            bits &= bits - 1;
        }
    }

    fn hub_tick(&mut self) {
        let transfer = &mut self.transfers[0];
        match self.hub.hub_state {
            HubState::Configuring => match self.hub.state {
                DeviceState::Default => {
                    let setup = SetupPacket::new(HOST_TO_DEVICE, SET_ADDRESS, 1, 0, 0);
                    transfer.setup(setup);
                    self.hub.state = DeviceState::Address;
                }
                DeviceState::Address => {
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
                self.hub.hub_state = HubState::Polling;
            }
            HubState::Polling | HubState::DidClearFeature => {
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
                let port = transfer.buf[0].trailing_zeros();
                console!("investigating port {port}");
                self.hub.port = port as u16;
                let setup = SetupPacket::new(
                    DEVICE_TO_HOST | CLASS_REQUEST | RECIPIENT_OTHER,
                    GET_STATUS,
                    0,
                    self.hub.port,
                    4,
                );
                // TODO: alloc new transfer here
                transfer.setup(setup);
                self.hub.hub_state = HubState::GotPortStatus;
            }
            HubState::GotPortStatus => {
                let status = u16::from_le_bytes(transfer.buf[0..2].try_into().unwrap());
                self.hub.changes = u16::from_le_bytes(transfer.buf[2..4].try_into().unwrap());
                console!("status {status:04x} changes {:04x}", self.hub.changes);
                if status & self.hub.changes & 1 != 0 {
                    self.hub.pending_resets |= 1 << self.hub.port;
                }
                // TODO: handle changes = 0 (unexpected)
                let change = self.hub.changes.trailing_zeros();
                console!("change = {change}");
                let setup = SetupPacket::new(
                    HOST_TO_DEVICE | CLASS_REQUEST | RECIPIENT_OTHER,
                    CLEAR_FEATURE,
                    change as u16 + 16,
                    self.hub.port,
                    0,
                );
                transfer.setup(setup);
                self.hub.changes &= self.hub.changes - 1;
                self.hub.hub_state = if change == 4 {
                    HubState::GotReset
                } else {
                    HubState::DidClearFeature
                };
            }

            _ => todo!(),
        }
    }
}

impl HubTask {
    fn tick(&mut self, transfer: &mut Transfer) -> TickResult {
        let pipe = transfer.pipe.as_mut().unwrap();
        match self.hub_state {
            HubState::Configuring => match self.state {
                DeviceState::Default => {
                    let setup = SetupPacket::new(HOST_TO_DEVICE, SET_ADDRESS, 1, 0, 0);
                    pipe.setup(setup);
                    transfer.phase = TransferPhase::Setup;
                    transfer.ty = TransferType::ControlNone;
                    self.state = DeviceState::Address;
                    TickResult::Send
                }
                DeviceState::Address => {
                    let setup = SetupPacket::new(HOST_TO_DEVICE, SET_CONFIGURATION, 1, 0, 0);
                    pipe.addr = 1;
                    pipe.setup(setup);
                    transfer.phase = TransferPhase::Setup;
                    transfer.ty = TransferType::ControlNone;
                    self.state = DeviceState::Configured;
                    TickResult::Send
                }
                DeviceState::Configured => {
                    let setup = SetupPacket::new(
                        DEVICE_TO_HOST | CLASS_REQUEST,
                        GET_DESCRIPTOR,
                        (HUB_DESCRIPTOR as u16) << 8,
                        0,
                        9,
                    );
                    pipe.setup(setup);
                    transfer.phase = TransferPhase::Setup;
                    transfer.ty = TransferType::ControlIn;
                    transfer.target_len = 9;
                    transfer.actual_len = 0;
                    self.hub_state = HubState::PowerPort(1);
                    // TODO: probably a good place to add a delay mechanism
                    TickResult::Send
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
                pipe.setup(setup);
                transfer.phase = TransferPhase::Setup;
                transfer.ty = TransferType::ControlNone;
                // TODO: iterate through the ports
                // (though apparently the CH334F doesn't really need this)
                self.hub_state = HubState::Polling;
                TickResult::Send
            }
            HubState::Polling => {
                pipe.ep = 1;
                pipe.req = Request::In;
                pipe.toggle = self.save_toggle;
                transfer.ty = TransferType::PollInterrupt;
                transfer.phase = TransferPhase::Data;
                transfer.target_len = 1;
                transfer.actual_len = 0;
                self.hub_state = HubState::GotPoll;
                TickResult::Send
            }
            HubState::GotPoll => {
                self.save_toggle = pipe.toggle;
                console!("poll result {:02x?}", &transfer.buf[0..1]);
                // This is a bit hinky; we're reusing the interrupt pipe to do hub work
                // Probably should just fold this logic into UsbTask
                let port = transfer.buf[0].trailing_zeros();
                console!("investigating port {port}");
                self.port = port as u16;
                let setup = SetupPacket::new(
                    DEVICE_TO_HOST | CLASS_REQUEST | RECIPIENT_OTHER,
                    GET_STATUS,
                    0,
                    self.port,
                    4,
                );
                pipe.setup(setup);
                transfer.phase = TransferPhase::Setup;
                transfer.ty = TransferType::ControlIn;
                transfer.target_len = 4;
                transfer.actual_len = 0;
                self.hub_state = HubState::GotPortStatus;
                TickResult::Send
            }
            HubState::GotPortStatus => {
                let status = u16::from_le_bytes(transfer.buf[0..2].try_into().unwrap());
                self.changes = u16::from_le_bytes(transfer.buf[2..4].try_into().unwrap());
                console!("status {status:04x} changes {:04x}", self.changes);
                if status & self.changes & 1 != 0 {
                    self.pending_resets |= 1 << self.port;
                }
                // TODO: handle changes = 0 (unexpected)
                let change = self.changes.trailing_zeros();
                console!("change = {change}");
                let setup = SetupPacket::new(
                    HOST_TO_DEVICE | CLASS_REQUEST | RECIPIENT_OTHER,
                    CLEAR_FEATURE,
                    change as u16 + 16,
                    self.port,
                    0,
                );
                pipe.setup(setup);
                transfer.phase = TransferPhase::Setup;
                transfer.ty = TransferType::ControlNone;
                self.changes &= self.changes - 1;
                self.hub_state = if change == 4 {
                    HubState::GotReset
                } else {
                    HubState::DidClearFeature
                };
                TickResult::Send
            }
            HubState::DidClearFeature => {
                // TODO: handle additional changes
                if self.pending_resets != 0 {
                    self.resetting = self.pending_resets.trailing_zeros() as u16;
                    let setup = SetupPacket::new(
                        HOST_TO_DEVICE | CLASS_REQUEST | RECIPIENT_OTHER,
                        SET_FEATURE,
                        PORT_RESET,
                        self.resetting,
                        0,
                    );
                    pipe.setup(setup);
                    transfer.phase = TransferPhase::Setup;
                    transfer.ty = TransferType::ControlNone;
                    self.hub_state = HubState::Polling;
                    self.pending_resets &= self.pending_resets - 1;
                    TickResult::Send
                } else {
                    TickResult::Done
                }
            }
            HubState::GotReset => {
                console!("got reset, resetting {}", self.resetting);
                TickResult::Done
            }
        }
    }
}
