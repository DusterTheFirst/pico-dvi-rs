use crate::dvi::tmds::TmdsPair;

const fn bw_palette() -> [TmdsPair; 16] {
    let mut result = [TmdsPair::encode_balanced_approx(0); 16];
    let p = TmdsPair::encode_pair(0xff, 1);
    result[4] = p;
    result[5] = p;
    result[6] = p;
    let p = TmdsPair::encode_pair(0, 0xfe);
    result[8] = p;
    result[9] = p;
    result[10] = p;
    let p = TmdsPair::encode_pair(0xff, 0xfe);
    result[12] = p;
    result[13] = p;
    result[14] = p;
    result
}

#[link_section = ".scratch_x"]
pub static BW_PALETTE: [TmdsPair; 16] = bw_palette();
