use alloc::format;
use rp_pico::hal::gpio::PinId;

use super::Counter;
use crate::{
    dvi::VERTICAL_REPEAT,
    render::{
        end_display_list, quantized_1bpp_palette, rgb, start_display_list, xrgb, PaletteEntry,
        BW_PALETTE, FONT_HEIGHT,
    },
};

pub struct GameOfLife {
    age: u32,
}

const TEXT_FOREGROUND: u32 = 0xffffff;
const FOREGROUND: u32 = 0xffff00;
const BACKGROUND: u32 = 0x800080;

#[link_section = ".scratch_x"]
pub static CONWAY_TEXT_PALETTE: [PaletteEntry; 4] =
    quantized_1bpp_palette(BACKGROUND, TEXT_FOREGROUND);

#[link_section = ".scratch_x"]
pub static CONWAY_PALETTE: [PaletteEntry; 4] = quantized_1bpp_palette(BACKGROUND, FOREGROUND);

impl GameOfLife {
    pub fn new() -> Self {
        GameOfLife { age: 0 }
    }

    pub fn tick(&mut self) {
        self.age += 1;
    }

    pub(super) fn render<P: PinId>(&self, counter: &Counter<P>) {
        let height = 480 / VERTICAL_REPEAT as u32;
        let width = 640;
        let (mut rb, mut sb) = start_display_list();

        rb.begin_stripe(height - FONT_HEIGHT * 2);
        rb.end_stripe();
        sb.begin_stripe(height - FONT_HEIGHT * 2);
        sb.solid(width, xrgb(BACKGROUND));
        sb.end_stripe();

        {
            rb.begin_stripe(FONT_HEIGHT);
            let text = format!("Conway's Game of life, age: {}", self.age);
            let text_width = rb.text(&text);
            let text_width = text_width + text_width % 2;
            rb.end_stripe();
            sb.begin_stripe(FONT_HEIGHT);
            sb.pal_1bpp(text_width, &CONWAY_PALETTE);
            sb.solid(width - text_width, xrgb(BACKGROUND));
            sb.end_stripe();
            rb.begin_stripe(FONT_HEIGHT);
            let text = format!("Hello pico-dvi-rs, frame {}", counter.count);
            let text_width = rb.text(&text);
            let text_width = text_width + text_width % 2;
            rb.end_stripe();
            sb.begin_stripe(FONT_HEIGHT);
            sb.pal_1bpp(text_width, &BW_PALETTE);
            sb.solid(width - text_width, rgb(0x00, 0x00, 0x00));
            sb.end_stripe();
        }
        end_display_list(rb, sb);
    }
}
