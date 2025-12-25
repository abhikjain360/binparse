# BinParse DSL Specification

This document combines the requirements and syntax specifications for the BinParse DSL, a declarative language for generating efficient, zero-copy Rust parsers for network packets and binary formats.

---

## 1. Data Layout & Primitives

### 1.1 Bit-Level Granularity & Primitives (R1.1, R1.3, R1.4)

**Requirements:**
- **R1.1 Bit-Level Granularity:** The DSL must support defining bit-slice fields with arbitrary bit-widths (e.g., 1-bit flags, 3-bit reserved fields, 20-bit flow labels).
    - Bit-slice fields have type `b<N>` where `N` is the number of bits.
    - Max `N` for a single bit-field is 128.
    - Bit-slice getters return the smallest unsigned integer primitive that can hold `N` bits (e.g., `b<1>`-`b<8>` return `u8`, `b<9>`-`b<16>` return `u16`, etc.).
    - Signed bit-fields are not supported.
    - **R1.1.1 Bit-ordering Across Bytes:** Bit numbering within a byte follows the specified endianness/bit-order (R1.2), and multi-byte bitfields are reconstructed by concatenating bits in the order they appear in the stream.
- **R1.3 Primitive Type Support:** The DSL must map to unsigned integer primitives (`u8` through `u128`) and fixed-size byte arrays (`[u8; 6]`).
    - There is no `bool` type; use `b<1>` for single-bit flags.
    - Signed integer primitives are not supported in the DSL.
    - **R1.3.1 Unaligned Access:** The generated code must handle unaligned memory access safely and efficiently (e.g., using `read_unaligned` or `from_be_bytes`).
- **R1.4 Padding & Alignment:** The DSL must support declarative "skip" or "reserved" fields that advance the cursor without exposing a value.
    - **R1.4.1 Alignment Constraints:** The DSL must allow requiring byte-alignment for a field. If a non-byte-aligned cursor attempts to start a byte-aligned field, parsing must fail.
    - **R1.4.2 Cursor Units:** The internal layout cursor is tracked in **bits**.
    - **R1.4.3 Bitfield Grouping Rule:** A sequence of `b<N>` fields representing a byte must be explicitly padded/skipped so that the cursor returns to a byte boundary before any subsequent non-`b<N>` field. Violations are DSL-level errors.

**DSL Syntax:**
```binparse
struct TcpFlags {
    // R1.1: Bit-fields
    // R1.4.2: Cursor tracked in bits
    data_offset: b<4>,
    reserved: b<3>,  
    nonce: b<1>,
    cwr: b<1>,
    ecn: b<1>,
    urg: b<1>,
    ack: b<1>,
    psh: b<1>,
    rst: b<1>,
    syn: b<1>,
    fin: b<1>,
    
    // R1.1.1: Bit-ordering (bits concatenated in stream order)
    // R1.4.3: Bitfield grouping rule (this starts at bit 12, must align if next is byte)
    window_size: b<16>, 
}
```

**Expected Rust:**
```rust
pub struct TcpFlags<'a> {
    data: &'a [u8], // R5.1 Reference-only storage
}

impl<'a> TcpFlags<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        // R5.8: Constant size optimization (32 bits = 4 bytes)
        if data.len() < 4 { return Err(Error::UnexpectedEof); }
        Ok(Self { data })
    }

    pub fn data_offset(&self) -> (u8, usize) { ((self.data[0] >> 4) & 0x0F, 0) }
    pub fn reserved(&self) -> (u8, usize) { ((self.data[0] >> 1) & 0x07, 0) }
    pub fn nonce(&self) -> (u8, usize) { (self.data[0] & 0x01, 0) }
    // ... other 1-bit flags ...
    pub fn fin(&self) -> (u8, usize) { (self.data[1] & 0x01, 1) }
    
    pub fn window_size(&self) -> (u16, usize) {
        // R1.3.1: Unaligned-safe reading (Big Endian)
        (u16::from_be_bytes([self.data[2], self.data[3]]), 2)
    }
}
```

### 1.2 Endianness & Alignment (R1.2, R1.3.1, R1.4)

