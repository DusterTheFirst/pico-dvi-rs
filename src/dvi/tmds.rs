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

impl TmdsSymbol {
    pub const C0: Self = TmdsSymbol(0x354);
    pub const C1: Self = TmdsSymbol(0xab);
    pub const C2: Self = TmdsSymbol(0x154);
    pub const C3: Self = TmdsSymbol(0x2ab);

    pub const fn encode(discrepancy: i32, byte: u32) -> (i32, Self) {
        let byte_ones = u8::count_ones(byte as u8);
        let a = (byte << 1) ^ byte;
        let b = (a << 2) ^ a;
        let mut c = ((b << 4) ^ b) & 0xff;

        if byte_ones > 4 || (byte_ones == 4 && (c & 1) == 0) {
            c ^= 0xaa;
        } else {
            c ^= 0x100;
        }

        let mut c_ones = u8::count_ones(c as u8);

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
    ///
    /// This method takes advantage of the fact that two values differing
    /// only in the least significant bit add are DC balanced.
    pub const fn encode_balanced_approx(byte: u32) -> Self {
        let (discrepancy, symbol_0) = TmdsSymbol::encode(0, byte);
        let (_, symbol_1) = TmdsSymbol::encode(discrepancy, byte ^ 1);
        Self::new(symbol_0, symbol_1)
    }
}

// TODO: https://lib.rs/crates/defmt-test
// TODO: generate test cases from known working implementation???
#[cfg(test)]
#[defmt_test::tests]
mod test {
    #[test]
    fn encode() {}
}
