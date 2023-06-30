// TMDS encoding for DVI

/// A single TMDS symbol.
///
/// The [TMDS] encoding for DVI produces one 10-bit symbol for each 8
/// bit word.
///
/// [TMDS]: https://en.wikipedia.org/wiki/Transition-minimized_differential_signaling
#[derive(Clone, Copy)]
pub struct TmdsSymbol(u32);

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

impl TmdsSymbol {
    pub const C0: Self = TmdsSymbol(0x354);
    pub const C1: Self = TmdsSymbol(0xab);
    pub const C2: Self = TmdsSymbol(0x154);
    pub const C3: Self = TmdsSymbol(0x2ab);

    pub const fn encode(discrepancy: i32, byte: u32) -> (i32, Self) {
        let byte_ones = popcnt_byte(byte);
        let a = (byte << 1) ^ byte;
        let b = (a << 2) ^ a;
        let mut c = ((b << 4) ^ b) & 0xff;

        if byte_ones > 4 || (byte_ones == 4 && (c & 1) == 0) {
            c ^= 0xaa;
        } else {
            c ^= 0x100;
        }

        let mut c_ones = popcnt_byte(c & 0xff);

        let invert = if discrepancy == 0 || c_ones == 4 {
            (c >> 8) == 0
        } else {
            (discrepancy > 0) == (c_ones > 4)
        };

        if invert {
            c ^= 0x2ff;
            c_ones = 9 - c_ones;
        }

        c_ones += (c >> 8) & 1;

        (discrepancy + (c_ones as i32 - 5), TmdsSymbol(c))
    }
}

impl TmdsPair {
    pub const fn new(sym0: TmdsSymbol, sym1: TmdsSymbol) -> Self {
        TmdsPair(sym0.0 | ((sym1.0) << 10))
    }

    pub const fn double(symbol: TmdsSymbol) -> Self {
        Self::new(symbol, symbol)
    }

    /// Encode two copies of a byte, approximating to achieve DC balance.
    pub const fn encode_balanced_approx(byte: u32) -> Self {
        let (discrepancy, symbol_0) = TmdsSymbol::encode(0, byte);
        if discrepancy == 0 {
            Self::double(symbol_0)
        } else {
            let (_, symbol_1) = TmdsSymbol::encode(discrepancy, byte ^ 1);
            Self::new(symbol_0, symbol_1)
        }
    }
}