**Requirements:**
- **R1.2 Explicit Endianness:** Specify endianness at three levels:
    - **R1.2.1 Global default:** (Default to Big Endian).
    - **R1.2.2 Per-struct override.**
    - **R1.2.3 Per-field override.**
Bit-level ordering (LSB 0 vs MSB 0) must also be definable, defaulting to MSB 0.

**DSL Syntax:**
```binparse
@endian(big) // R1.2 Global default
struct EndianExample {
    val_be: u32,       // Inherits big
    val_le: @endian(little) u32,   // R1.2 Per-field override
    
    // R1.2: Explicit bit-ordering (default is msb)
    @bit_order(lsb)
    lsb_flags: b<8>,

    // ... requires 5 bits of padding to reach next byte (3 + 5 = 8 bits)
    @skip pad: b<5>, 
    
    // R1.4.1: Alignment check (fail if cursor not at byte boundary)
    @align(1)
    aligned_val: u8,
}
```

**Expected Rust:**
```rust
pub struct EndianExample<'a> {
    data: &'a [u8],
}

impl<'a> EndianExample<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        // R1.4.1: Alignment check
        // R5.8: Statically known fixed-size (10 bytes)
        if data.len() < 10 { return Err(Error::UnexpectedEof); }
        Ok(Self { data })
    }

    pub fn val_be(&self) -> (u32, usize) {
        // R1.3.1: Unaligned access helper
        (u32::from_be_bytes(self.data[0..4].try_into().unwrap()), 0)
    }
    
    pub fn val_le(&self) -> (u32, usize) {
        // R1.2.3: Per-field override
        (u32::from_le_bytes(self.data[4..8].try_into().unwrap()), 4)
    }
    
    pub fn flags(&self) -> (u8, usize) {
        ((self.data[8] >> 5) & 0x07, 8)
    }
    
    pub fn aligned_val(&self) -> (u8, usize) {
        (self.data[9], 9)
    }
}
```

### 1.3 Unions & Choices (R1.6)

**Requirements:**
- **R1.6 Unions / Choices:** Define choices between layouts at the same cursor position.
    - **R1.6.1 Selection Rule:** Must be deterministic based on prior fields.
    - **R1.6.2 Multi-Field Selection (Tuple Unions):** Support matching on a tuple of fields.
    - **R1.6.3 No Lookahead:** Discriminant determined solely by fields parsed *prior* to the union.
    - **R1.6.4 Fallback Rule:** Must provide `_` fallback if variants aren't exhaustive.
    - **R1.6.5 Cursor Advancement:** Cursor advances by the selected variant's size. Participation in boundary caching (R5.6) if dynamic.
    - **R1.6.6 Validation Rule:** Variant matches only if it passes its own bounds and semantic checks.

**DSL Syntax:**
```binparse
struct IcmpPacket {
    type: u8,
    code: u8,
    checksum: u16,
    
    body: union(type) {
        0 | 8 => Echo { 
            id: u16, 
            seq: u16, 
            payload: @greedy [u8]
        },
        3 => DestUnreach { 
            unused: u32, 
            orig_header: @greedy [u8]
        },
        _ => Raw { data: @greedy [u8] },
    }
}

struct TupleUnionExample {
    major: u8,
    minor: u8,
    
    version_data: union(major, minor) {
        (1, 0) => V1_0 { ... },
        (1, 1) => V1_1 { ... },
        (2, _) => V2_Any { ... },
        _ => Unknown,
    }
}
```

**Expected Rust:**
```rust
pub struct IcmpPacket<'a> {
    data: &'a [u8],
}

pub enum IcmpPacket_body<'a> {
    Echo(Echo<'a>),
    DestUnreach(DestUnreach<'a>),
    Raw(Raw<'a>),
}

impl<'a> IcmpPacket<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        if data.len() < 4 { return Err(Error::UnexpectedEof); }
        Ok(Self { data })
    }

    pub fn body(&self) -> Result<(IcmpPacket_body<'a>, usize, usize), Error> {
        let variant_data = &self.data[4..];
        let len = variant_data.len();
        match self.icmp_type().0 {
            0 | 8 => Ok((IcmpPacket_body::Echo(Echo::parse(variant_data)?), 4, len)),
            3 => Ok((IcmpPacket_body::DestUnreach(DestUnreach::parse(variant_data)?), 4, len)),
            _ => Ok((IcmpPacket_body::Raw(Raw::parse(variant_data)?), 4, len)),
        }
    }
}
```

