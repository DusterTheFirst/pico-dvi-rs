use alloc::vec::Vec;

use crate::render::{Palette1bpp, Palette4bppFast};

extern "C" {
    fn video_scan_solid_16();

    fn video_scan_1bpp_pal_16();

    fn video_scan_4bpp_pal_16();

    fn video_scan_stop();
}

/// A display list for video scanout.
///
/// A scanlist contains a description of how to render the scene into
/// TMDS encoded scan lines. The input to this stage is intended to be
/// line buffers, but at present only solid color blocks are implemented.
///
/// There are a number of safety requirements, as the scanlist is
/// interpreted by an unsafe virtual machine. The width of each scanline
/// must match the actual buffer provided, and the total height must
/// also be the number of scanlines.
///
/// One potential direction is to make the scanlist builder enforce the
/// safety requirements. This would have a modest runtime cost (none
/// during scanout).
pub struct Scanlist(Vec<u32>);

/// A builder for scanlists.
///
/// The application builds a scanlist, then hands it to the display
/// system for scanout. Typically it is double-buffered, so one is being
/// scanned out, the other is built by the app.
pub struct ScanlistBuilder {
    v: Vec<u32>,
    x: u32,
}

impl ScanlistBuilder {
    pub fn new(_width: u32, _height: u32) -> Self {
        ScanlistBuilder {
            v: alloc::vec![],
            x: 0,
        }
    }

    pub fn recycle(mut scanlist: Scanlist) -> Self {
        scanlist.0.clear();
        ScanlistBuilder {
            v: scanlist.0,
            x: 0,
        }
    }

    pub fn build(self) -> Scanlist {
        // TODO: check width, do some kind of error?
        Scanlist(self.v)
    }

    pub fn begin_stripe(&mut self, height: u32) {
        self.v.push(height);
    }

    pub fn end_stripe(&mut self) {
        self.v.push(video_scan_stop as u32);
    }

    /// Generate a run of solid color.
    pub fn solid(&mut self, count: u32, color: u32) {
        self.v
            .extend_from_slice(&[video_scan_solid_16 as u32, count, color]);
        self.x += count;
    }

    /// Safety note: we take a reference to the palette, but the
    /// lifetime must extend until it is used.
    // TODO: this really should be marked as unsafe, or we should make a safe
    // variant that stores the palette in the display list. Either way, it should
    // be consistent with the 4bpp method.
    pub fn pal_1bpp(&mut self, count: u32, palette: &Palette1bpp) {
        self.v.extend_from_slice(&[
            video_scan_1bpp_pal_16 as u32,
            count,
            palette as *const _ as u32,
        ]);
        self.x += count;
    }

    /// Safety note: we take a reference to the palette, but the
    /// lifetime must extend until it is used.
    pub unsafe fn pal_4bpp(&mut self, count: u32, palette: &Palette4bppFast) {
        self.v.extend_from_slice(&[
            video_scan_4bpp_pal_16 as u32,
            count,
            palette as *const _ as u32,
        ]);
        self.x += count;
    }
}

impl Scanlist {
    pub fn get(&self) -> &[u32] {
        &self.0
    }
}
