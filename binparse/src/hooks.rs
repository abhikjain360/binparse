pub fn cstring(data: &[u8]) -> (String, usize) {
    let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
    let s = String::from_utf8_lossy(&data[..end]).to_string();
    let consumed = if end < data.len() { end + 1 } else { end };
    (s, consumed)
}

pub fn leb128_unsigned(data: &[u8]) -> (u64, usize) {
    let mut result: u64 = 0;
    let mut shift = 0;
    let mut consumed = 0;

    for &byte in data {
        consumed += 1;
        result |= ((byte & 0x7F) as u64) << shift;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
    }

    (result, consumed)
}

pub fn leb128_signed(data: &[u8]) -> (i64, usize) {
    let mut result: i64 = 0;
    let mut shift = 0;
    let mut consumed = 0;
    let mut byte = 0u8;

    for &b in data {
        byte = b;
        consumed += 1;
        result |= ((byte & 0x7F) as i64) << shift;
        shift += 7;
        if byte & 0x80 == 0 {
            break;
        }
    }

    if shift < 64 && (byte & 0x40) != 0 {
        result |= !0i64 << shift;
    }

    (result, consumed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cstring_basic() {
        let data = b"hello\0world";
        let (s, len) = cstring(data);
        assert_eq!(s, "hello");
        assert_eq!(len, 6);
    }

    #[test]
    fn test_cstring_no_null() {
        let data = b"hello";
        let (s, len) = cstring(data);
        assert_eq!(s, "hello");
        assert_eq!(len, 5);
    }

    #[test]
    fn test_cstring_empty() {
        let data = b"\0rest";
        let (s, len) = cstring(data);
        assert_eq!(s, "");
        assert_eq!(len, 1);
    }

    #[test]
    fn test_leb128_unsigned_single_byte() {
        let data = [0x7F];
        let (val, len) = leb128_unsigned(&data);
        assert_eq!(val, 127);
        assert_eq!(len, 1);
    }

    #[test]
    fn test_leb128_unsigned_multi_byte() {
        let data = [0xE5, 0x8E, 0x26];
        let (val, len) = leb128_unsigned(&data);
        assert_eq!(val, 624485);
        assert_eq!(len, 3);
    }

    #[test]
    fn test_leb128_unsigned_zero() {
        let data = [0x00];
        let (val, len) = leb128_unsigned(&data);
        assert_eq!(val, 0);
        assert_eq!(len, 1);
    }

    #[test]
    fn test_leb128_signed_positive() {
        let data = [0x7F];
        let (val, len) = leb128_signed(&data);
        assert_eq!(val, -1);
        assert_eq!(len, 1);
    }

    #[test]
    fn test_leb128_signed_negative() {
        let data = [0xC0, 0xBB, 0x78];
        let (val, len) = leb128_signed(&data);
        assert_eq!(val, -123456);
        assert_eq!(len, 3);
    }

    #[test]
    fn test_leb128_signed_zero() {
        let data = [0x00];
        let (val, len) = leb128_signed(&data);
        assert_eq!(val, 0);
        assert_eq!(len, 1);
    }

    #[test]
    fn test_leb128_signed_positive_multi() {
        let data = [0x80 | 57, 0x00];
        let (val, len) = leb128_signed(&data);
        assert_eq!(val, 57);
        assert_eq!(len, 2);
    }
}
