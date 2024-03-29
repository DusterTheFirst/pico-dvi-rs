use crate::dvi::tmds::TmdsPair;

#[repr(C)]
#[derive(Clone, Copy)]
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
        PaletteEntry {
            blue: pair,
            green: pair,
            red: pair,
            padding: 0,
        }
    }

    /// Create a palette entry for a pair of colors.
    ///
    /// The input colors are of the form 0xRRGGBB.
    ///
    /// The colors are quantized to 4 bits per component and chosen from a
    /// a precomputed palette of DC-balanced pairs.
    pub const fn quantized_4bit(color0: u32, color1: u32) -> PaletteEntry {
        // Quantize to 4 bits
        const fn extract_quantize(rgb: u32, ix: u32) -> usize {
            let val = (rgb >> (ix * 8)) & 0xff;
            ((val * 3855 + 32768) >> 16) as usize
        }

        PaletteEntry {
            blue: GRAY_4BPP_CONST[extract_quantize(color0, 0) + extract_quantize(color1, 0) * 16],
            green: GRAY_4BPP_CONST[extract_quantize(color0, 1) + extract_quantize(color1, 1) * 16],
            red: GRAY_4BPP_CONST[extract_quantize(color0, 2) + extract_quantize(color1, 2) * 16],
            padding: 0,
        }
    }
}

pub const fn quantized_1bpp_palette(color0: u32, color1: u32) -> [PaletteEntry; 4] {
    [
        PaletteEntry::quantized_4bit(color0, color0),
        PaletteEntry::quantized_4bit(color1, color0),
        PaletteEntry::quantized_4bit(color0, color1),
        PaletteEntry::quantized_4bit(color1, color1),
    ]
}

#[link_section = ".scratch_x"]
pub static BW_PALETTE: [PaletteEntry; 4] = [
    PaletteEntry::gray_pair(0, 1),
    PaletteEntry::gray_pair(0xff, 1),
    PaletteEntry::gray_pair(0, 0xfe),
    PaletteEntry::gray_pair(0xff, 0xfe),
];

#[link_section = ".data"]
pub static mut GLOBAL_PALETTE: [PaletteEntry; 256] = [PaletteEntry::gray_pair(0, 1); 256];

/// Initialize a palette from a list of 16 RGB colors.
///
/// The input colors are of the form 0xRRGGBB.
///
/// Note: this implementation quantizes to 4 bits per component and uses
/// a precomputed palette. A more precise approach is possible, at the cost
/// of more compute time (and code complexity).
pub fn init_4bpp_palette(pal: &mut [PaletteEntry; 256], rgb: &[u32; 16]) {
    // Get a component from the 24bpp RGB value, quantizing to 4 bits
    fn get(rgb: u32, ix: u32) -> usize {
        let val = (rgb >> (ix * 8)) & 0xff;
        ((val * 3855 + 32768) >> 16) as usize
    }
    for i in 0..256 {
        let rgb0 = rgb[i % 16];
        let rgb1 = rgb[i / 16];
        pal[i].blue = GRAY_4BPP[get(rgb0, 0) + get(rgb1, 0) * 16];
        pal[i].green = GRAY_4BPP[get(rgb0, 1) + get(rgb1, 1) * 16];
        pal[i].red = GRAY_4BPP[get(rgb0, 2) + get(rgb1, 2) * 16];
    }
}

