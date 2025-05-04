use embedded_hal::digital::StatefulOutputPin;
use rp235x_hal::gpio::{FunctionSioOutput, Pin, PullDown};

use crate::{
    dvi::VERTICAL_REPEAT,
    hal::gpio::PinId,
    render::{end_display_list, rgb, start_display_list},
};

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

fn colorbars<P: PinId>(_counter: &Counter<P>) {
    let height = 480 / VERTICAL_REPEAT as u32;
    let (mut rb, mut sb) = start_display_list();
    rb.begin_stripe(height);
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
    sb.begin_stripe(120 / VERTICAL_REPEAT as u32);
    sb.solid(114, rgb(0, 0x21, 0x4c));
    sb.solid(114, rgb(0xff, 0xff, 0xff));
    sb.solid(114, rgb(0x32, 0, 0x6a));
    sb.solid(116, rgb(0x13, 0x13, 0x13));
    sb.solid(30, rgb(0x09, 0x09, 0x09));
    sb.solid(30, rgb(0x13, 0x13, 0x13));
    sb.solid(30, rgb(0x1d, 0x1d, 0x1d));
    sb.solid(92, rgb(0x13, 0x13, 0x13));
    sb.end_stripe();
    end_display_list(rb, sb);
}

pub fn demo<P: PinId>(led_pin: Pin<P, FunctionSioOutput, PullDown>) -> ! {
    let mut counter = Counter { led_pin, count: 0 };

    loop {
        counter.count();
        colorbars(&counter);
    }
}
