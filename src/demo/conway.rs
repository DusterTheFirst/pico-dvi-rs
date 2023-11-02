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
                0b00000000000000000000000000000000u32.reverse_bits(),
                0b01100011000011000110000100000000u32.reverse_bits(),
                0b01100100100100100101001010000000u32.reverse_bits(),
                0b00000011000010100010000100000000u32.reverse_bits(),
                0b00000000000001000000000000000000u32.reverse_bits(),
                0b00000000000000000000000000000000u32.reverse_bits(),
                0b00000000000000000000000000000000u32.reverse_bits(),
                0b00100000000011000000000000000000u32.reverse_bits(),
                0b00100001110011000000000000000000u32.reverse_bits(),
                0b00100011100000110000000000000000u32.reverse_bits(),
                0b00000000000000110000000000000000u32.reverse_bits(),
                0b00000000000000000000000000000000u32.reverse_bits(),
                0b00000000000000000000000000000000u32.reverse_bits(),
                0b00000000000000000000000000000000u32.reverse_bits(),
                0b00000000000000000000000000000000u32.reverse_bits(),
                0b00000000000000000000000000000000u32.reverse_bits(),
                0b00000000000000000000000000000000u32.reverse_bits(),
                0b00000000000000000000000000000000u32.reverse_bits(),
                0b00000000000000000000000000000000u32.reverse_bits(),
                0b00000000000000000000000000000000u32.reverse_bits(),
                0b00000000000000000000000000000000u32.reverse_bits(),
                0b00000000000000000000000000000000u32.reverse_bits(),
                0b01100011000011000110000100000000u32.reverse_bits(),
                0b01100100100100100101001010000000u32.reverse_bits(),
                0b00000011000010100010000100000000u32.reverse_bits(),
                0b00000000000001000000000000000000u32.reverse_bits(),
                0b00000000000000000000000000000000u32.reverse_bits(),
                0b00000000000000000000000000000000u32.reverse_bits(),
                0b00000000000000000000000000000000u32.reverse_bits(),
                0b00000000000000000000000000000000u32.reverse_bits(),
                0b00000000000000000000000000000000u32.reverse_bits(),
                0b00000000000000000000000000000000u32.reverse_bits(),
            ],
        }
    }

    pub fn tick(&mut self) {
        self.age += 1;

        // let last = *self.universe.last().unwrap();
        // for i in (1..self.universe.len()).rev() {
        //     self.universe[i] = self.universe[i - 1];
        // }
        // self.universe[0] = last;

        // At each step in time, the following transitions occur:

        // Any live cell with fewer than two live neighbours dies, as if by underpopulation.
        // Any live cell with two or three live neighbours lives on to the next generation.
        // Any live cell with more than three live neighbours dies, as if by overpopulation.
        // Any dead cell with exactly three live neighbours becomes a live cell, as if by reproduction.

        // These rules, which compare the behaviour of the automaton to real life, can be condensed into the following:

        // Any live cell with two or three live neighbours survives.
        // Any dead cell with three live neighbours becomes a live cell.
        // All other live cells die in the next generation. Similarly, all other dead cells stay dead.

        let mut previous_new_line = 0;
        let mut new_line = 0;

        for row in 0..self.universe.len() {
            let previous_line = row.checked_sub(1).map(|i| self.universe[i]).unwrap_or(0);
            let current_line = self.universe[row];
            let next_line = self.universe.get(row + 1).copied().unwrap_or(0);

            let masks = core::iter::once(0xc0000000)
                .chain((0..(core::mem::size_of::<u32>() * 8 - 1)).map(|i| 0xe0000000 >> i))
                .enumerate();

            for (column, mask) in masks {
                let previous_masked = previous_line & mask;
                let current_masked = current_line & mask;
                let next_masked = next_line & mask;
                let neighborhood = previous_masked.count_ones()
                    + current_masked.count_ones()
                    + next_masked.count_ones();

                let previous_state = (current_line >> (31 - column)) & 0x1;

                let new_state = match neighborhood {
                    3 => 1,
                    4 => previous_state,
                    _ => 0,
                };

                // if new_state != previous_state {
                //     defmt::debug!(
                //         "{=usize}/{=usize}: {=u32} => {=u32} @ {=u32}\n\n{=u32:032b}\n\n{=u32:032b}\t{=u32:032b}\n{=u32:032b}\t{=u32:032b}\n{=u32:032b}\t{=u32:032b}",
                //         row,
                //         column,

                //         previous_state,
                //         new_state,

                //         neighborhood,
                //         mask,

                //         previous_line, previous_masked,
                //         current_line, current_masked,
                //         next_masked, next_masked
                //     );
                // }

                new_line = (new_line << 1) + new_state;
            }

            if let Some(i) = row.checked_sub(1) {
                self.universe[i] = previous_new_line;
            }
            previous_new_line = core::mem::take(&mut new_line);
        }
        // panic!()
    }
}

const TEXT: u32 = 0xffffff;
const BACKGROUND: u32 = 0x800080;
const ALIVE: u32 = 0x00ffff;
const DEAD: u32 = 0x303030;

#[link_section = ".scratch_x"]
pub static CONWAY_TEXT_PALETTE: [PaletteEntry; 4] = quantized_1bpp_palette(BACKGROUND, TEXT);

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
