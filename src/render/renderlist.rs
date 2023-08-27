use alloc::vec::Vec;

use super::font::{FONT_BITS, FONT_STRIDE, FONT_X_OFFSETS, FONT_X_WIDTHS};

extern "C" {
    fn render_stop();

    fn render_blit_simple();

    fn render_blit_out();

    fn render_blit_straddle();

    fn render_blit_straddle_out();
}

pub struct Renderlist(Vec<u32>);

pub struct RenderlistBuilder {
    v: Vec<u32>,
    stripe_start: usize,
}

impl RenderlistBuilder {
    pub fn new() -> Self {
        RenderlistBuilder {
            v: alloc::vec![],
            stripe_start: 0,
        }
    }

    pub fn recycle(mut renderlist: Renderlist) -> Self {
        renderlist.0.clear();
        RenderlistBuilder {
            v: renderlist.0,
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
            let op = if next > 32 {
                render_blit_straddle
            } else if next == 32 {
                render_blit_out
            } else {
                render_blit_simple
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

    pub fn build(self) -> Renderlist {
        Renderlist(self.v)
    }
}

impl Renderlist {
    pub fn get(&self) -> &[u32] {
        &self.0
    }
}
