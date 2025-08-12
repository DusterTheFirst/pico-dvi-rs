use core::sync::atomic::{AtomicU32, AtomicU8, Ordering::Relaxed};

use alloc::string::String;

use crate::render::{end_display_list, rgb, start_display_list, BW_PALETTE_1BPP, FONT_HEIGHT};

const BUFFER_SIZE: usize = 4096;
static BUFFER: [AtomicU8; BUFFER_SIZE] = [const { AtomicU8::new(0) }; BUFFER_SIZE];
static BUFFER_IX: AtomicU32 = AtomicU32::new(0);
const N_LINES: usize = 480 / FONT_HEIGHT as usize;

macro_rules! console {
    ($fmt_str:literal $(, $args:expr)*) => {{
        let s = alloc::format!($fmt_str $(, $args)*);
        $crate::console::write_string(&s);
    }};
}

pub fn write_string(s: &str) {
    let ix = BUFFER_IX.load(Relaxed);
    for (i, b) in s.bytes().enumerate() {
        BUFFER[(ix as usize + i) % BUFFER_SIZE].store(b, Relaxed);
    }
    BUFFER[(ix as usize + s.len()) % BUFFER_SIZE].store(b'\n', Relaxed);
    BUFFER_IX.fetch_add(s.len() as u32 + 1, Relaxed); // probably should be release
}

fn get_byte(ix: usize) -> u8 {
    BUFFER[ix % BUFFER_SIZE].load(Relaxed)
}

pub fn display_console() -> ! {
    loop {
        let end_ix = BUFFER_IX.load(Relaxed) as usize;
        let mut start_ix = end_ix;
        let mut n_lines = 0;
        while start_ix > end_ix.saturating_sub(BUFFER_SIZE) {
            let b = get_byte(start_ix);
            if b == b'\n' {
                if n_lines == N_LINES {
                    break;
                }
                n_lines += 1;
            }
            start_ix -= 1;
        }
        let mut height = 480;
        let (mut rb, mut sb) = start_display_list();
        while start_ix < end_ix {
            let mut line_end = start_ix;
            let mut s = String::new();
            while line_end < end_ix {
                let b = get_byte(line_end);
                if b == b'\n' {
                    break;
                }
                s.push(b as char);
                line_end += 1;
            }
            rb.begin_stripe(FONT_HEIGHT);

            let width = rb.text(&s);
            rb.end_stripe();
            sb.begin_stripe(FONT_HEIGHT);
            if width > 0 {
                sb.pal_1bpp(width, &BW_PALETTE_1BPP);
            }
            sb.solid(640 - width, rgb(0, 0, 0));
            sb.end_stripe();
            height -= FONT_HEIGHT;
            start_ix = line_end + 1;
        }
        if height > 0 {
            rb.begin_stripe(height);
            rb.end_stripe();
            sb.begin_stripe(height);
            sb.solid(640, rgb(0, 0, 0));
            sb.end_stripe();
        }
        end_display_list(rb, sb);
    }
}