### 1.4 Bit Literals & Constants (R1.7)

**Requirements:**
- **R1.7 Bit Literals & Constants:** Support binary (`b110`), hex (`xFF`), and decimal literals.
    - **R1.7.1 Constant Constraints:** Validate input against constant (e.g., `magic: b<4> = b1010`).
    - **R1.7.2 Width Validation:** Compile-time check that literal fits `b<N>`.

**DSL Syntax:**
```binparse
struct ConstBitExample {
    reserved = b000, 
    magic = xFF,
    version = 10,
    mode: b<3>,
    if (mode == b101) {
        special_param: u8,
    }
}
```

**Expected Rust:**
```rust
impl<'a> ConstBitExample<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        if data.len() < 3 { return Err(Error::UnexpectedEof); }
        if (data[0] >> 5) & 0x07 != 0b000 { return Err(Error::InvalidConst); }
        if data[1] != 0xFF { return Err(Error::InvalidConst); }
        if data[2] != 10 { return Err(Error::InvalidConst); }
        Ok(Self { data })
    }
}
```

### 1.5 Concatenated Fields (R1.5)

**Requirements:**
- **R1.5 Field as Concatenation:** Combine disjoint bits (e.g., `concat(chunk_a: b<4>, skip<b<4>>, chunk_b: b<8>)`).

**DSL Syntax:**
```binparse
struct FragmentedField {
    bit_field: concat(
        chunk_a: b<4>, 
        @skip reserved: b<4>, 
        chunk_b: b<8>
    ),
}
```

**Expected Rust:**
```rust
impl<'a> FragmentedField<'a> {
    pub fn bit_field(&self) -> (u16, usize) {
        let chunk_a = (self.data[0] >> 4) as u16;
        let chunk_b = self.data[1] as u16;
        ((chunk_a << 8) | chunk_b, 0)
    }
}
```

---

## 2. Variable Length & Dynamic Sizing

### 2.1 Length Prefixes & Expressions (R2.1, R2.2, R2.3)

**Requirements:**
- **R2.1 Length-Prefixed Fields:** Size determined by a previous integer field.
- **R2.2 Expression-Based Sizing:** Arithmetic expressions (e.g., `(len * 4) - 20`) evaluated in declaration order.
- **R2.3 Self-Describing Encodings (VarInts):** Support hooks (`@parse_with`) for data-dependent sizing.

**DSL Syntax:**
```binparse
struct Tlv {
    tag: u8,
    len: u16,
    value: [u8; (len * 2) - 4], 
    
    @no_cache
    trailer: @greedy(unsafe_eof) [u8],
}
```

**Expected Rust:**
```rust
impl<'a> Tlv<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        let mut cursor = 0;
        if data.len() < 3 { return Err(Error::UnexpectedEof); }
        let len = u16::from_be_bytes(data[1..3].try_into().unwrap());
        let value_len = (len as usize).checked_mul(2).and_then(|x| x.checked_sub(4)).ok_or(Error::BadLength)?;
        if data.len() < 3 + value_len { return Err(Error::UnexpectedEof); }
        Ok(Self { data, value_end: 3 + value_len })
    }
}
```

### 2.2 Sentinels & Opaque (R2.4, R2.7)

**Requirements:**
- **R2.4 Sentinel-Terminated Fields:** Read until byte value (e.g., `0x00`).
    - **R2.4.1 Optimization:** Use `memchr` or SIMD for byte sentinels.
- **R2.7 Opaque Fields:** Calculate size and skip validation of `T` until accessed.

