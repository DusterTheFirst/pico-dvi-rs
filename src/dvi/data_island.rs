use core::{arch::asm, mem::MaybeUninit};

#[derive(Default)]
pub struct DataPacket {
    pub header: [u8; 4],
    pub subpacket: [[u8; 8]; 4],
}

#[repr(u8)]
#[allow(unused)]
pub enum ScanInfo {
    NoData,
    Overscan,
    Underscan,
}

#[repr(u8)]
#[allow(unused)]
pub enum PixelFormat {
    Rgb,
    Ycbcr422,
    Ycbcr444,
}

#[repr(u8)]
#[allow(unused)]
pub enum Colorimetry {
    NoData,
    Itu601,
    Itu709,
    Extended,
}

#[repr(u8)]
#[allow(unused)]
pub enum PictureAspectRatio {
    NoData,
    Ratio4_3,
    Ratio16_9,
}

#[repr(u8)]
#[allow(unused)]
#[derive(PartialEq, Eq)]
pub enum ActiveFormatAspectRatio {
    NoData = !0,
    SameAsPar = 8,
    Ratio4_3,
    Ratio16_9,
    Ratio14_9,
}

#[repr(u8)]
#[allow(unused)]
pub enum QuantizationRange {
    Default,
    Limited,
    Full,
}

#[repr(u8)]
#[allow(unused)]
pub enum VideoCode {
    Code640x480P60 = 1,
    Code720x480P60 = 2,
    // Many other codes exist, but we can't reach them from RP2040
}

#[link_section = ".data"]
pub static TERC4_SYMBOLS: [u16; 16] = [
    0b1010011100,
    0b1001100011,
    0b1011100100,
    0b1011100010,
    0b0101110001,
    0b0100011110,
    0b0110001110,
    0b0100111100,
    0b1011001100,
    0b0100111001,
    0b0110011100,
    0b1011000110,
    0b1010001110,
    0b1001110001,
    0b0101100011,
    0b1011000011,
];

const fn mk_terc4_table() -> [u32; 256] {
    let mut table = [0; 256];
    let mut i = 0;
    while i < 256 {
        let j = (i & 0x81)
            | (i & 2) << 3
            | (i & 4) >> 1
            | (i & 8) << 2
            | (i & 0x10) >> 2
            | (i & 0x20) << 1
            | (i & 0x40) >> 3;
        let c1 = (TERC4_SYMBOLS[j % 16] as u32) << 10;
        let c2 = (TERC4_SYMBOLS[j / 16] as u32) << 20;
        table[i] = c1 | c2;
        i += 1;
    }
    table
}

#[link_section = ".data"]
pub static TERC4_TABLE: [u32; 256] = mk_terc4_table();

#[link_section = ".data"]
static BCH_TABLE: [u8; 256] = [
    0x00, 0xd9, 0xb5, 0x6c, 0x6d, 0xb4, 0xd8, 0x01, 0xda, 0x03, 0x6f, 0xb6, 0xb7, 0x6e, 0x02, 0xdb,
    0xb3, 0x6a, 0x06, 0xdf, 0xde, 0x07, 0x6b, 0xb2, 0x69, 0xb0, 0xdc, 0x05, 0x04, 0xdd, 0xb1, 0x68,
    0x61, 0xb8, 0xd4, 0x0d, 0x0c, 0xd5, 0xb9, 0x60, 0xbb, 0x62, 0x0e, 0xd7, 0xd6, 0x0f, 0x63, 0xba,
    0xd2, 0x0b, 0x67, 0xbe, 0xbf, 0x66, 0x0a, 0xd3, 0x08, 0xd1, 0xbd, 0x64, 0x65, 0xbc, 0xd0, 0x09,
    0xc2, 0x1b, 0x77, 0xae, 0xaf, 0x76, 0x1a, 0xc3, 0x18, 0xc1, 0xad, 0x74, 0x75, 0xac, 0xc0, 0x19,
    0x71, 0xa8, 0xc4, 0x1d, 0x1c, 0xc5, 0xa9, 0x70, 0xab, 0x72, 0x1e, 0xc7, 0xc6, 0x1f, 0x73, 0xaa,
    0xa3, 0x7a, 0x16, 0xcf, 0xce, 0x17, 0x7b, 0xa2, 0x79, 0xa0, 0xcc, 0x15, 0x14, 0xcd, 0xa1, 0x78,
    0x10, 0xc9, 0xa5, 0x7c, 0x7d, 0xa4, 0xc8, 0x11, 0xca, 0x13, 0x7f, 0xa6, 0xa7, 0x7e, 0x12, 0xcb,
    0x83, 0x5a, 0x36, 0xef, 0xee, 0x37, 0x5b, 0x82, 0x59, 0x80, 0xec, 0x35, 0x34, 0xed, 0x81, 0x58,
    0x30, 0xe9, 0x85, 0x5c, 0x5d, 0x84, 0xe8, 0x31, 0xea, 0x33, 0x5f, 0x86, 0x87, 0x5e, 0x32, 0xeb,
    0xe2, 0x3b, 0x57, 0x8e, 0x8f, 0x56, 0x3a, 0xe3, 0x38, 0xe1, 0x8d, 0x54, 0x55, 0x8c, 0xe0, 0x39,
    0x51, 0x88, 0xe4, 0x3d, 0x3c, 0xe5, 0x89, 0x50, 0x8b, 0x52, 0x3e, 0xe7, 0xe6, 0x3f, 0x53, 0x8a,
    0x41, 0x98, 0xf4, 0x2d, 0x2c, 0xf5, 0x99, 0x40, 0x9b, 0x42, 0x2e, 0xf7, 0xf6, 0x2f, 0x43, 0x9a,
    0xf2, 0x2b, 0x47, 0x9e, 0x9f, 0x46, 0x2a, 0xf3, 0x28, 0xf1, 0x9d, 0x44, 0x45, 0x9c, 0xf0, 0x29,
    0x20, 0xf9, 0x95, 0x4c, 0x4d, 0x94, 0xf8, 0x21, 0xfa, 0x23, 0x4f, 0x96, 0x97, 0x4e, 0x22, 0xfb,
    0x93, 0x4a, 0x26, 0xff, 0xfe, 0x27, 0x4b, 0x92, 0x49, 0x90, 0xfc, 0x25, 0x24, 0xfd, 0x91, 0x48,
];

