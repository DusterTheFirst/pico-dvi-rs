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

impl Renderlist {
    pub fn get(&self) -> &[u32] {
        &self.0
    }
}

pub struct RenderlistBuilder {
    renderlist: Vec<u32>,
    stripe_start: usize,
}

impl RenderlistBuilder {
    pub fn new() -> Self {
        RenderlistBuilder {
            renderlist: alloc::vec![],
            stripe_start: 0,
        }
    }

    /// Create a [`RenderlistBuilder`] reusing the existing allocation from a [`Renderlist`]
    pub fn recycle(mut renderlist: Renderlist) -> Self {
        renderlist.0.clear();
        RenderlistBuilder {
            renderlist: renderlist.0,
            stripe_start: 0,
        }
    }

    pub fn begin_stripe(&mut self, height: u32) {
        self.renderlist.extend([height, 0]);
    }

    pub fn end_stripe(&mut self) {
        self.renderlist.push(render_stop as u32);
        let len = self.renderlist.len();
        self.renderlist[self.stripe_start + 1] = len as u32;
        self.stripe_start = len;
    }

    /// Returns width.
    ///
    /// Should take a font object, but that's hardcoded for now.
    pub fn text(&mut self, text: &str) -> u32 {
        let mut x_bit: u32 = 0;
        for char in text.as_bytes() {
            let glyph = char - b' ';

            let width = FONT_X_WIDTHS[glyph as usize] as u32;
            // TODO: be aware of max width, clamp
            let offset = FONT_X_OFFSETS[glyph as usize] as u32;

            let next_bit = x_bit % 32 + width;
            let op = match next_bit {
                33.. => render_blit_straddle,
                32 => render_blit_out,
                ..=31 => render_blit_simple,
            };

            let mut shifts = offset & 31;
            if next_bit > 32 {
                shifts |= (x_bit % 32) << 8;
                shifts |= (32 - (offset % 32 + width)) << 16;
                shifts |= (64 - next_bit) << 24;
            } else {
                shifts |= (32 - width) << 8;
                shifts |= (32 - next_bit) << 16;
            }

            let font_ptr = unsafe { FONT_BITS.as_ptr().add(offset as usize / 32) };
            self.renderlist
                .extend([op as u32, font_ptr as u32, FONT_STRIDE, shifts]);
            x_bit += width;
        }

        if !text.is_empty() {
            // Update the last instruction to output version
            let op_ix = self.renderlist.len() - 4;
            if self.renderlist[op_ix] == render_blit_simple as u32 {
                self.renderlist[op_ix] = render_blit_out as u32;
            } else if self.renderlist[op_ix] == render_blit_straddle as u32 {
                self.renderlist[op_ix] = render_blit_straddle_out as u32;
            }
        }

        x_bit
    }

    pub fn build(self) -> Renderlist {
        Renderlist(self.renderlist)
    }
}
