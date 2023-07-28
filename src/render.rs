use crate::dvi::tmds::TmdsPair;

#[link_section = ".data"]
static SMPTE_BARS: &[TmdsPair] = &[
    TmdsPair::encode_balanced_approx(0xc0),
    TmdsPair::encode_balanced_approx(0xc0),
    TmdsPair::encode_balanced_approx(0xc0),
    TmdsPair::encode_balanced_approx(0x00),
    TmdsPair::encode_balanced_approx(0xc0),
    TmdsPair::encode_balanced_approx(0xc0),
    TmdsPair::encode_balanced_approx(0xc0),
    TmdsPair::encode_balanced_approx(0xc0),
    TmdsPair::encode_balanced_approx(0x00),
    TmdsPair::encode_balanced_approx(0x00),
    TmdsPair::encode_balanced_approx(0xc0),
    TmdsPair::encode_balanced_approx(0x00),
    TmdsPair::encode_balanced_approx(0xc0),
    TmdsPair::encode_balanced_approx(0x00),
    TmdsPair::encode_balanced_approx(0xc0),
    TmdsPair::encode_balanced_approx(0x00),
    TmdsPair::encode_balanced_approx(0x00),
    TmdsPair::encode_balanced_approx(0xc0),
    TmdsPair::encode_balanced_approx(0xc0),
    TmdsPair::encode_balanced_approx(0x00),
    TmdsPair::encode_balanced_approx(0x00),
    // second row
    TmdsPair::encode_balanced_approx(0xc0),
    TmdsPair::encode_balanced_approx(0x00),
    TmdsPair::encode_balanced_approx(0x00),
    TmdsPair::encode_balanced_approx(0x13),
    TmdsPair::encode_balanced_approx(0x13),
    TmdsPair::encode_balanced_approx(0x13),
    TmdsPair::encode_balanced_approx(0xc0),
    TmdsPair::encode_balanced_approx(0x00),
    TmdsPair::encode_balanced_approx(0xc0),
    TmdsPair::encode_balanced_approx(0x13),
    TmdsPair::encode_balanced_approx(0x13),
    TmdsPair::encode_balanced_approx(0x13),
    TmdsPair::encode_balanced_approx(0xc0),
    TmdsPair::encode_balanced_approx(0xc0),
    TmdsPair::encode_balanced_approx(0x00),
    TmdsPair::encode_balanced_approx(0x13),
    TmdsPair::encode_balanced_approx(0x13),
    TmdsPair::encode_balanced_approx(0x13),
    TmdsPair::encode_balanced_approx(0xc0),
    TmdsPair::encode_balanced_approx(0xc0),
    TmdsPair::encode_balanced_approx(0xc0),
];

#[link_section = ".data"]
fn set_slice(slice: &mut [TmdsPair], val: TmdsPair) {
    for x in slice {
        *x = val;
    }
}

#[link_section = ".data"]
#[inline(never)]
pub fn render_scanline(tmds_buf: &mut [TmdsPair], y: u32) {
    let line = (y >= 160) as usize;
    for chan in 0..3 {
        for i in 0..7 {
            let val = SMPTE_BARS[line * 21 + i * 3 + chan];
            set_slice(&mut tmds_buf[(chan * 320 + i * 45)..][..45], val);
        }
    }
}
