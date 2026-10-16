//! UTF-16 unit offsets into a Rust UTF-8 string, for the parser engine's
//! non-ASCII TSRX route whose tape spans use `OriginalUtf16Units`.

/// Maps UTF-16 code-unit offsets to UTF-8 byte offsets in `source`.
pub struct Utf16Index {
    /// For each UTF-16 unit offset, the UTF-8 byte offset, or `None` when the
    /// offset lands between a surrogate pair's halves. Length is `units + 1`.
    units_to_bytes: Vec<Option<u32>>,
}

impl Utf16Index {
    /// Builds the index and returns it with the UTF-16 encoding of `source`.
    ///
    /// Unpaired surrogates cannot appear in a Rust `&str`, so `encode_utf16` is lossless here.
    #[must_use]
    pub fn new(source: &str) -> (Self, Vec<u16>) {
        let mut units = Vec::with_capacity(source.len());
        let mut units_to_bytes = Vec::with_capacity(source.len() + 1);
        for (byte, ch) in source.char_indices() {
            let mut buf = [0u16; 2];
            let encoded = ch.encode_utf16(&mut buf);
            for (index, unit) in encoded.iter().enumerate() {
                units.push(*unit);
                // A position between the pair's halves has no byte boundary.
                units_to_bytes.push((index == 0).then(|| u32::try_from(byte).unwrap_or(u32::MAX)));
            }
        }
        units_to_bytes.push(Some(u32::try_from(source.len()).unwrap_or(u32::MAX)));
        (Self { units_to_bytes }, units)
    }

    /// Converts one UTF-16 unit offset to a UTF-8 byte offset.
    ///
    /// Returns `None` for an offset that lands mid-surrogate-pair or beyond the end; callers
    /// treat it as "not editable" rather than clamping into a wrong position.
    #[must_use]
    pub fn byte_at(&self, unit: u32) -> Option<u32> {
        self.units_to_bytes.get(unit as usize).copied().flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::Utf16Index;

    #[test]
    fn ascii_offsets_are_identical() {
        let (index, units) = Utf16Index::new("a.b(c)");
        assert_eq!(units.len(), 6);
        assert_eq!(index.byte_at(0), Some(0));
        assert_eq!(index.byte_at(6), Some(6));
        assert_eq!(index.byte_at(7), None);
    }

    #[test]
    fn wide_chars_shift_units() {
        // 'a' = 1 unit/1 byte, 'é' = 1 unit/2 bytes, '𝕏' = 2 units/4 bytes.
        let (index, units) = Utf16Index::new("aé𝕏z");
        assert_eq!(units.len(), 5);
        assert_eq!(index.byte_at(0), Some(0));
        assert_eq!(index.byte_at(1), Some(1)); // é byte offset
        assert_eq!(index.byte_at(2), Some(3)); // 𝕏 byte offset
        assert_eq!(index.byte_at(3), None); // inside surrogate pair
        assert_eq!(index.byte_at(4), Some(7)); // z byte offset
        assert_eq!(index.byte_at(5), Some(8)); // end
    }
}
