use core::ops::Range;

use crate::{
    hal::gpio::PinId,
    render::{Palette1bpp, BW_PALETTE_1BPP},
};
use alloc::format;

use super::Counter;
use crate::{
    dvi::VERTICAL_REPEAT,
    render::{end_display_list, rgb, start_display_list, xrgb, FONT_HEIGHT},
};

// Sadly these can not be generic on GameOfLife struct due to limitations with const-generics
const BOARD_WIDTH: usize = 420;
const BOARD_HEIGHT: usize = 210;

const fn div_ceil(numerator: usize, denominator: usize) -> usize {
    (numerator + denominator - 1) / denominator
}

const BOARD_WIDTH_WORDS: usize = div_ceil(BOARD_WIDTH, 32);

pub struct GameOfLife {
    age: u32,
    universe: [u32; BOARD_WIDTH_WORDS * BOARD_HEIGHT], // TODO: pack?
}

impl GameOfLife {
    pub fn new(universe_seed: &str) -> Self {
        let center_x = BOARD_WIDTH / 2;
        let center_y = BOARD_HEIGHT / 2;

        let mut universe = [0; BOARD_WIDTH_WORDS * BOARD_HEIGHT];

        let mut current_x = center_x;
        let mut current_y = center_y;

        for line in universe_seed.lines() {
            if line.is_empty() {
                continue;
            }

            if let Some(position) = line.strip_prefix("#P").map(str::trim) {
                let (x, y): (i32, i32) = position
                    .split_once(' ')
                    .map(|(x, y)| (x.parse().unwrap(), y.parse().unwrap()))
                    .unwrap();

                current_x = (center_x as i32 + x) as usize;
                current_y = (center_y as i32 + y) as usize;

                continue;
            }

            let line = line.as_bytes();
            let current_x_word = current_x / 32;
            let current_x_bit = current_x % 32;
            let line_words = div_ceil(current_x_bit + line.len(), 32);

            let (unaligned_prefix, line) =
                line.split_at(usize::min(32 - current_x_bit, line.len()));
            let (aligned, unaligned_suffix) = line.split_at((line.len() / 32) * 32);

            fn chars_to_byte(chars: &[u8]) -> u32 {
                chars.iter().rev().fold(0, |word, byte| match byte {
                    b'.' => word << 1,
                    b'*' => (word << 1) | 0b1u32,
                    _ => unimplemented!(),
                })
            }

            let universe = &mut universe[current_x_word + current_y * BOARD_WIDTH_WORDS..];

            let starting_word = if !unaligned_prefix.is_empty() {
                let unaligned_prefix = chars_to_byte(unaligned_prefix) << current_x_bit;
                universe[0] |= unaligned_prefix; // FIXME: zero these bits out first

                1
            } else {
                0
            };

            let ending_word = if !unaligned_suffix.is_empty() {
                let unaligned_suffix = chars_to_byte(unaligned_suffix);
                universe[line_words - 1] |= unaligned_suffix; // FIXME: zero these bits out first

                line_words - 1
            } else {
                line_words
            };

            let mut aligned = aligned.chunks_exact(32).map(chars_to_byte);
            universe[starting_word..ending_word].fill_with(|| aligned.next().unwrap());

            current_y += 1;
        }

        let actual_size = core::mem::size_of::<Self>();
        let actual_size_words = div_ceil(actual_size, 4);

        let line_waste = BOARD_WIDTH % 32;
        let total_waste = line_waste * BOARD_HEIGHT;
        let total_waste_words = div_ceil(total_waste, 32);

        let ideal_size = div_ceil(BOARD_WIDTH * BOARD_HEIGHT, 8) + 4;
        let ideal_size_words = div_ceil(ideal_size, 4);

        defmt::info!(
            "size_of::<GameOfLife>() = {=usize} words\nline_waste = {=usize} bits\nideal_size = {=usize} words\ntotal_waste = {=usize} words",
            actual_size_words,
            line_waste,
            ideal_size_words,
            total_waste_words
        );

        GameOfLife { age: 0, universe }
    }

