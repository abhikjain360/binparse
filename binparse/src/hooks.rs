use crate::{HookContext, ParseError, ParseResult};

pub fn cstring(data: &[u8], _ctx: HookContext<'_>) -> ParseResult<(String, usize)> {
    match data.iter().position(|&b| b == 0) {
        Some(end) => Ok((String::from_utf8_lossy(&data[..end]).to_string(), end + 1)),
        None => Err(ParseError::NotEnoughData {
            expected: data.len().saturating_add(1),
            got: data.len(),
        }),
    }
}

pub fn leb128_unsigned(data: &[u8], ctx: HookContext<'_>) -> ParseResult<(u64, usize)> {
    let mut result: u64 = 0;
    let mut shift = 0u32;

    for (i, &byte) in data.iter().enumerate() {
        if shift > 63 || (shift == 63 && byte & 0x7E != 0) {
            return Err(ParseError::HookFailed {
                field: ctx.field,
                reason: "LEB128 value overflows u64",
            });
        }
        result |= u64::from(byte & 0x7F) << shift;
        if byte & 0x80 == 0 {
            return Ok((result, i + 1));
        }
        shift += 7;
    }

    Err(ParseError::NotEnoughData {
        expected: data.len().saturating_add(1),
        got: data.len(),
    })
}

pub fn leb128_signed(data: &[u8], ctx: HookContext<'_>) -> ParseResult<(i64, usize)> {
    let mut result: i64 = 0;
    let mut shift = 0u32;

    for (i, &byte) in data.iter().enumerate() {
        if shift > 63 {
            return Err(ParseError::HookFailed {
                field: ctx.field,
                reason: "LEB128 value overflows i64",
            });
        }
        result |= i64::from(byte & 0x7F) << shift;
        shift += 7;
        if byte & 0x80 == 0 {
            if shift < 64 && byte & 0x40 != 0 {
                result |= !0i64 << shift;
            }
            return Ok((result, i + 1));
        }
    }

    Err(ParseError::NotEnoughData {
        expected: data.len().saturating_add(1),
        got: data.len(),
    })
}

pub fn zigzag_varint(data: &[u8], ctx: HookContext<'_>) -> ParseResult<(i64, usize)> {
    let (raw, consumed) = leb128_unsigned(data, ctx)?;
    Ok((((raw >> 1) as i64) ^ -((raw & 1) as i64), consumed))
}