#[link_section = ".data"]
fn compute_bch(inp: &[u8]) -> u8 {
    inp.iter().fold(0, |v, b| BCH_TABLE[(b ^ v) as usize])
}

#[link_section = ".data"]
pub fn clear_data_packet(dp: &mut MaybeUninit<DataPacket>) {
    unsafe {
        asm!(
            "strd {zero}, {zero}, [{ptr}]",
            "strd {zero}, {zero}, [{ptr}, #8]",
            "strd {zero}, {zero}, [{ptr}, #16]",
            "strd {zero}, {zero}, [{ptr}, #24]",
            "str {zero}, [{ptr}, #32]",
            zero = in(reg) 0,
            ptr = in(reg) dp as *mut _,
            options(nostack),
        );
    }
}

impl DataPacket {
    #[link_section = ".data"]
    fn compute_header_parity(&mut self) {
        self.header[3] = compute_bch(&self.header[0..3]);
    }

    #[link_section = ".data"]
    fn compute_subpacket_parity(&mut self, i: usize) {
        self.subpacket[i][7] = compute_bch(&self.subpacket[i][0..7]);
    }

    #[link_section = ".data"]
    fn compute_info_frame_checksum(&mut self) {
        let mut s = 0u8;
        for i in 0..3 {
            s = s.wrapping_add(self.header[i]);
        }
        let mut n = self.header[2] + 1;
        for j in 0..4 {
            let len = 7.min(n);
            for i in 0..len {
                s = s.wrapping_add(self.subpacket[j][i as usize]);
            }
            n -= len;
        }
        self.subpacket[0][0] = s.wrapping_neg();
    }

    #[link_section = ".data"]
    pub fn set_audio(&mut self, audio: &[[i16; 2]], frame_count: &mut i32) {
        self.header[0] = 2;
        let n = audio.len();
        let sample_present = (1 << n) - 1;
        self.header[1] = sample_present;
        let b = if *frame_count < 4 {
            1 << *frame_count
        } else {
            0
        };
        self.header[2] = b << 4;
        self.compute_header_parity();
        for i in 0..n {
            let [l, r] = audio[i];
            self.subpacket[i][0] = 0;
            self.subpacket[i][1] = l as u8;
            self.subpacket[i][2] = (l >> 8) as u8;
            self.subpacket[i][3] = 0;
            self.subpacket[i][4] = r as u8;
            self.subpacket[i][5] = (r >> 8) as u8;
            let pl = parity_u16(l as u16);
            let pr = parity_u16(r as u16);
            self.subpacket[i][6] = ((pl << 3) | (pr << 7)) ^ 0x99;
            self.compute_subpacket_parity(i);
        }
        // Assumes packet is clear
        //for i in n..4 {
        //    self.subpacket[i] = [0; 8];
        //}
        *frame_count -= n as i32;
        if *frame_count < 0 {
            *frame_count += 192;
        }
    }

