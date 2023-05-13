use embedded_graphics::{
    pixelcolor::{raw::RawU8, Rgb565},
    prelude::{PixelColor, RgbColor, WebColors},
};
use modular_bitfield::{
    bitfield,
    specifiers::{B2, B3},
};

// VGA
const WIDTH: usize = 320;
const HEIGHT: usize = 240;

pub static mut FRAMEBUFFER_16BPP: [Rgb565; WIDTH * HEIGHT] = [Rgb565::CSS_GOLD; WIDTH * HEIGHT];
pub static mut FRAMEBUFFER_8BPP: [Rgb332; WIDTH * HEIGHT] = [Rgb332::WHITE; WIDTH * HEIGHT];

#[bitfield(bytes = 1)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb332 {
    red: B3,
    green: B3,
    blue: B2,
}

impl PixelColor for Rgb332 {
    type Raw = RawU8;
}

#[allow(clippy::unusual_byte_groupings)]
impl RgbColor for Rgb332 {
    const MAX_R: u8 = 0b111;
    const MAX_G: u8 = 0b111;
    const MAX_B: u8 = 0b11;

    const BLACK: Self = Rgb332::new();
    const WHITE: Self = Rgb332::from_bytes([0b111_111_11]);

    const RED: Self = Rgb332::from_bytes([0b111_000_00]);
    const GREEN: Self = Rgb332::from_bytes([0b000_111_00]);
    const BLUE: Self = Rgb332::from_bytes([0b000_000_11]);

    const YELLOW: Self = Rgb332::from_bytes([0b111_111_00]);
    const MAGENTA: Self = Rgb332::from_bytes([0b111_000_11]);
    const CYAN: Self = Rgb332::from_bytes([0b000_111_11]);

    fn r(&self) -> u8 {
        self.red()
    }

    fn g(&self) -> u8 {
        self.green()
    }

    fn b(&self) -> u8 {
        self.blue()
    }
}

pub struct FrameBuffer {}
