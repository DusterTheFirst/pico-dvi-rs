use embedded_hal::digital::StatefulOutputPin;
use rp235x_hal::pio::PIOExt;

use crate::pio_usb::{
    host::SetupPacket,
    wire::{
        CLASS_REQUEST, DEVICE_DESCRIPTOR, DEVICE_TO_HOST, GET_DESCRIPTOR, GET_STATUS,
        HOST_TO_DEVICE, PID_ACK, PID_IN, PID_NAK, PORT_RESET, RECIPIENT_OTHER, SET_ADDRESS,
        SET_CONFIGURATION, SET_FEATURE,
    },
    RxResult, UsbPio,
};

const N_DEVICE: usize = 4;

const N_PORT: usize = 5;

#[derive(Default)]
pub struct UsbBus {
    devices: [Device; N_DEVICE],
    ports: [Port; N_PORT],
}

#[derive(Default)]
pub struct Device {
    state: DeviceState,
    timer: u32,
    interval: u32,
}

#[derive(Default)]
pub struct Port {
    state: PortState,
    address: u8,
    timer: u32,
}

#[derive(Clone, Copy, PartialEq, Default)]
enum DeviceState {
    #[default]
    Empty,
    InReset,
}

#[derive(Clone, Copy, PartialEq, Default)]
enum PortState {
    #[default]
    Empty,
    InReset,
    Powered,
    Enabled,
    AddressWait,
    Configured,
}

// Currently only deal with one hub; we'll make this dynamic
const HUB_ADDRESS: u8 = 1;

impl<PIO: PIOExt> UsbPio<PIO> {
    #[link_section = ".data"]
    fn set_port_feature(&mut self, hub_addr: u8, port: u8, feature: u16) {
        let setup = SetupPacket::new(
            HOST_TO_DEVICE | CLASS_REQUEST | RECIPIENT_OTHER,
            SET_FEATURE,
            feature,
            port as u16,
            0,
        );
        _ = self.control_transfer_none(setup, hub_addr);
    }

    #[link_section = ".data"]
    fn get_port_status(&mut self, port: u8) -> (u16, u16) {
        let setup = SetupPacket::new(
            DEVICE_TO_HOST | CLASS_REQUEST | RECIPIENT_OTHER,
            GET_STATUS,
            0,
            port as u16,
            4,
        );
        let _rx_result = self.control_transfer_in(setup, HUB_ADDRESS);
        let state = u16::from_le_bytes(self.buf[2..4].try_into().unwrap());
        let changes = u16::from_le_bytes(self.buf[4..6].try_into().unwrap());
        (state, changes)
    }

    #[link_section = ".data"]
    pub fn handle_hub_packet(&mut self, mut packet: u16) {
        while packet != 0 {
            let ix = packet.trailing_zeros();
            console!("investigate port {ix}");
            // clear lowest set bit
            packet &= packet.wrapping_sub(1);
            let (state, mut changes) = self.get_port_status(ix as u8);
            console!("get_status state = {state:x} changes = {changes:x}");
            while changes != 0 {
                let bit = changes.trailing_zeros();
                _ = self.clear_port_feature(1, ix as u8, (bit + 16) as u16);
                if bit == 0 {
                    // C_PORT_CONNECTION
                    if (state & 1) != 0 {
                        // now connected
                        self.set_port_feature(HUB_ADDRESS, ix as u8, PORT_RESET);
                    }
                    self.bus.ports[ix as usize].state = PortState::InReset;
                    self.bus.ports[ix as usize].timer = 50;
                }
                changes &= changes.wrapping_sub(1);
            }
        }
    }

    #[link_section = ".data"]
    pub fn tick_bus(&mut self) {
        for port in 0..N_PORT {
            if self.bus.ports[port].state == PortState::InReset {
                if self.bus.ports[port].timer > 0 {
                    self.bus.ports[port].timer -= 1;
                    if self.bus.ports[port].timer == 0 {
                        let (state, _changes) = self.get_port_status(port as u8);
                        if (state & 2) != 0 {
                            let setup = SetupPacket::new(
                                DEVICE_TO_HOST,
                                GET_DESCRIPTOR,
                                (DEVICE_DESCRIPTOR as u16) << 8,
                                0,
                                0x8,
                            );
                            let rx_result = self.control_transfer_in(setup, 0);
                            if rx_result == Some(PID_ACK) {
                                self.bus.ports[port].state = PortState::Enabled;
                                console!("get_desc buf_ix = {}", self.buf_ix);
                                for i in 0..self.buf_ix {
                                    console!("data: {:02x}", self.buf[i]);
                                }
                            } else {
                                self.bus.ports[port].state = PortState::Powered;
                                console!("fail rx_result = {rx_result:x?}");
                            }
                        }
                    }
                }
            }
            if self.bus.ports[port].state == PortState::Enabled {
                self.debug.toggle();
                let setup = SetupPacket::new(HOST_TO_DEVICE, SET_ADDRESS, 2, 0, 0);
                let rx_result = self.control_transfer_none(setup, 0);
                if rx_result != Some(PID_ACK) {
                    console!("rx set addr {rx_result:x?}");
                }
                self.bus.ports[port].state = PortState::AddressWait;
                let setup = SetupPacket::new(HOST_TO_DEVICE, SET_CONFIGURATION, 1, 0, 0);
                let rx_result = self.control_transfer_none(setup, 2);
                if rx_result != Some(PID_ACK) {
                    console!("rx set conf {rx_result:x?}");
                }
                self.bus.ports[port].state = PortState::Configured;
            }
            if self.bus.ports[port].state == PortState::Configured {
                self.tx_token(PID_IN, 2, 1);
                let rx_result = self.rx_packet();
                if rx_result == RxResult::Ok && self.buf[1] != PID_NAK {
                    self.tx_handshake(PID_ACK);
                    for i in 0..self.buf_ix {
                        console!("report {i}: {:02x}", self.buf[i]);
                    }
                }
            }
        }
    }
}