pub fn quic_varint(data: &[u8], _ctx: HookContext<'_>) -> ParseResult<(u64, usize)> {
    let Some(&first) = data.first() else {
        return Err(ParseError::NotEnoughData {
            expected: 1,
            got: 0,
        });
    };
    let len = 1usize << (first >> 6);
    if data.len() < len {
        return Err(ParseError::NotEnoughData {
            expected: len,
            got: data.len(),
        });
    }
    let mut value = u64::from(first & 0x3F);
    for &byte in &data[1..len] {
        value = (value << 8) | u64::from(byte);
    }
    Ok((value, len))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(data: &[u8]) -> HookContext<'_> {
        HookContext {
            field: "Test.field",
            offset: 0,
            enclosing: data,
        }
    }

    #[test]
    fn test_cstring_basic() {
        let data = b"hello\0world";
        let (s, len) = cstring(data, ctx(data)).unwrap();
        assert_eq!(s, "hello");
        assert_eq!(len, 6);
    }

    #[test]
    fn test_cstring_no_null_is_error() {
        let data = b"hello";
        assert_eq!(
            cstring(data, ctx(data)),
            Err(ParseError::NotEnoughData {
                expected: 6,
                got: 5
            })
        );
    }

    #[test]
    fn test_cstring_empty() {
        let data = b"\0rest";
        let (s, len) = cstring(data, ctx(data)).unwrap();
        assert_eq!(s, "");
        assert_eq!(len, 1);
    }

    #[test]
    fn test_leb128_unsigned_single_byte() {
        let data = [0x7F];
        let (val, len) = leb128_unsigned(&data, ctx(&data)).unwrap();
        assert_eq!(val, 127);
        assert_eq!(len, 1);
    }

    #[test]
    fn test_leb128_unsigned_multi_byte() {
        let data = [0xE5, 0x8E, 0x26];
        let (val, len) = leb128_unsigned(&data, ctx(&data)).unwrap();
        assert_eq!(val, 624485);
        assert_eq!(len, 3);
    }

    #[test]
    fn test_leb128_unsigned_zero() {
        let data = [0x00];
        let (val, len) = leb128_unsigned(&data, ctx(&data)).unwrap();
        assert_eq!(val, 0);
        assert_eq!(len, 1);
    }

    #[test]
    fn test_leb128_unsigned_max() {
        let data = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x01];
        let (val, len) = leb128_unsigned(&data, ctx(&data)).unwrap();
        assert_eq!(val, u64::MAX);
        assert_eq!(len, 10);
    }

    #[test]
    fn test_leb128_unsigned_unterminated_is_error() {
        let data = [0xFF, 0xFF];
        assert_eq!(
            leb128_unsigned(&data, ctx(&data)),
            Err(ParseError::NotEnoughData {
                expected: 3,
                got: 2
            })
        );
    }

    #[test]
    fn test_leb128_unsigned_overflow_is_error() {
        let data = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x02];
        assert_eq!(
            leb128_unsigned(&data, ctx(&data)),
            Err(ParseError::HookFailed {
                field: "Test.field",
                reason: "LEB128 value overflows u64",
            })
        );
        let data = [0xFF; 11];
        assert!(matches!(
            leb128_unsigned(&data, ctx(&data)),
            Err(ParseError::HookFailed { .. })
        ));
    }

    #[test]
    fn test_leb128_signed_positive() {
        let data = [0x7F];
        let (val, len) = leb128_signed(&data, ctx(&data)).unwrap();
        assert_eq!(val, -1);
        assert_eq!(len, 1);
    }

    #[test]
    fn test_leb128_signed_negative() {
        let data = [0xC0, 0xBB, 0x78];
        let (val, len) = leb128_signed(&data, ctx(&data)).unwrap();
        assert_eq!(val, -123456);
        assert_eq!(len, 3);
    }

    #[test]
    fn test_leb128_signed_zero() {
        let data = [0x00];
        let (val, len) = leb128_signed(&data, ctx(&data)).unwrap();
        assert_eq!(val, 0);
        assert_eq!(len, 1);
    }

    #[test]
    fn test_leb128_signed_positive_multi() {
        let data = [0x80 | 57, 0x00];
        let (val, len) = leb128_signed(&data, ctx(&data)).unwrap();
        assert_eq!(val, 57);
        assert_eq!(len, 2);
    }

    #[test]
    fn test_leb128_signed_unterminated_is_error() {
        let data = [0xC0];
        assert_eq!(
            leb128_signed(&data, ctx(&data)),
            Err(ParseError::NotEnoughData {
                expected: 2,
                got: 1
            })
        );
    }

    #[test]
    fn test_leb128_signed_overflow_is_error() {
        let data = [0xFF; 11];
        assert!(matches!(
            leb128_signed(&data, ctx(&data)),
            Err(ParseError::HookFailed { .. })
        ));
    }

    #[test]
    fn test_zigzag_varint() {
        let cases: [(&[u8], i64); 5] = [
            (&[0x00], 0),
            (&[0x01], -1),
            (&[0x02], 1),
            (&[0x03], -2),
            (&[0xFE, 0x01], 127),
        ];
        for (data, expected) in cases {
            let (val, len) = zigzag_varint(data, ctx(data)).unwrap();
            assert_eq!(val, expected);
            assert_eq!(len, data.len());
        }
    }

    #[test]
    fn test_quic_varint() {
        let data = [0x25];
        assert_eq!(quic_varint(&data, ctx(&data)).unwrap(), (37, 1));
        let data = [0x7B, 0xBD];
        assert_eq!(quic_varint(&data, ctx(&data)).unwrap(), (15293, 2));
        let data = [0x9D, 0x7F, 0x3E, 0x7D];
        assert_eq!(quic_varint(&data, ctx(&data)).unwrap(), (494878333, 4));
        let data = [0xC2, 0x19, 0x7C, 0x5E, 0xFF, 0x14, 0xE8, 0x8C];
        assert_eq!(
            quic_varint(&data, ctx(&data)).unwrap(),
            (151288809941952652, 8)
        );
    }

    #[test]
    fn test_quic_varint_truncated_is_error() {
        assert_eq!(
            quic_varint(&[], ctx(&[])),
            Err(ParseError::NotEnoughData {
                expected: 1,
                got: 0
            })
        );
        let data = [0x7B];
        assert_eq!(
            quic_varint(&data, ctx(&data)),
            Err(ParseError::NotEnoughData {
                expected: 2,
                got: 1
            })
        );
    }
}
