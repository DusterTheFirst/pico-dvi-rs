//! Configuration of the DVI pinout.

/// Assignment of HSTX pin pair.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DviPair {
    D0,
    D1,
    D2,
    Clk,
}

/// Polarity of HSTX pins
///
/// This is the polarity of the lower of the two pins in a pin pair.
#[allow(unused)]
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DviPolarity {
    Pos,
    Neg,
}

#[derive(Clone, Copy)]
pub struct DviPinout {
    pins: [DviPair; 4],
    polarity: DviPolarity,
}

impl DviPinout {
    pub const fn new(pins: [DviPair; 4], polarity: DviPolarity) -> Self {
        Self { pins, polarity }
    }

    pub(crate) const fn cfg_bits(&self, pin: usize) -> u32 {
        let pair = self.pins[pin / 2];
        let mut bits = match pair {
            DviPair::Clk => 1 << 17, // CLK
            _ => {
                let perm = pair as u8 as u32 * 10;
                perm | ((perm + 1) << 8) // SEL_P | SEL_N
            }
        };
        if pin % 2 != self.polarity as u8 as usize {
            bits |= 1 << 16; // INV
        }
        bits
    }
}
