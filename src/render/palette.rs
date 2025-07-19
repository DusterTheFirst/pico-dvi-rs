use super::{rgb, xrgb};

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Palette1bpp([u32; 4]);

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Palette4bppFast([u32; 256]);

impl Palette1bpp {
    // Arguments are 16bpp; use `rgb`
    pub const fn new(bg: u32, fg: u32) -> Self {
        Self([
            bg | (bg << 16),
            fg | (bg << 16),
            bg | (fg << 16),
            fg | (fg << 16),
        ])
    }

    // Arguments are 0xRRGGBB
    pub const fn new_rgb(bg: u32, fg: u32) -> Self {
        Self::new(xrgb(bg), xrgb(fg))
    }
}

#[link_section = ".scratch_x"]
pub static BW_PALETTE_1BPP: Palette1bpp = Palette1bpp::new(rgb(0, 0, 0), rgb(255, 255, 255));

impl Palette4bppFast {
    pub const fn new(colors: &[u32; 16]) -> Self {
        let mut a = [0; 256];
        let mut i = 0;
        while i < 256 {
            a[i] = xrgb(colors[i % 16]) | (xrgb(colors[i / 16]) << 16);
            i += 1;
        }
        Self(a)
    }
}