    #[link_section = ".data"]
    pub fn set_audio_clock_regeneration(&mut self, cts: u32, n: u32) {
        self.header[0] = 1;
        self.header[1] = 0;
        self.header[2] = 0;
        self.compute_header_parity();
        self.subpacket[0][0] = 0;
        self.subpacket[0][1] = (cts >> 16) as u8;
        self.subpacket[0][2] = (cts >> 8) as u8;
        self.subpacket[0][3] = cts as u8;
        self.subpacket[0][4] = (n >> 16) as u8;
        self.subpacket[0][5] = (n >> 8) as u8;
        self.subpacket[0][6] = n as u8;
        self.compute_subpacket_parity(0);
        for i in 1..4 {
            self.subpacket[i] = self.subpacket[0];
        }
    }

    #[link_section = ".data"]
    pub fn set_audio_info_frame(&mut self, freq: u32) {
        self.header[0] = 0x84;
        self.header[1] = 1; // version
        self.header[2] = 10; // length
        self.compute_header_parity();
        let cc = 1; // 2 channels
        let ct = 1; // IEC 60958 PCM
        let ss = 1; // 16 bit
        let sf = match freq {
            48000 => 3,
            44100 => 2,
            _ => 0,
        };
        let ca = 0; // speaker placement; FR, FL
        let lsv = 0; // level shift, 0db
        let dm_inh = 0;
        for i in 0..4 {
            self.subpacket[i] = [0; 8];
        }
        self.subpacket[0][1] = cc | (ct << 4);
        self.subpacket[0][2] = ss | (sf << 2);
        self.subpacket[0][4] = ca;
        self.subpacket[0][5] = (lsv << 3) | (dm_inh << 7);
        self.compute_info_frame_checksum();
        self.compute_subpacket_parity(0);
    }

    #[link_section = ".data"]
    pub fn set_avi_info_frame(
        &mut self,
        s: ScanInfo,
        y: PixelFormat,
        c: Colorimetry,
        m: PictureAspectRatio,
        r: ActiveFormatAspectRatio,
        q: QuantizationRange,
        v: VideoCode,
    ) {
        self.header[0] = 0x82;
        self.header[1] = 2; // version
        self.header[2] = 13; // length
        self.compute_header_parity();
        for i in 0..4 {
            self.subpacket[i] = [0; 8];
        }
        let a = (r != ActiveFormatAspectRatio::NoData) as u8;
        let sc = 0; // no non-uniform picture scaling
        self.subpacket[0][1] = s as u8 | (a << 4) | ((y as u8) << 5);
        self.subpacket[0][2] = r as u8 | ((m as u8) << 4) | ((c as u8) << 6);
        self.subpacket[0][3] = sc | ((q as u8) << 2);
        self.subpacket[0][4] = v as u8;
        self.compute_info_frame_checksum();
        self.compute_subpacket_parity(0);
    }

    #[link_section = ".data"]
    pub fn encode(&self, hv: u8, result: &mut [u32]) {
        let c0_base = hv as usize + 8;
        let c0_guard = TERC4_SYMBOLS[c0_base + 4];
        let gb = c0_guard as u32 | (0x133 << 10) | (0x133 << 20);
        result[0] = gb;
        result[1] = gb;
        result[34] = gb;
        result[35] = gb;
        let header = u32::from_le_bytes(self.header);
        for i in 0..8 {
            let v = self.subpacket[0][i] as u32
                | ((self.subpacket[1][i] as u32) << 8)
                | ((self.subpacket[2][i] as u32) << 16)
                | ((self.subpacket[3][i] as u32) << 24);
            let t = (v ^ (v >> 6)) & 0xcc00cc;
            let v = v ^ t ^ (t << 6);
            let t = (v ^ (v >> 12)) & 0xf0f0;
            let v = v ^ t ^ (t << 12);
            for j in 0..4 {
                let ix = i * 4 + j;
                let header_bit = ((header >> ix) & 1) as usize;
                let first_off = (ix == 0) as usize * 8;
                let c0 = TERC4_SYMBOLS[c0_base - first_off + header_bit * 4];
                let mut tmds = c0 as u32;
                tmds |= TERC4_TABLE[((v >> (j * 8)) & 0xff) as usize];
                result[ix + 2] = tmds;
            }
        }
    }
}

fn parity_u16(x: u16) -> u8 {
    let a = x ^ (x >> 8);
    let a = a ^ (a >> 4);
    let a = a ^ (a >> 2);
    let a = a ^ (a >> 1);
    a as u8 & 1
}
