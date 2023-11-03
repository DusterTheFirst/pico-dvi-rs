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
    pub fn new(universe: &str) -> Self {
        let mut rows = universe.lines().flat_map(|line| {
            let bytes = line.as_bytes();

            bytes
                .chunks(32)
                .map(|word| {
                    word.iter().fold(0, |word, byte| match byte {
                        b'.' => word >> 1,
                        b'*' => (word >> 1) | 0x80000000,
                        _ => unimplemented!(),
                    })
                })
                .chain(core::iter::repeat(0).take(BOARD_WIDTH_WORDS - div_ceil(bytes.len(), 32)))
        });

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

        GameOfLife {
            age: 0,
            universe: core::array::from_fn(|_| rows.next().unwrap_or(0xaaaaaaaa)),
        }
    }

    pub fn tick(&mut self) {
        self.age += 1;

        let mut previous_new_line = [0; BOARD_WIDTH_WORDS];
        let mut new_line = [0; BOARD_WIDTH_WORDS];

        for row in 0..BOARD_HEIGHT {
            // FIXME: very jank indexing
            let previous_line = if row != 0 {
                self.universe[(row - 1) * BOARD_WIDTH_WORDS..][..BOARD_WIDTH_WORDS]
                    .try_into()
                    .unwrap()
            } else {
                &[0; BOARD_WIDTH_WORDS]
            };
            let current_line = &self.universe[row * BOARD_WIDTH_WORDS..][..BOARD_WIDTH_WORDS]
                .try_into()
                .unwrap();
            let next_line = if row != BOARD_HEIGHT - 1 {
                self.universe[(row + 1) * BOARD_WIDTH_WORDS..][..BOARD_WIDTH_WORDS]
                    .try_into()
                    .unwrap()
            } else {
                &[0; BOARD_WIDTH_WORDS]
            };

            fn new_state(
                (previous_line, current_line, next_line): (
                    &[u32; BOARD_WIDTH_WORDS],
                    &[u32; BOARD_WIDTH_WORDS],
                    &[u32; BOARD_WIDTH_WORDS],
                ),
                mask: impl Fn(u32, u32, u32) -> u32,
                word: usize,
                byte: usize,
                new_line: &mut [u32; BOARD_WIDTH_WORDS],
            ) {
                let previous_masked = mask(
                    word.checked_sub(1).map(|i| previous_line[i]).unwrap_or(0), // Previous word (or 0 if none previous)
                    previous_line[word],
                    previous_line.get(word + 1).copied().unwrap_or(0),
                );
                let current_masked = mask(
                    word.checked_sub(1).map(|i| current_line[i]).unwrap_or(0), // Previous word (or 0 if none previous)
                    current_line[word],
                    current_line.get(word + 1).copied().unwrap_or(0),
                );
                let next_masked = mask(
                    word.checked_sub(1).map(|i| next_line[i]).unwrap_or(0), // Previous word (or 0 if none previous)
                    next_line[word],
                    next_line.get(word + 1).copied().unwrap_or(0),
                );

                let neighborhood = previous_masked.count_ones()
                    + current_masked.count_ones()
                    + next_masked.count_ones();

                let previous_state = (current_line[word] >> byte) & 0x1;

                let new_state = match neighborhood {
                    3 => 1,
                    4 => previous_state,
                    _ => 0,
                };

                // if new_state != previous_state {
                //     defmt::debug!(
                //         "{=usize}+{=usize}: {=u32} => {=u32} N{=u32}\n\n[10987654321098765432109876543210]\n{=[?; 2]:032b}\t{=u32:032b}\n{=[?; 2]:032b}\t{=u32:032b}\n{=[?; 2]:032b}\t{=u32:032b}",
                //         word,
                //         byte,

                //         previous_state,
                //         new_state,

                //         neighborhood,

                //         previous_line, previous_masked,
                //         current_line, current_masked,
                //         next_line, next_masked
                //     );
                // }

                new_line[word] = (new_line[word] << 1) + new_state;
            }

            // pp ... ppp ppn pnn nnn ... nn
            for word in 0..BOARD_WIDTH_WORDS {
                new_state(
                    (previous_line, current_line, next_line),
                    |pre, cur, _| pre & 0b001 | cur & (0b110 << 30),
                    word,
                    31,
                    &mut new_line,
                );
                for i in (1..=30).rev() {
                    new_state(
                        (previous_line, current_line, next_line),
                        |_, cur, _| cur & (0b111 << (i - 1)),
                        word,
                        i,
                        &mut new_line,
                    );
                }
                new_state(
                    (previous_line, current_line, next_line),
                    |_, cur, next| cur & 0b011 | next & (0b100 << 31),
                    word,
                    0,
                    &mut new_line,
                );
            }

            if let Some(i) = row.checked_sub(1) {
                self.universe[i * BOARD_WIDTH_WORDS..][..BOARD_WIDTH_WORDS]
                    .copy_from_slice(&previous_new_line);
            }
            previous_new_line = core::mem::take(&mut new_line);
        }

        // Apply the last new line
        self.universe[(BOARD_HEIGHT - 1) * BOARD_WIDTH_WORDS..][..BOARD_WIDTH_WORDS]
            .copy_from_slice(&previous_new_line);
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
            sb.pal_1bpp(text_width, &BW_PALETTE);
            sb.solid(width - text_width, rgb(0x00, 0x00, 0x00));
            sb.end_stripe();
        }
        end_display_list(rb, sb);
    }
}