// Static for use in non-const code
static GRAY_4BPP: [TmdsPair; 256] = GRAY_4BPP_CONST;
// Const for use in const fn as they can not refer to statics
const GRAY_4BPP_CONST: [TmdsPair; 256] = [
    // This was auto-generated by garden/tmds.py
    TmdsPair::encode_pair(0x00, 0x01), // (+0, +1)
    TmdsPair::encode_pair(0x0e, 0x02), // (-3, +2)
    TmdsPair::encode_pair(0x1f, 0x04), // (-3, +4)
    TmdsPair::encode_pair(0x31, 0x01), // (-2, +1)
    TmdsPair::encode_pair(0x41, 0x02), // (-3, +2)
    TmdsPair::encode_pair(0x52, 0x04), // (-3, +4)
    TmdsPair::encode_pair(0x66, 0x02), // (+0, +2)
    TmdsPair::encode_pair(0x78, 0x02), // (+1, +2)
    TmdsPair::encode_pair(0x87, 0x02), // (-1, +2)
    TmdsPair::encode_pair(0x99, 0x02), // (+0, +2)
    TmdsPair::encode_pair(0xa6, 0x04), // (-4, +4)
    TmdsPair::encode_pair(0xb8, 0x04), // (-3, +4)
    TmdsPair::encode_pair(0xce, 0x01), // (+2, +1)
    TmdsPair::encode_pair(0xda, 0x04), // (-3, +4)
    TmdsPair::encode_pair(0xeb, 0x04), // (-3, +4)
    TmdsPair::encode_pair(0xff, 0x01), // (+0, +1)
    TmdsPair::encode_pair(0x03, 0x0f), // (+3, -2)
    TmdsPair::encode_pair(0x11, 0x11), // (+0, +0)
    TmdsPair::encode_pair(0x22, 0x11), // (+0, +0)
    TmdsPair::encode_pair(0x33, 0x12), // (+0, +1)
    TmdsPair::encode_pair(0x44, 0x11), // (+0, +0)
    TmdsPair::encode_pair(0x55, 0x11), // (+0, +0)
    TmdsPair::encode_pair(0x68, 0x10), // (+2, -1)
    TmdsPair::encode_pair(0x77, 0x11), // (+0, +0)
    TmdsPair::encode_pair(0x88, 0x11), // (+0, +0)
    TmdsPair::encode_pair(0x98, 0x12), // (-1, +1)
    TmdsPair::encode_pair(0xaa, 0x11), // (+0, +0)
    TmdsPair::encode_pair(0xbb, 0x11), // (+0, +0)
    TmdsPair::encode_pair(0xcc, 0x12), // (+0, +1)
    TmdsPair::encode_pair(0xdd, 0x11), // (+0, +0)
    TmdsPair::encode_pair(0xee, 0x11), // (+0, +0)
    TmdsPair::encode_pair(0xfc, 0x14), // (-3, +3)
    TmdsPair::encode_pair(0x04, 0x1f), // (+4, -3)
    TmdsPair::encode_pair(0x11, 0x22), // (+0, +0)
    TmdsPair::encode_pair(0x22, 0x22), // (+0, +0)
    TmdsPair::encode_pair(0x35, 0x21), // (+2, -1)
    TmdsPair::encode_pair(0x44, 0x22), // (+0, +0)
    TmdsPair::encode_pair(0x55, 0x22), // (+0, +0)
    TmdsPair::encode_pair(0x64, 0x24), // (-2, +2)
    TmdsPair::encode_pair(0x77, 0x22), // (+0, +0)
    TmdsPair::encode_pair(0x88, 0x22), // (+0, +0)
    TmdsPair::encode_pair(0x9a, 0x21), // (+1, -1)
    TmdsPair::encode_pair(0xaa, 0x22), // (+0, +0)
    TmdsPair::encode_pair(0xbb, 0x22), // (+0, +0)
    TmdsPair::encode_pair(0xcb, 0x24), // (-1, +2)
    TmdsPair::encode_pair(0xdd, 0x22), // (+0, +0)
    TmdsPair::encode_pair(0xee, 0x22), // (+0, +0)
    TmdsPair::encode_pair(0xfa, 0x24), // (-5, +2)
    TmdsPair::encode_pair(0x03, 0x31), // (+3, -2)
    TmdsPair::encode_pair(0x0f, 0x35), // (-2, +2)
    TmdsPair::encode_pair(0x20, 0x34), // (-2, +1)
    TmdsPair::encode_pair(0x33, 0x32), // (+0, -1)
    TmdsPair::encode_pair(0x43, 0x34), // (-1, +1)
    TmdsPair::encode_pair(0x56, 0x32), // (+1, -1)
    TmdsPair::encode_pair(0x66, 0x33), // (+0, +0)
    TmdsPair::encode_pair(0x78, 0x33), // (+1, +0)
    TmdsPair::encode_pair(0x87, 0x33), // (-1, +0)
    TmdsPair::encode_pair(0x99, 0x33), // (+0, +0)
    TmdsPair::encode_pair(0xa8, 0x34), // (-2, +1)
    TmdsPair::encode_pair(0xbd, 0x32), // (+2, -1)
    TmdsPair::encode_pair(0xcc, 0x32), // (+0, -1)
    TmdsPair::encode_pair(0xde, 0x32), // (+1, -1)
    TmdsPair::encode_pair(0xed, 0x34), // (-1, +1)
    TmdsPair::encode_pair(0xfc, 0x33), // (-3, +0)
    TmdsPair::encode_pair(0x04, 0x40), // (+4, -4)
    TmdsPair::encode_pair(0x11, 0x44), // (+0, +0)
    TmdsPair::encode_pair(0x22, 0x44), // (+0, +0)
    TmdsPair::encode_pair(0x33, 0x43), // (+0, -1)
    TmdsPair::encode_pair(0x44, 0x44), // (+0, +0)
    TmdsPair::encode_pair(0x55, 0x44), // (+0, +0)
    TmdsPair::encode_pair(0x67, 0x43), // (+1, -1)
    TmdsPair::encode_pair(0x77, 0x44), // (+0, +0)
    TmdsPair::encode_pair(0x88, 0x44), // (+0, +0)
    TmdsPair::encode_pair(0x9b, 0x43), // (+2, -1)
    TmdsPair::encode_pair(0xaa, 0x44), // (+0, +0)
    TmdsPair::encode_pair(0xbb, 0x44), // (+0, +0)
    TmdsPair::encode_pair(0xcc, 0x43), // (+0, -1)
    TmdsPair::encode_pair(0xdd, 0x44), // (+0, +0)
    TmdsPair::encode_pair(0xee, 0x44), // (+0, +0)
    TmdsPair::encode_pair(0xfa, 0x48), // (-5, +4)
    TmdsPair::encode_pair(0x05, 0x51), // (+5, -4)
    TmdsPair::encode_pair(0x11, 0x55), // (+0, +0)
    TmdsPair::encode_pair(0x22, 0x55), // (+0, +0)
    TmdsPair::encode_pair(0x32, 0x56), // (-1, +1)
    TmdsPair::encode_pair(0x44, 0x55), // (+0, +0)
    TmdsPair::encode_pair(0x55, 0x55), // (+0, +0)
    TmdsPair::encode_pair(0x66, 0x56), // (+0, +1)
    TmdsPair::encode_pair(0x77, 0x55), // (+0, +0)
    TmdsPair::encode_pair(0x88, 0x55), // (+0, +0)
    TmdsPair::encode_pair(0x99, 0x56), // (+0, +1)
    TmdsPair::encode_pair(0xaa, 0x55), // (+0, +0)
    TmdsPair::encode_pair(0xbb, 0x55), // (+0, +0)
    TmdsPair::encode_pair(0xca, 0x56), // (-2, +1)
    TmdsPair::encode_pair(0xdd, 0x55), // (+0, +0)
    TmdsPair::encode_pair(0xee, 0x55), // (+0, +0)
    TmdsPair::encode_pair(0xfb, 0x56), // (-4, +1)
    TmdsPair::encode_pair(0x02, 0x66), // (+2, +0)
    TmdsPair::encode_pair(0x13, 0x65), // (+2, -1)
    TmdsPair::encode_pair(0x24, 0x64), // (+2, -2)
    TmdsPair::encode_pair(0x33, 0x66), // (+0, +0)
    TmdsPair::encode_pair(0x43, 0x67), // (-1, +1)
    TmdsPair::encode_pair(0x56, 0x65), // (+1, -1)
    TmdsPair::encode_pair(0x66, 0x67), // (+0, +1)
    TmdsPair::encode_pair(0x79, 0x65), // (+2, -1)
    TmdsPair::encode_pair(0x87, 0x67), // (-1, +1)
    TmdsPair::encode_pair(0x99, 0x67), // (+0, +1)
    TmdsPair::encode_pair(0xac, 0x64), // (+2, -2)
    TmdsPair::encode_pair(0xbd, 0x65), // (+2, -1)
    TmdsPair::encode_pair(0xcc, 0x66), // (+0, +0)
    TmdsPair::encode_pair(0xde, 0x65), // (+1, -1)
    TmdsPair::encode_pair(0xed, 0x67), // (-1, +1)
    TmdsPair::encode_pair(0xfd, 0x66), // (-2, +0)
    TmdsPair::encode_pair(0x02, 0x78), // (+2, +1)
    TmdsPair::encode_pair(0x11, 0x77), // (+0, +0)
    TmdsPair::encode_pair(0x22, 0x77), // (+0, +0)
    TmdsPair::encode_pair(0x33, 0x78), // (+0, +1)
    TmdsPair::encode_pair(0x44, 0x77), // (+0, +0)
    TmdsPair::encode_pair(0x55, 0x77), // (+0, +0)
    TmdsPair::encode_pair(0x68, 0x76), // (+2, -1)
    TmdsPair::encode_pair(0x77, 0x77), // (+0, +0)
    TmdsPair::encode_pair(0x88, 0x77), // (+0, +0)
    TmdsPair::encode_pair(0x98, 0x78), // (-1, +1)
    TmdsPair::encode_pair(0xaa, 0x77), // (+0, +0)
    TmdsPair::encode_pair(0xbb, 0x77), // (+0, +0)
    TmdsPair::encode_pair(0xcc, 0x78), // (+0, +1)
    TmdsPair::encode_pair(0xdd, 0x77), // (+0, +0)
    TmdsPair::encode_pair(0xee, 0x77), // (+0, +0)
    TmdsPair::encode_pair(0xfd, 0x78), // (-2, +1)
    TmdsPair::encode_pair(0x03, 0x86), // (+3, -2)
    TmdsPair::encode_pair(0x11, 0x88), // (+0, +0)
    TmdsPair::encode_pair(0x22, 0x88), // (+0, +0)
    TmdsPair::encode_pair(0x33, 0x87), // (+0, -1)
    TmdsPair::encode_pair(0x44, 0x88), // (+0, +0)
    TmdsPair::encode_pair(0x55, 0x88), // (+0, +0)
    TmdsPair::encode_pair(0x67, 0x87), // (+1, -1)
    TmdsPair::encode_pair(0x77, 0x88), // (+0, +0)
    TmdsPair::encode_pair(0x88, 0x88), // (+0, +0)
    TmdsPair::encode_pair(0x9b, 0x87), // (+2, -1)
    TmdsPair::encode_pair(0xaa, 0x88), // (+0, +0)
    TmdsPair::encode_pair(0xbb, 0x88), // (+0, +0)
    TmdsPair::encode_pair(0xcc, 0x87), // (+0, -1)
    TmdsPair::encode_pair(0xdd, 0x88), // (+0, +0)
    TmdsPair::encode_pair(0xee, 0x88), // (+0, +0)
    TmdsPair::encode_pair(0xfb, 0x8c), // (-4, +4)
    TmdsPair::encode_pair(0x03, 0x98), // (+3, -1)
    TmdsPair::encode_pair(0x13, 0x99), // (+2, +0)
    TmdsPair::encode_pair(0x21, 0x99), // (-1, +0)
    TmdsPair::encode_pair(0x33, 0x99), // (+0, +0)
    TmdsPair::encode_pair(0x42, 0x9a), // (-2, +1)
    TmdsPair::encode_pair(0x56, 0x99), // (+1, +0)
    TmdsPair::encode_pair(0x66, 0x98), // (+0, -1)
    TmdsPair::encode_pair(0x78, 0x98), // (+1, -1)
    TmdsPair::encode_pair(0x89, 0x97), // (+1, -2)
    TmdsPair::encode_pair(0x99, 0x98), // (+0, -1)
    TmdsPair::encode_pair(0xa9, 0x99), // (-1, +0)
    TmdsPair::encode_pair(0xb9, 0x9b), // (-2, +2)
    TmdsPair::encode_pair(0xcc, 0x99), // (+0, +0)
    TmdsPair::encode_pair(0xde, 0x99), // (+1, +0)
    TmdsPair::encode_pair(0xec, 0x9a), // (-2, +1)
    TmdsPair::encode_pair(0xfc, 0x98), // (-3, -1)
    TmdsPair::encode_pair(0x04, 0xa6), // (+4, -4)
    TmdsPair::encode_pair(0x11, 0xaa), // (+0, +0)
    TmdsPair::encode_pair(0x22, 0xaa), // (+0, +0)
    TmdsPair::encode_pair(0x35, 0xa9), // (+2, -1)
    TmdsPair::encode_pair(0x44, 0xaa), // (+0, +0)
    TmdsPair::encode_pair(0x55, 0xaa), // (+0, +0)
    TmdsPair::encode_pair(0x64, 0xac), // (-2, +2)
    TmdsPair::encode_pair(0x77, 0xaa), // (+0, +0)
    TmdsPair::encode_pair(0x88, 0xaa), // (+0, +0)
    TmdsPair::encode_pair(0x9a, 0xa9), // (+1, -1)
    TmdsPair::encode_pair(0xaa, 0xaa), // (+0, +0)
    TmdsPair::encode_pair(0xbb, 0xaa), // (+0, +0)
    TmdsPair::encode_pair(0xcb, 0xac), // (-1, +2)
    TmdsPair::encode_pair(0xdd, 0xaa), // (+0, +0)
    TmdsPair::encode_pair(0xee, 0xaa), // (+0, +0)
    TmdsPair::encode_pair(0xfb, 0xad), // (-4, +3)
    TmdsPair::encode_pair(0x04, 0xb8), // (+4, -3)
    TmdsPair::encode_pair(0x11, 0xbb), // (+0, +0)
    TmdsPair::encode_pair(0x22, 0xbb), // (+0, +0)
    TmdsPair::encode_pair(0x34, 0xb9), // (+1, -2)
    TmdsPair::encode_pair(0x44, 0xbb), // (+0, +0)
    TmdsPair::encode_pair(0x55, 0xbb), // (+0, +0)
    TmdsPair::encode_pair(0x68, 0xba), // (+2, -1)
    TmdsPair::encode_pair(0x77, 0xbb), // (+0, +0)
    TmdsPair::encode_pair(0x88, 0xbb), // (+0, +0)
    TmdsPair::encode_pair(0x9b, 0xb9), // (+2, -2)
    TmdsPair::encode_pair(0xaa, 0xbb), // (+0, +0)
    TmdsPair::encode_pair(0xbb, 0xbb), // (+0, +0)
    TmdsPair::encode_pair(0xcb, 0xbc), // (-1, +1)
    TmdsPair::encode_pair(0xdd, 0xbb), // (+0, +0)
    TmdsPair::encode_pair(0xee, 0xbb), // (+0, +0)
    TmdsPair::encode_pair(0xfd, 0xbe), // (-2, +3)
    TmdsPair::encode_pair(0x02, 0xcd), // (+2, +1)
    TmdsPair::encode_pair(0x12, 0xcc), // (+1, +0)
    TmdsPair::encode_pair(0x24, 0xcb), // (+2, -1)
    TmdsPair::encode_pair(0x33, 0xcd), // (+0, +1)
    TmdsPair::encode_pair(0x43, 0xcc), // (-1, +0)
    TmdsPair::encode_pair(0x57, 0xcb), // (+2, -1)
    TmdsPair::encode_pair(0x66, 0xcc), // (+0, +0)
    TmdsPair::encode_pair(0x78, 0xcc), // (+1, +0)
    TmdsPair::encode_pair(0x87, 0xcc), // (-1, +0)
    TmdsPair::encode_pair(0x99, 0xcc), // (+0, +0)
    TmdsPair::encode_pair(0xac, 0xcb), // (+2, -1)
    TmdsPair::encode_pair(0xbc, 0xcc), // (+1, +0)
    TmdsPair::encode_pair(0xcc, 0xcd), // (+0, +1)
    TmdsPair::encode_pair(0xdf, 0xcb), // (+2, -1)
    TmdsPair::encode_pair(0xed, 0xcc), // (-1, +0)
    TmdsPair::encode_pair(0xfd, 0xcd), // (-2, +1)
    TmdsPair::encode_pair(0x04, 0xda), // (+4, -3)
    TmdsPair::encode_pair(0x11, 0xdd), // (+0, +0)
    TmdsPair::encode_pair(0x22, 0xdd), // (+0, +0)
    TmdsPair::encode_pair(0x32, 0xde), // (-1, +1)
    TmdsPair::encode_pair(0x44, 0xdd), // (+0, +0)
    TmdsPair::encode_pair(0x55, 0xdd), // (+0, +0)
    TmdsPair::encode_pair(0x66, 0xde), // (+0, +1)
    TmdsPair::encode_pair(0x77, 0xdd), // (+0, +0)
    TmdsPair::encode_pair(0x88, 0xdd), // (+0, +0)
    TmdsPair::encode_pair(0x99, 0xde), // (+0, +1)
    TmdsPair::encode_pair(0xaa, 0xdd), // (+0, +0)
    TmdsPair::encode_pair(0xbb, 0xdd), // (+0, +0)
    TmdsPair::encode_pair(0xca, 0xde), // (-2, +1)
    TmdsPair::encode_pair(0xdd, 0xdd), // (+0, +0)
    TmdsPair::encode_pair(0xee, 0xdd), // (+0, +0)
    TmdsPair::encode_pair(0xfc, 0xe0), // (-3, +3)
    TmdsPair::encode_pair(0x04, 0xeb), // (+4, -3)
    TmdsPair::encode_pair(0x11, 0xee), // (+0, +0)
    TmdsPair::encode_pair(0x22, 0xee), // (+0, +0)
    TmdsPair::encode_pair(0x34, 0xed), // (+1, -1)
    TmdsPair::encode_pair(0x44, 0xee), // (+0, +0)
    TmdsPair::encode_pair(0x55, 0xee), // (+0, +0)
    TmdsPair::encode_pair(0x65, 0xf0), // (-1, +2)
    TmdsPair::encode_pair(0x77, 0xee), // (+0, +0)
    TmdsPair::encode_pair(0x88, 0xee), // (+0, +0)
    TmdsPair::encode_pair(0x9b, 0xed), // (+2, -1)
    TmdsPair::encode_pair(0xaa, 0xee), // (+0, +0)
    TmdsPair::encode_pair(0xbb, 0xee), // (+0, +0)
    TmdsPair::encode_pair(0xca, 0xf0), // (-2, +2)
    TmdsPair::encode_pair(0xdd, 0xee), // (+0, +0)
    TmdsPair::encode_pair(0xee, 0xee), // (+0, +0)
    TmdsPair::encode_pair(0xfd, 0xf1), // (-2, +3)
    TmdsPair::encode_pair(0x00, 0xfe), // (+0, -1)
    TmdsPair::encode_pair(0x14, 0xfc), // (+3, -3)
    TmdsPair::encode_pair(0x24, 0xfa), // (+2, -5)
    TmdsPair::encode_pair(0x30, 0xff), // (-3, +0)
    TmdsPair::encode_pair(0x48, 0xfa), // (+4, -5)
    TmdsPair::encode_pair(0x58, 0xfa), // (+3, -5)
    TmdsPair::encode_pair(0x67, 0xfc), // (+1, -3)
    TmdsPair::encode_pair(0x79, 0xfc), // (+2, -3)
    TmdsPair::encode_pair(0x8c, 0xfb), // (+4, -4)
    TmdsPair::encode_pair(0x9c, 0xfc), // (+3, -3)
    TmdsPair::encode_pair(0xaf, 0xfb), // (+5, -4)
    TmdsPair::encode_pair(0xbf, 0xfb), // (+4, -4)
    TmdsPair::encode_pair(0xcf, 0xfd), // (+3, -2)
    TmdsPair::encode_pair(0xe0, 0xfc), // (+3, -3)
    TmdsPair::encode_pair(0xf0, 0xfc), // (+2, -3)
    TmdsPair::encode_pair(0xff, 0xfe), // (+0, -1)
];