**DSL Syntax:**
```binparse
struct CString {
    content: @until(0x00) [u8],
}

struct Container {
    len: u16,
    @opaque inner: @opaque [InnerPacket; len],
}
```

**Expected Rust:**
```rust
impl<'a> CString<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        let content_end = data.iter().position(|&b| b == 0x00).ok_or(Error::UnexpectedEof)?;
        Ok(Self { data, content_end })
    }
}
```

### 2.3 Greedy Fields & Scopes (R2.5, R2.8)

**Requirements:**
- **R2.5 End-of-Input Consumers:** `@greedy` consumes remaining buffer. Must be last. Requires an established scope (R2.8/R4.3) unless `@greedy(unsafe_eof)` is used.
- **R2.8 Self-Limiting Scopes:** Define logical end via `@len`.
    - **R2.8.1 Struct-level:** Restricts all fields.
    - **R2.8.2 Field-level:** Restricts specific nested field.

**DSL Syntax:**
```binparse
@len(total_len)
struct ScopedPacket {
    total_len: u16,
    header: Header,
    payload: @greedy [u8],
}
```

---

## 3. Conditional Presence & Arrays

### 3.1 Conditionals (R3.1, R3.2)

**Requirements:**
- **R3.1/R3.2 Dependency:** Fields can depend on bit-flags or integer values of prior fields.

**DSL Syntax:**
```binparse
struct ConditionalExample {
    flags: u8,
    if (flags & 0x01 != 0) {
        optional_field: u32,
    }
}
```

### 3.2 Arrays (R3.3, R3.4)

**Requirements:**
- **R3.3 Repeated Fields:** `[T; N]` where `N` is constant or dynamic.
    - **R3.3.2 Max iterations (BPF):** Require `max_iteration` for BPF safety.
- **R3.4 Allocation-Free Iterators:** Arrays exposed as zero-copy views/iterators.

**DSL Syntax:**
```binparse
struct Table {
    count: u16,
    @max_iter(1024)
    records: [Record; count],
}
```

---

## 4. Composition & Encapsulation

**Requirements:**
- **R4.1 Protocol Graph:** Transitions between protocols via tagged choices.
- **R4.2 Offset Inheritance:** Nested protocol base offsets calculated relative to parent.
- **R4.3 Context Propagation:** Pass `total_length` or `limit` to child parsers.
- **R4.5 Cross-Layer Constraints:** Define consistency checks (e.g., `Parent.len == Child.len`) enforced during lazy access.

---

## 5. Performance & Memory Model

- **R5.1 Reference-Only:** Structs hold `&[u8]` and `usize`. No heap allocation.
- **R5.2 Lazy Evaluation:** Masking/swapping happens in getters, not constructor.
- **R5.3 Shallow Validation:** Constructor only scans the *current* layer.
- **R5.6 Offset Caching:** Store boundaries for dynamic fields to keep getters O(1).
- **R5.8 Constant Size Optimization:** Bypass scanning for fixed-size structs.

---

## 6. Hooks & Extensibility

- **R6.1 Pure Rust Hooks:**
    - `@transform`: For decryption/decompression.
    - `@parse_with`: For custom VarInt/complex parsers.

---

## 7. Safety

- **R7.1 Malformed Input:** No panics; return `Error::UnexpectedEof` or `Error::BadLength`.
- **R7.3 Arithmetic Safety:** Use `.checked_add`, `.checked_mul`.
- **R9.6.1 Panic-Free Access:** Use `.get()` for BPF/no_std.

---

## 8. Custom Error Types (R8)

**DSL Syntax:**
```binparse
error {
    MISSING_THIS_FLAG { found: b<3>, expected: b<3> },
    CHECKSUM_MISMATCH,
}
```

**Expected Rust:**
```rust
pub enum Error {
    Io(std::io::Error),
    UnexpectedEof,
    MissingThisFlag { found: u8, expected: u8 },
    ChecksumMismatch,
}
```

---

## 9. Edge Cases

- **R9.1 Linear Buffers Only:** No support for fragmented `iovec`.
- **R9.3 Recursive Protocols:** Flattened execution via lazy evaluation. Manual layering for IP-in-IP.

