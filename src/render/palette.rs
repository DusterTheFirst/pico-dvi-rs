use crate::dvi::tmds::TmdsPair;

#[repr(C)]
pub struct PaletteEntry {
    blue: TmdsPair,
    green: TmdsPair,
    red: TmdsPair,
    padding: u32,
}

impl PaletteEntry {
    /// Create a palette entry for a pair of gray pixels.
    ///
    /// The values *should* be chosen so the result is DC-balanced, but
    /// that isn't enforced.
    pub const fn gray_pair(gray0: u8, gray1: u8) -> PaletteEntry {
        let pair = TmdsPair::encode_pair(gray0, gray1);
        PaletteEntry { blue: pair, green: pair, red: pair, padding: 0 }
    }
}

#[link_section = ".scratch_x"]
pub static BW_PALETTE: [PaletteEntry; 4] = [
    PaletteEntry::gray_pair(0, 1),
    PaletteEntry::gray_pair(0xff, 1),
    PaletteEntry::gray_pair(0, 0xfe),
    PaletteEntry::gray_pair(0xff, 0xfe),
];