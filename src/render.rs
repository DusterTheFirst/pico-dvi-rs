use crate::{
    dvi::{tmds::TmdsPair, VERTICAL_REPEAT},
    scanlist::{Scanlist, ScanlistBuilder},
};

pub struct ScanRender {
    scanlist: Scanlist,
    stripe_remaining: u32,
    scan_ptr: *const u32,
    scan_next: *const u32,
}

core::arch::global_asm! {
    include_str!("render.asm"),
    options(raw)
}

extern "C" {
    fn tmds_scan(
        scan_list: *const u32,
        input: *const u32,
        output: *mut TmdsPair,
        stride: u32,
    ) -> *const u32;
}

fn rgb(r: u8, g: u8, b: u8) -> [TmdsPair; 3] {
    [
        TmdsPair::encode_balanced_approx(b),
        TmdsPair::encode_balanced_approx(g),
        TmdsPair::encode_balanced_approx(r),
    ]
}

impl ScanRender {
    pub fn new() -> Self {
        let mut sb = ScanlistBuilder::new(640, 480 / VERTICAL_REPEAT as u32);
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
        let scanlist = sb.build();
        let stripe_remaining = 0;
        let scan_ptr = core::ptr::null();
        let scan_next = core::ptr::null();
        ScanRender {
            scanlist,
            stripe_remaining,
            scan_ptr,
            scan_next,
        }
    }

    #[link_section = ".data"]
    #[inline(never)]
    pub fn render_scanline(&mut self, tmds_buf: &mut [TmdsPair], y: u32) {
        unsafe {
            if y == 0 {
                self.scan_next = self.scanlist.get().as_ptr();
            }
            if self.stripe_remaining == 0 {
                self.stripe_remaining = self.scan_next.read();
                self.scan_ptr = self.scan_next.add(1);
            }
            self.scan_next = tmds_scan(
                self.scan_ptr,
                core::ptr::null(),
                tmds_buf.as_mut_ptr(),
                1280,
            );
            self.stripe_remaining -= 1;
        }
    }
}
