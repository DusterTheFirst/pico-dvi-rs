use rp235x_hal::pio::PIOExt;

use crate::pio_usb::{
    wire::{PID_ACK, PID_DATA0, PID_DATA1, PID_IN, PID_OUT, PID_SETUP},
    RxResult, UsbPio,
};

#[derive(Copy, Clone)]
pub struct SetupPacket {
    raw: [u8; 8],
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
}

impl<PIO: PIOExt> UsbPio<PIO> {
    #[link_section = ".data"]
    fn control_transfer(&mut self, setup: SetupPacket, addr: u8, endpoint: u8) -> Option<u8> {
        self.tx_token(PID_SETUP, addr, endpoint);
        self.tx_data(PID_DATA0, &setup.raw);
        self.rx_handshake()
    }

    // Leaves input in buffer.
    #[link_section = ".data"]
    pub fn control_transfer_in(
        &mut self,
        setup: SetupPacket,
        addr: u8,
        endpoint: u8,
    ) -> Option<u8> {
        if self.control_transfer(setup, addr, endpoint) != Some(PID_ACK) {
            return None;
        }
        // data stage
        self.tx_token(PID_IN, addr, endpoint);
        if self.rx_packet() != RxResult::Ok {
            // TODO: check other constraints including PID
            return None;
        }
        // status stage, acknowledge
        self.tx_handshake(PID_ACK);
        self.tx_token(PID_OUT, addr, endpoint);
        self.tx_data(PID_DATA1, &[]);
        self.rx_handshake()
    }

    #[link_section = ".data"]
    pub fn control_transfer_none(
        &mut self,
        setup: SetupPacket,
        addr: u8,
        endpoint: u8,
    ) -> Option<u8> {
        if self.control_transfer(setup, addr, endpoint) != Some(PID_ACK) {
            return None;
        }
        // data stage
        self.tx_token(PID_IN, addr, endpoint);
        if self.rx_packet() != RxResult::Ok {
            // TODO: check other constraints including PID
            return None;
        }
        // status stage, acknowledge
        self.tx_handshake(PID_ACK);
        Some(PID_ACK)
    }
}
