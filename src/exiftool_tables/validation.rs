//! Shared execution of source-authenticated directory size comparisons.
//! These are comparisons with a declared size, not structural bounds checks.

use crate::io::ByteOrder;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SizeExpectation {
    /// Declared size plus a source-provided signed constant.
    Relative(i32),
    Constant(u32),
    /// Native numeric division. Compare by exact multiplication so odd sizes
    /// do not get rounded down to an integer that incorrectly matches.
    Quotient(u32),
}

#[derive(Clone, Copy, Debug)]
pub struct U16SizeCheck {
    pub offset: u32,
    pub expected: &'static [SizeExpectation],
    /// Audit provenance; execution never dispatches by this name or path.
    pub expression: &'static str,
    pub callee: &'static str,
    pub source_file: &'static str,
    pub source_sha256: &'static str,
}

impl U16SizeCheck {
    /// The generated helper reads at start + offset in the supplied buffer.
    /// Native short/out-of-range Get16u is undefined and numeric comparison
    /// coerces it to zero. Reproduce that explicitly; callers still enforce
    /// their own file/directory bounds independently of this comparison.
    #[must_use]
    pub fn matches(&self, data: &[u8], start: usize, size: u32, order: ByteOrder) -> bool {
        let word = start
            .checked_add(self.offset as usize)
            .and_then(|at| at.checked_add(2).and_then(|end| data.get(at..end)))
            .map_or(0, |bytes| match order {
                ByteOrder::Big => u16::from_be_bytes([bytes[0], bytes[1]]),
                ByteOrder::Little => u16::from_le_bytes([bytes[0], bytes[1]]),
            });
        self.expected.iter().any(|expected| match *expected {
            SizeExpectation::Relative(delta) => {
                i64::from(word) == i64::from(size) + i64::from(delta)
            }
            SizeExpectation::Constant(value) => u32::from(word) == value,
            SizeExpectation::Quotient(divisor) => {
                divisor != 0 && u64::from(word) * u64::from(divisor) == u64::from(size)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(expected: &'static [SizeExpectation], offset: u32) -> U16SizeCheck {
        U16SizeCheck {
            offset,
            expected,
            expression: "test",
            callee: "test",
            source_file: "test",
            source_sha256: "test",
        }
    }

    #[test]
    fn inherited_order_and_declared_size_match_native_not_buffer_length() {
        let check = check(&[SizeExpectation::Relative(0)], 0);
        assert!(check.matches(&[0, 6], 0, 6, ByteOrder::Big));
        assert!(!check.matches(&[0, 6], 0, 6, ByteOrder::Little));
        assert!(!check.matches(&[0, 6], 0, 2, ByteOrder::Big));
    }

    #[test]
    fn alternatives_offsets_and_short_read_zero_coercion_match_native() {
        let check = check(
            &[SizeExpectation::Relative(-2), SizeExpectation::Relative(0)],
            2,
        );
        assert!(check.matches(&[9, 9, 4, 0], 0, 6, ByteOrder::Little));
        assert!(check.matches(&[9, 9, 6, 0], 0, 6, ByteOrder::Little));
        assert!(!check.matches(&[9, 9, 5, 0], 0, 6, ByteOrder::Little));
        assert!(check.matches(&[], 0, 2, ByteOrder::Big));
        assert!(check.matches(&[9], usize::MAX, 0, ByteOrder::Big));
        assert!(!check.matches(&[], 0, 1, ByteOrder::Big));
    }

    #[test]
    fn quotient_preserves_fractional_values_and_large_integer_domain() {
        let check = check(&[SizeExpectation::Quotient(2)], 0);
        assert!(check.matches(&[3, 0], 0, 6, ByteOrder::Little));
        assert!(!check.matches(&[3, 0], 0, 7, ByteOrder::Little));
        assert!(!check.matches(&[255, 255], 0, u32::MAX, ByteOrder::Little));
    }
}
