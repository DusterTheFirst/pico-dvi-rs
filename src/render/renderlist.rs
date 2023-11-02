use core::cmp;

use alloc::vec::Vec;

use super::font::{FONT_BITS, FONT_STRIDE, FONT_X_OFFSETS, FONT_X_WIDTHS};

extern "C" {
    fn render_stop();

    fn render_blit_simple();

    fn render_blit_out();

    fn render_blit_straddle();

    fn render_blit_straddle_out();

    fn render_blit_64_aligned();

    fn render_blit_64_straddle();
}

pub struct Renderlist(Vec<u32>, u32);

pub struct RenderlistBuilder {
    v: Vec<u32>,
    width: u32,
    x: u32,
    stripe_start: usize,
}

impl RenderlistBuilder {
    pub fn new(width: u32) -> Self {
        RenderlistBuilder {
            v: alloc::vec![],
            width,
            x: 0,
            stripe_start: 0,
        }
    }

    pub fn recycle(mut renderlist: Renderlist) -> Self {
        renderlist.0.clear();
        RenderlistBuilder {
            v: renderlist.0,
            width: renderlist.1,
            x: 0,
            stripe_start: 0,
        }
    }

    pub fn begin_stripe(&mut self, height: u32) {
        self.v.extend([height, 0]);
    }

    pub fn end_stripe(&mut self) {
        self.v.push(render_stop as u32);
        let len = self.v.len();
        self.v[self.stripe_start + 1] = len as u32;
        self.stripe_start = len;
        self.x = 0;
    }

    fn tile_slice(&mut self, tile: &[u32], stride: u32, start: u32, end: u32) {
        let next = self.x % 8 + 8 - start;
        let op = match next.cmp(&8) {
            cmp::Ordering::Greater => render_blit_straddle,
            cmp::Ordering::Equal => render_blit_out,
            cmp::Ordering::Less => render_blit_simple,
        };
        let tile_ptr = tile.as_ptr() as u32;
        let mut shifts = start * 4;
        if next > 8 {
            shifts |= (self.x % 8) << 10;
            shifts |= (8 - end) << 18;
            shifts |= (16 - next) << 26;
        } else {
            shifts |= (8 - (end - start)) << 10;
            shifts |= (8 - next) << 18;
        }
        self.v.extend([op as u32, tile_ptr, stride, shifts]);
        self.x += end - start;
    }

    // Note: this is currently set up for 4bpp, but could be adapted
    // for other bit widths.
    pub fn tile64(&mut self, tile: &[u32], start: u32, mut end: u32) {
        if self.x + (end - start) >= self.width {
            end = self.width - self.x + start;
        }
        let stride = 8; // hardcoded for tiles, but maybe should be an argument
        if start == 0 && end == 16 {
            let offset = self.x % 8;
            if offset == 0 {
                self.v
                    .extend([render_blit_64_aligned as u32, tile.as_ptr() as u32, stride]);
            } else {
                let off4 = offset * 4;
                let shift = off4 + ((32 - off4) << 8);
                self.v.extend([
                    render_blit_64_straddle as u32,
                    tile.as_ptr() as u32,
                    stride,
                    shift,
                ]);
            }
            self.x += start + end;
        } else {
            if start < 8 {
                self.tile_slice(tile, stride, start, end.min(8));
            }
            if end > 8 {
                self.tile_slice(&tile[1..], stride, start.max(8) - 8, end - 8);
            }
        }
    }

    /// Returns width.
    ///
    /// Should take a font object, but that's hardcoded for now.
    pub fn text(&mut self, text: &str) -> u32 {
        let mut x = 0;
        for c in text.as_bytes() {
            let glyph = c - b' ';
            let width = FONT_X_WIDTHS[glyph as usize] as u32;
            // TODO: be aware of max width, clamp
            let offset = FONT_X_OFFSETS[glyph as usize] as u32;
            let next = x % 32 + width;
            let op = match next.cmp(&32) {
                cmp::Ordering::Greater => render_blit_straddle,
                cmp::Ordering::Equal => render_blit_out,
                cmp::Ordering::Less => render_blit_simple,
            };
            let mut shifts = offset & 31;
            if next > 32 {
                shifts |= (x % 32) << 8;
                shifts |= (32 - (offset % 32 + width)) << 16;
                shifts |= (64 - next) << 24;
            } else {
                shifts |= (32 - width) << 8;
                shifts |= (32 - next) << 16;
            }
            let font_ptr = unsafe { FONT_BITS.as_ptr().add(offset as usize / 32) };
            self.v
                .extend([op as u32, font_ptr as u32, FONT_STRIDE, shifts]);
            x += width;
        }
        if !text.is_empty() {
            let op_ix = self.v.len() - 4;
            if self.v[op_ix] == render_blit_simple as u32 {
                self.v[op_ix] = render_blit_out as u32;
            } else if self.v[op_ix] == render_blit_straddle as u32 {
                self.v[op_ix] = render_blit_straddle_out as u32;
            }
        }
        x
    }

    pub fn blit(&mut self, array: &[u32], stride: u32) {
        self.v
            .extend_from_slice(&[render_blit_out as u32, array.as_ptr() as u32, stride, 0]);
    }

    pub fn build(self) -> Renderlist {
        Renderlist(self.v, self.width)
    }
}

impl Renderlist {
    pub fn get(&self) -> &[u32] {
        &self.0
    }
}
