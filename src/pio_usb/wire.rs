#![expect(unused)]

pub const SYNC: u8 = 0x80;
pub const PID_ACK: u8 = 0xd2;
pub const PID_DATA0: u8 = 0xc3;
pub const PID_DATA1: u8 = 0x4b;
pub const PID_IN: u8 = 0x69;
pub const PID_NAK: u8 = 0x5a;
pub const PID_OUT: u8 = 0xe1;
pub const PID_SOF: u8 = 0xa5;
pub const PID_SETUP: u8 = 0x2d;

// request types

pub const DEVICE_TO_HOST: u8 = 0x80;

pub const HOST_TO_DEVICE: u8 = 0x00;

pub const CLASS_REQUEST: u8 = 0x20;

pub const RECIPIENT_DEVICE: u8 = 0;

pub const RECIPIENT_INTERFACE: u8 = 1;

pub const RECIPIENT_ENDPOINT: u8 = 2;

pub const RECIPIENT_OTHER: u8 = 3;

// requests

pub const GET_STATUS: u8 = 0;

pub const CLEAR_FEATURE: u8 = 1;

pub const SET_FEATURE: u8 = 3;

pub const SET_ADDRESS: u8 = 5;

pub const GET_DESCRIPTOR: u8 = 6;

pub const SET_CONFIGURATION: u8 = 9;

// features

pub const PORT_RESET: u16 = 4;

pub const PORT_POWER: u16 = 8;

// descriptor types

pub const DEVICE_DESCRIPTOR: u8 = 1;

pub const HUB_DESCRIPTOR: u8 = 0x29;
