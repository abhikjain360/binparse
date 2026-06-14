//! Protocol-specific consuming hooks referenced from the DSL specs.

use binparse::{HookContext, ParseError, ParseResult};

/// A decoded DNS name as its raw labels (each label's bytes, no separator).
/// Labels are opaque octets on the wire (not guaranteed ASCII/UTF-8), so they
/// are returned verbatim — callers interpret them as they see fit.
pub type DnsLabels = Vec<Vec<u8>>;

/// Decode a DNS name (RFC 1035 §4.1.4), following compression pointers. The
/// returned `usize` is the number of bytes consumed from the field's start (a
/// pointer terminates consumption at the pointer itself); pointers jump within
/// the enclosing message via `ctx.enclosing`.
pub fn dns_name(_data: &[u8], ctx: HookContext<'_>) -> ParseResult<(DnsLabels, usize)> {
    let msg = ctx.enclosing;
    let mut labels: DnsLabels = Vec::new();
    let mut pos = ctx.offset;
    let mut consumed = None;
    let mut jumps = 0;
    loop {
        let len_byte = *msg.get(pos).ok_or(ParseError::NotEnoughData {
            expected: pos + 1,
            got: msg.len(),
        })?;
        if len_byte & 0xC0 == 0xC0 {
            let second = *msg.get(pos + 1).ok_or(ParseError::NotEnoughData {
                expected: pos + 2,
                got: msg.len(),
            })?;
            if consumed.is_none() {
                consumed = Some(pos + 2 - ctx.offset);
            }
            jumps += 1;
            if jumps > 8 {
                return Err(ParseError::HookFailed {
                    field: ctx.field,
                    reason: "too many DNS compression jumps",
                });
            }
            pos = (usize::from(len_byte & 0x3F) << 8) | usize::from(second);
        } else if len_byte == 0 {
            let consumed = consumed.unwrap_or_else(|| pos + 1 - ctx.offset);
            return Ok((labels, consumed));
        } else {
            let end = pos + 1 + usize::from(len_byte);
            let label = msg.get(pos + 1..end).ok_or(ParseError::NotEnoughData {
                expected: end,
                got: msg.len(),
            })?;
            labels.push(label.to_vec());
            pos = end;
        }
    }
}
