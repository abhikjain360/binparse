use std::ops::{Add, Mul};

pub mod hooks;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Len {
    pub byte: usize,
    pub bit: usize,
}

impl Len {
    pub const ZERO: Self = Self { byte: 0, bit: 0 };

    pub fn bits(self) -> usize {
        self.byte.saturating_mul(8).saturating_add(self.bit)
    }

    pub fn byte_ceil(self) -> usize {
        self.byte.saturating_add(usize::from(self.bit > 0))
    }

    pub fn is_byte_aligned(self) -> bool {
        self.bit == 0
    }

    /// Rounds up to the next multiple of `align` bytes, first rounding any
    /// partial bit offset up to the next byte. `align` of 0 only byte-aligns.
    pub fn pad_to(self, align: usize) -> Self {
        let mut byte = self.byte_ceil();
        if align > 0 {
            let rem = byte % align;
            if rem != 0 {
                byte = byte.saturating_add(align - rem);
            }
        }
        Self { byte, bit: 0 }
    }
}

/// Context passed to consuming hooks: `fn(&[u8], HookContext<'_>) -> ParseResult<(T, usize)>`.
///
/// The data slice given to the hook spans from the field's start to the end
/// of the enclosing struct's (already bounded) slice; the returned `usize` is
/// the number of bytes consumed from the field's start and may not exceed that
/// slice. `enclosing` is the enclosing struct's full slice and `offset` the
/// field's byte offset within it, so hooks for back-referencing formats (e.g.
/// DNS name compression) can inspect earlier bytes without escaping the bound.
#[derive(Debug, Clone, Copy)]
pub struct HookContext<'a> {
    pub field: &'static str,
    pub offset: usize,
    pub enclosing: &'a [u8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    #[error("not enough data: expected {expected} bytes, got {got}")]
    NotEnoughData { expected: usize, got: usize },
    #[error("unaligned length: {0:?}")]
    UnalignedLength(Len),
    #[error("validation failed for field '{field}': actual value {actual}")]
    ValidationFailed { field: &'static str, actual: u128 },
    #[error("field '{field}' has more than {max} elements")]
    MaxIterationsExceeded { field: &'static str, max: usize },
    #[error("field '{field}' must start at a multiple of {align} bytes, got offset {offset:?}")]
    Misaligned {
        field: &'static str,
        align: usize,
        offset: Len,
    },
    #[error("hook failed for field '{field}': {reason}")]
    HookFailed {
        field: &'static str,
        reason: &'static str,
    },
}

pub type ParseResult<T> = std::result::Result<T, ParseError>;

impl Add for Len {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        let mut byte = self.byte.saturating_add(other.byte);
        let mut bit = self.bit + other.bit;
        if bit >= 8 {
            byte = byte.saturating_add(1);
            bit -= 8;
        }
        Self { byte, bit }
    }
}

impl Mul<usize> for Len {
    type Output = Self;

    fn mul(self, rhs: usize) -> Self::Output {
        let bit = self.bit.saturating_mul(rhs);
        Self {
            byte: self
                .byte
                .saturating_mul(rhs)
                .saturating_add(bit / 8),
            bit: bit % 8,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn len_bits_and_byte_ceil() {
        let len = Len { byte: 3, bit: 5 };
        assert_eq!(len.bits(), 29);
        assert_eq!(len.byte_ceil(), 4);
        assert!(!len.is_byte_aligned());
        assert_eq!(Len { byte: 2, bit: 0 }.byte_ceil(), 2);
        assert!(Len::ZERO.is_byte_aligned());
        assert_eq!(Len::ZERO.bits(), 0);
    }

    #[test]
    fn len_pad_to_rounds_up_to_alignment() {
        assert_eq!(Len { byte: 5, bit: 0 }.pad_to(4), Len { byte: 8, bit: 0 });
        assert_eq!(Len { byte: 8, bit: 0 }.pad_to(4), Len { byte: 8, bit: 0 });
        assert_eq!(Len { byte: 4, bit: 3 }.pad_to(4), Len { byte: 8, bit: 0 });
        assert_eq!(Len { byte: 4, bit: 3 }.pad_to(1), Len { byte: 5, bit: 0 });
        assert_eq!(Len { byte: 4, bit: 3 }.pad_to(0), Len { byte: 5, bit: 0 });
        assert_eq!(Len::ZERO.pad_to(4), Len::ZERO);
        assert_eq!(
            Len {
                byte: usize::MAX,
                bit: 1
            }
            .pad_to(8),
            Len {
                byte: usize::MAX,
                bit: 0
            }
        );
    }

    #[test]
    fn len_add_carries_bits() {
        let a = Len { byte: 1, bit: 5 };
        let b = Len { byte: 2, bit: 6 };
        assert_eq!(a + b, Len { byte: 4, bit: 3 });
    }

    #[test]
    fn len_mul_carries_bits() {
        let len = Len { byte: 1, bit: 3 };
        assert_eq!(len * 5, Len { byte: 6, bit: 7 });
    }

    #[test]
    fn len_arithmetic_saturates_instead_of_overflowing() {
        let max = Len {
            byte: usize::MAX,
            bit: 7,
        };
        assert_eq!(
            max + max,
            Len {
                byte: usize::MAX,
                bit: 6
            }
        );
        let big = Len {
            byte: usize::MAX / 2,
            bit: 0,
        };
        assert_eq!(big * 4, Len { byte: usize::MAX, bit: 0 });
        assert_eq!(max.bits(), usize::MAX);
        assert_eq!(max.byte_ceil(), usize::MAX);
    }
}
