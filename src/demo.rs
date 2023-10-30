use alloc::format;
use embedded_hal::digital::v2::ToggleableOutputPin;
use rp_pico::hal::gpio::{FunctionSioOutput, Pin, PinId, PullDown};

use crate::{
    dvi::VERTICAL_REPEAT,
    render::{end_display_list, rgb, start_display_list, BW_PALETTE, FONT_HEIGHT, GLOBAL_PALETTE},
};

const TILE_DATA: &[u32] = &[
    0x44444444, 0x44454444, 0x54545444, 0x44455454, 0x55555555, 0x55555555, 0x74754444, 0x74747474,
    0x44454444, 0x44444444, 0x44454644, 0x44444444, 0x44454644, 0x44444444, 0x44454444, 0x44444444,
    0x44454444, 0x44444444, 0x44455454, 0x54444444, 0x55555555, 0x55555555, 0x74747474, 0x74754474,
    0x44444444, 0x44454444, 0x44444444, 0x44454444, 0x66444444, 0x44454444, 0x44444444, 0x44454444,
];

struct Counter<P: PinId> {
    led_pin: Pin<P, FunctionSioOutput, PullDown>,
    count: u32,
}

impl<P: PinId> Counter<P> {
    // We might just want to move the led pin into the serializer,
    // but for the moment we let the app continue to own it.
    fn count(&mut self) {
        if self.count % 15 == 0 {
            self.led_pin.toggle().unwrap();
        }
        self.count = self.count.wrapping_add(1);
    }
}

fn colorbars<P: PinId>(counter: &mut Counter<P>) {
    counter.count();
    let height = 480 / VERTICAL_REPEAT as u32;
    let (mut rb, mut sb) = start_display_list();
    rb.begin_stripe(height - FONT_HEIGHT);
    rb.end_stripe();
    sb.begin_stripe(320 / VERTICAL_REPEAT as u32);
    sb.solid(92, rgb(0xc0, 0xc0, 0xc0));
    sb.solid(90, rgb(0xc0, 0xc0, 0));
    sb.solid(92, rgb(0, 0xc0, 0xc0));
    sb.solid(92, rgb(0, 0xc0, 0x0));
    sb.solid(92, rgb(0xc0, 0, 0xc0));
    sb.solid(90, rgb(0xc0, 0, 0));
    sb.solid(92, rgb(0, 0, 0xc0));
    sb.end_stripe();
    sb.begin_stripe(40 / VERTICAL_REPEAT as u32);
    sb.solid(92, rgb(0, 0, 0xc0));
    sb.solid(90, rgb(0x13, 0x13, 0x13));
    sb.solid(92, rgb(0xc0, 0, 0xc0));
    sb.solid(92, rgb(0x13, 0x13, 0x13));
    sb.solid(92, rgb(0, 0xc0, 0xc0));
    sb.solid(90, rgb(0x13, 0x13, 0x13));
    sb.solid(92, rgb(0xc0, 0xc0, 0xc0));
    sb.end_stripe();
    sb.begin_stripe(120 / VERTICAL_REPEAT as u32 - FONT_HEIGHT);
    sb.solid(114, rgb(0, 0x21, 0x4c));
    sb.solid(114, rgb(0xff, 0xff, 0xff));
    sb.solid(114, rgb(0x32, 0, 0x6a));
    sb.solid(116, rgb(0x13, 0x13, 0x13));
    sb.solid(30, rgb(0x09, 0x09, 0x09));
    sb.solid(30, rgb(0x13, 0x13, 0x13));
    sb.solid(30, rgb(0x1d, 0x1d, 0x1d));
    sb.solid(92, rgb(0x13, 0x13, 0x13));
    sb.end_stripe();
    rb.begin_stripe(FONT_HEIGHT);
    let text = format!("Hello pico-dvi-rs, frame {}", counter.count);
    let width = rb.text(&text);
    let width = width + width % 2;
    rb.end_stripe();
    sb.begin_stripe(FONT_HEIGHT);
    sb.pal_1bpp(width, &BW_PALETTE);
    sb.solid(640 - width, rgb(0, 0, 0));
    sb.end_stripe();
    end_display_list(rb, sb);
}

fn tiles<P: PinId>(counter: &mut Counter<P>) {
    counter.count();
    let (mut rb, mut sb) = start_display_list();
    let anim_frame = counter.count % 240;
    let (x_off, y_off) = if anim_frame < 60 {
        (anim_frame, 0)
    } else if anim_frame < 120 {
        (60, anim_frame - 60)
    } else if anim_frame < 180 {
        (180 - anim_frame, 60)
    } else {
        (0, 240 - anim_frame)
    };
    let height = 480 / VERTICAL_REPEAT as u32;
    let mut y = 0;
    let tiled_height = height - FONT_HEIGHT;
    while y < tiled_height {
        let ystart = if y == 0 { y_off % 16 } else { 0 };
        let this_height = (tiled_height - y).min(16 - ystart);
        rb.begin_stripe(this_height);
        let x = x_off % 16;
        let tile_top = &TILE_DATA[ystart as usize * 2..];
        rb.tile64(tile_top, x, 16);
        let mut x = 16 - x;
        while x < 640 {
            let width = (640 - x).min(16);
            rb.tile64(tile_top, 0, width);
            x += width;
        }
        rb.end_stripe();
        y += this_height;
    }
    sb.begin_stripe(tiled_height);
    unsafe {
        sb.pal_4bpp(640, &GLOBAL_PALETTE);
    }
    sb.end_stripe();
    rb.begin_stripe(FONT_HEIGHT);
    let text = format!("Hello pico-dvi-rs, frame {}", counter.count);
    let width = rb.text(&text);
    let width = width + width % 2;
    rb.end_stripe();
    sb.begin_stripe(FONT_HEIGHT);
    sb.pal_1bpp(width, &BW_PALETTE);
    sb.solid(640 - width, rgb(0, 0, 0));
    sb.end_stripe();
    end_display_list(rb, sb);
}

pub fn demo<P: PinId>(led_pin: Pin<P, FunctionSioOutput, PullDown>) -> ! {
    let mut counter = Counter { led_pin, count: 0 };

    loop {
        for _ in 0..120 {
            colorbars(&mut counter);
        }
        for _ in 0..240 {
            tiles(&mut counter);
        }
    }
}
