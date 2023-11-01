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
    universe: [u32; BOARD_WIDTH / 32 * BOARD_HEIGHT],
}

const BOARD_WIDTH: usize = 32;
const BOARD_HEIGHT: usize = 32;

impl GameOfLife {
    // TODO: Seed input?
    pub fn new() -> Self {
        GameOfLife {
            age: 0,
            universe: [
                0b00000000000000000000000000000000,
                0b01100011000011000110000100000000,
                0b01100100100100100101001010000000,
                0b00000011000010100010000100000000,
                0b00000000000001000000000000000000,
                0b00000000000000000000000000000000,
                0b00000000000000000000000000000000,
                0b00100000000011000000000000000000,
                0b00100001110011000000000000000000,
                0b00100011100000110000000000000000,
                0b00000000000000110000000000000000,
                0b00000000000000000000000000000000,
                0b00000000000000000000000000000000,
                0b00000000000000000000000000000000,
                0b00000000000000000000000000000000,
                0b00000000000000000000000000000000,
                0b00000000000000000000000000000000,
                0b00000000000000000000000000000000,
                0b00000000000000000000000000000000,
                0b00000000000000000000000000000000,
                0b00000000000000000000000000000000,
                0b00000000000000000000000000000000,
                0b00000000000000000000000000000000,
                0b00000000000000000000000000000000,
                0b00000000000000000000000000000000,
                0b00000000000000000000000000000000,
                0b00000000000000000000000000000000,
                0b00000000000000000000000000000000,
                0b01100011000011000110000100000000,
                0b01100100100100100101001010000000,
                0b00000011000010100010000100000000,
                0b00000000000001000000000000000000,
            ],
        }
    }

    pub fn tick(&mut self) {
        self.age += 1;

        // At each step in time, the following transitions occur:

        // Any live cell with fewer than two live neighbours dies, as if by underpopulation.
        // Any live cell with two or three live neighbours lives on to the next generation.
        // Any live cell with more than three live neighbours dies, as if by overpopulation.
        // Any dead cell with exactly three live neighbours becomes a live cell, as if by reproduction.

        // These rules, which compare the behaviour of the automaton to real life, can be condensed into the following:

        // Any live cell with two or three live neighbours survives.
        // Any dead cell with three live neighbours becomes a live cell.
        // All other live cells die in the next generation. Similarly, all other dead cells stay dead.
    }
}

const TEXT: u32 = 0xffffff;
const BACKGROUND: u32 = 0x800080;
const ALIVE: u32 = 0x00ffff;
const DEAD: u32 = 0x303030;

#[link_section = ".scratch_x"]
pub static CONWAY_TEXT_PALETTE: [PaletteEntry; 4] =
    quantized_1bpp_palette(BACKGROUND, TEXT);

#[link_section = ".scratch_x"]
pub static CONWAY_PALETTE: [PaletteEntry; 4] = quantized_1bpp_palette(DEAD, ALIVE);

impl GameOfLife {
    pub(super) fn render<P: PinId>(&self, counter: &Counter<P>) {
        let height = 480 / VERTICAL_REPEAT as u32;
        let width = 640;
        let (mut rb, mut sb) = start_display_list();

        // --- 224 ---
        // 304  32x32 304
        // --- 194 ---
        // 15
        // 15

        rb.begin_stripe(224);
        rb.end_stripe();
        sb.begin_stripe(224);
        sb.solid(width, xrgb(BACKGROUND));
        sb.end_stripe();

        rb.begin_stripe(32);
        rb.blit(&self.universe);
        rb.end_stripe();
        sb.begin_stripe(32);
        sb.solid(304, xrgb(BACKGROUND));
        sb.pal_1bpp(32, &CONWAY_PALETTE); // TODO: display game of life board here
        sb.solid(304, xrgb(BACKGROUND));
        sb.end_stripe();

        rb.begin_stripe(194);
        rb.end_stripe();
        sb.begin_stripe(194);
        sb.solid(width, xrgb(BACKGROUND));
        sb.end_stripe();

        {
            rb.begin_stripe(FONT_HEIGHT);
            let text = format!("Conway's Game of life, age: {}", self.age);
            let text_width = rb.text(&text);
            let text_width = text_width + text_width % 2;
            rb.end_stripe();
            sb.begin_stripe(FONT_HEIGHT);
            sb.pal_1bpp(text_width, &CONWAY_TEXT_PALETTE);
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
