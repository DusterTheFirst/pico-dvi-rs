/// [TMDS] encoding for DVI
/// [TMDS]: https://en.wikipedia.org/wiki/Transition-minimized_differential_signaling

/// A single [TMDS] symbol.
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

    pub const fn encode(discrepancy: i32, byte: u8) -> (i32, Self) {
        let byte_ones = byte.count_ones();

        // The first step of encoding TMDS data is to XOR/XNOR each input bit with the previous output bit, one by one
        //
        // Instead using a for loop over each bit, TMDS encoding can be approached
        // as a carry-less multiplication with 255. The decoding step would be a
        // carry-less multiplication with 3.
        //
        // To reduce the amount of steps, the carry-less multiplication with 255
        // can be split up into a multiplication with 3 * 5 * 17. The following
        // 3 lines respectively can be seen as those smaller multiplications.
        let byte_mul = ((byte as u32) << 1) ^ byte as u32;
        let byte_mul = (byte_mul << 2) ^ byte_mul;
        let byte_mul = (byte_mul << 4) ^ byte_mul;
        // We only care about the bottom byte
        let mut byte_encoded = byte_mul & 0xff;

        let should_xnor = byte_ones > 4 || (byte_ones == 4 && (byte_encoded & 1) == 0);

        if should_xnor {
            // Convert the XOR case to the XNOR case, toggling every other bit
            byte_encoded ^= 0xaa
        } else {
            // Set bit 8 to indicate XOR
            byte_encoded ^= 0x100
        };

        let encoded_ones = u8::count_ones(byte_encoded as _);

        let should_invert = if discrepancy == 0 || encoded_ones == 4 {
            (byte_encoded >> 8) == 0
        } else {
            (discrepancy > 0) == (encoded_ones > 4)
        };

        let bit_8 = (byte_encoded >> 8) & 1;
        let symbol_ones = if should_invert {
            // Invert the lower byte and set bit 9
            byte_encoded ^= 0x2ff;

            // Invert the ones count of the lower 8 bits, add bit 8 and bit 9
            (8 - encoded_ones) + bit_8 + 1
        } else {
            encoded_ones + bit_8
        };

        let discrepancy = discrepancy + (symbol_ones as i32 - 5);

        (discrepancy, TmdsSymbol(byte_encoded))
    }
}

impl TmdsPair {
    pub const fn new(sym0: TmdsSymbol, sym1: TmdsSymbol) -> Self {
        TmdsPair(sym0.0 | ((sym1.0) << 10))
    }

    pub const fn double(symbol: TmdsSymbol) -> Self {
        Self::new(symbol, symbol)
    }

    /// Encode a pair of bytes.
    ///
    /// This is not guaranteed to be DC balanced unless the bytes are
    /// carefully chosen.
    pub const fn encode_pair(b0: u8, b1: u8) -> Self {
        let (pair, discrepancy) = Self::encode_pair_discrepancy(b0, b1);

        // TODO: REMOVE?
        // Ensure the pair is balanced in testing
        debug_assert!(discrepancy == 0);

        pair
    }

    /// Encode a pair of bytes, returning the pair and their discrepancy.
    pub const fn encode_pair_discrepancy(b0: u8, b1: u8) -> (Self, i32) {
        let (discrepancy, symbol_0) = TmdsSymbol::encode(0, b0);
        let (discrepancy, symbol_1) = TmdsSymbol::encode(discrepancy, b1);

        (Self::new(symbol_0, symbol_1), discrepancy)
    }

    /// Encode two copies of a byte, approximating to achieve DC balance.
    ///
    /// This method takes advantage of the fact that two values differing
    /// only in the least significant bit add are DC balanced.
    pub const fn encode_balanced_approx(byte: u8) -> Self {
        Self::encode_pair(byte, byte ^ 1)
    }

    pub const fn raw(self) -> u32 {
        self.0
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