    pub fn tick(&mut self) {
        self.age += 1;

        const EMPTY_LINE: [u32; BOARD_WIDTH_WORDS] = [0; BOARD_WIDTH_WORDS];

        let mut previous_new_line = EMPTY_LINE;
        let mut new_line = EMPTY_LINE;

        let mut current_range = None;
        let mut next_range = Some(0..BOARD_WIDTH_WORDS);

        for _ in 0..BOARD_HEIGHT {
            let previous_range = current_range;
            current_range = next_range.clone();
            next_range = next_range.map(|Range { start, end }| Range {
                start: start + BOARD_WIDTH_WORDS,
                end: end + BOARD_WIDTH_WORDS,
            });

            let previous_line = previous_range
                .clone()
                .and_then(|range| self.universe.get(range))
                .unwrap_or(&EMPTY_LINE);
            let current_line = current_range
                .clone()
                .and_then(|range| self.universe.get(range))
                .unwrap_or(&EMPTY_LINE);
            let next_line = next_range
                .clone()
                .and_then(|range| self.universe.get(range))
                .unwrap_or(&EMPTY_LINE);

            fn straddle_mask_top(previous: u32, current: u32) -> u32 {
                previous & (0b1 << 31) | current & 0b11
            }
            fn mask(word: u32, bit: usize) -> u32 {
                word & (0b111 << (bit - 1))
            }
            fn straddle_mask_bottom(current: u32, next: u32) -> u32 {
                current & (0b11 << 30) | next & 0b1
            }

            /// Rather than calling u32::count_ones() (`popcnt`) 3 times, we can put all 3 of the
            /// 3 bit integers into one word, and call count_ones on that
            ///
            /// This does result in significant assembly savings (https://godbolt.org/z/YKbEoab9d)
            /// but it might not be the most optimal
            fn neighbors(previous: u32, current: u32, next: u32) -> u32 {
                (previous | current.rotate_left(4) | next.rotate_right(4)).count_ones()
            }

            for word in 0..BOARD_WIDTH_WORDS {
                let previous_word = word.checked_sub(1);
                let next_word = if word < (BOARD_WIDTH_WORDS - 1) {
                    Some(word + 1)
                } else {
                    None
                };

                let mut new_word = 0;

                new_word |= match neighbors(
                    straddle_mask_top(
                        previous_word
                            .map(|word| previous_line[word])
                            .unwrap_or_default(),
                        previous_line[word],
                    ),
                    straddle_mask_top(
                        previous_word
                            .map(|word| current_line[word])
                            .unwrap_or_default(),
                        current_line[word],
                    ),
                    straddle_mask_top(
                        previous_word
                            .map(|word| next_line[word])
                            .unwrap_or_default(),
                        next_line[word],
                    ),
                ) {
                    3 => 0b1,
                    4 => current_line[word] & 0b1,
                    _ => 0b0,
                };

                {
                    let previous_word = previous_line[word];
                    let current_word = current_line[word];
                    let next_word = next_line[word];
                    for bit in 1..31 {
                        new_word |= match neighbors(
                            mask(previous_word, bit),
                            mask(current_word, bit),
                            mask(next_word, bit),
                        ) {
                            3 => 0b1 << bit,
                            4 => current_word & (0b1 << bit),
                            _ => 0b0,
                        };
                    }
                }
                new_word |= match neighbors(
                    straddle_mask_bottom(
                        previous_line[word],
                        next_word
                            .map(|word| previous_line[word])
                            .unwrap_or_default(),
                    ),
                    straddle_mask_bottom(
                        current_line[word],
                        next_word.map(|word| current_line[word]).unwrap_or_default(),
                    ),
                    straddle_mask_bottom(
                        next_line[word],
                        next_word.map(|word| next_line[word]).unwrap_or_default(),
                    ),
                ) {
                    3 => 0b1 << 31,
                    4 => current_line[word] & (0b1 << 31),
                    _ => 0b0,
                };

                new_line[word] = new_word;
            }

            if let Some(range) = previous_range {
                self.universe[range].copy_from_slice(&previous_new_line);
            }
            previous_new_line = core::mem::take(&mut new_line);
        }

        // Apply the last new line
        if let Some(range) = current_range {
            self.universe[range].copy_from_slice(&previous_new_line);
        }
    }
}

const TEXT: u32 = 0xffffff;
const BACKGROUND: u32 = 0x800080;
const ALIVE: u32 = 0x00ffff;
const DEAD: u32 = 0x303030;

#[link_section = ".scratch_x"]
pub static CONWAY_TEXT_PALETTE: Palette1bpp = Palette1bpp::new_rgb(BACKGROUND, TEXT);

#[link_section = ".scratch_x"]
pub static CONWAY_PALETTE: Palette1bpp = Palette1bpp::new_rgb(DEAD, ALIVE);

impl GameOfLife {
    pub(super) fn render<P: PinId>(&self, counter: &Counter<P>) {
        let height = 480 / VERTICAL_REPEAT as u32;
        let width = 640;
        let background = xrgb(BACKGROUND);
        let (mut rb, mut sb) = start_display_list();

        let horizontal_padding = width - BOARD_WIDTH as u32;
        let padding_left = horizontal_padding / 2;
        let padding_right = padding_left + (horizontal_padding & 0b1); // Deal with odd padding

        let vertical_padding = height - BOARD_HEIGHT as u32;
        let padding_top = vertical_padding / 2;
        let padding_bottom = padding_top + (vertical_padding & 0b1); // Deal with odd padding

        rb.begin_stripe(padding_top);
        rb.end_stripe();
        sb.begin_stripe(padding_top);
        sb.solid(width, background);
        sb.end_stripe();

        rb.begin_stripe(BOARD_HEIGHT as u32);
        rb.blit_1bpp(
            &self.universe,
            BOARD_WIDTH_WORDS,
            BOARD_WIDTH_WORDS as u32 * 4,
        );
        rb.end_stripe();
        sb.begin_stripe(BOARD_HEIGHT as u32);
        sb.solid(padding_left, background);
        sb.pal_1bpp(BOARD_WIDTH as u32, &CONWAY_PALETTE);
        sb.solid(padding_right, background);
        sb.end_stripe();

        rb.begin_stripe(padding_bottom - FONT_HEIGHT * 2);
        rb.end_stripe();
        sb.begin_stripe(padding_bottom - FONT_HEIGHT * 2);
        sb.solid(width, background);
        sb.end_stripe();

        {
            rb.begin_stripe(FONT_HEIGHT);
            let text = format!("Conway's Game of life, age: {}", self.age);
            let text_width = rb.text(&text);
            let text_width = text_width + text_width % 2;
            rb.end_stripe();
            sb.begin_stripe(FONT_HEIGHT);
            sb.pal_1bpp(text_width, &CONWAY_TEXT_PALETTE);
            sb.solid(width - text_width, background);
            sb.end_stripe();
            rb.begin_stripe(FONT_HEIGHT);
            let text = format!("Hello pico-dvi-rs, frame {}", counter.count);
            let text_width = rb.text(&text);
            let text_width = text_width + text_width % 2;
            rb.end_stripe();
            sb.begin_stripe(FONT_HEIGHT);
            sb.pal_1bpp(text_width, &BW_PALETTE_1BPP);
            sb.solid(width - text_width, rgb(0x00, 0x00, 0x00));
            sb.end_stripe();
        }
        end_display_list(rb, sb);
    }
}
