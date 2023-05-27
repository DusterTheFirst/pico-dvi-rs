// TMDS encoding for DVI

/// A single TMDS symbol.
///
/// The [TMDS] encoding for DVI produces one 10-bit symbol for each 8
/// bit word.
///
/// [TMDS]: https://en.wikipedia.org/wiki/Transition-minimized_differential_signaling
#[derive(Clone, Copy)]
pub struct TmdsSym(u32);

/// A pair of TMDS symbols.
///
/// These are packed two to a word, laid out for serialization.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct TmdsPair(u32);

/// A version of popcnt that works on one-byte values, designed
/// to be compact in code size and fast on 32 bit microcontrollers.
const fn popcnt_byte(byte: u32) -> u32 {
    let a = ((byte >> 1) & 0x55) + (byte & 0x55);
    let b = ((a >> 2) & 0x33) + (a & 0x33);
    ((b >> 4) + b) & 0xf
}

impl TmdsSym {
    pub const C0: Self = TmdsSym(0x354);
    pub const C1: Self = TmdsSym(0xab);
    pub const C2: Self = TmdsSym(0x154);
    pub const C3: Self = TmdsSym(0x2ab);

    pub const fn encode(discrepancy: i32, byte: u32) -> (i32, Self) {
        let cnt = popcnt_byte(byte);
        let a = (byte << 1) ^ byte;
        let b = (a << 2) ^ a;
        let mut c = ((b << 4) ^ b) & 0xff;
        let mut cnt_c = popcnt_byte(c);
        if cnt > 4 || (cnt == 4 && (c & 1) == 0) {
            c ^= 0xaa;
        } else {
            c ^= 0x100;
        }
        let invert = if discrepancy == 0 || cnt_c == 4 {
            (c >> 8) == 0
        } else {
            (discrepancy > 0) == (cnt_c > 4)
        };
        if invert {
            c ^= 0x2ff;
            cnt_c = 9 - cnt_c;
        }
        cnt_c += (c >> 8) & 1;
        (discrepancy + (cnt_c as i32 - 5), TmdsSym(c))
    }
}

impl TmdsPair {
    pub const fn new(sym0: TmdsSym, sym1: TmdsSym) -> Self {
        TmdsPair(sym0.0 | ((sym1.0) << 10))
    }

    pub const fn double(sym: TmdsSym) -> Self {
        Self::new(sym, sym)
    }

    /// Encode two copies of a byte, approximating to achieve DC balance.
    pub const fn encode_balanced_approx(byte: u32) -> Self {
        let (discrepancy, sym0) = TmdsSym::encode(0, byte);
        if discrepancy == 0 {
            Self::double(sym0)
        } else {
            let (_, sym1) = TmdsSym::encode(discrepancy, byte ^ 1);
            Self::new(sym0, sym1)
        }
    }
}
