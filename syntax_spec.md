# BinParse DSL Syntax & Implementation Spec

This document defines the DSL syntax and expected Rust code generation corresponding to `instructions.md`. It translates the requirements into a concrete syntax and demonstrates how that syntax maps to the requirements (referenced by R-codes).

---

## 1. Data Layout & Primitives

### 1.1 Basic Primitives & Bit-Fields (R1.1, R1.3)

**DSL Syntax:**
Use `b<N>` for bit-fields and standard `u8`..`u128` for byte-aligned primitives.

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

**DSL Syntax:**
Use `@attributes` for metadata.

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
    // Now we are aligned to byte boundary
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
        // R1.4.1: Alignment check (fail if cursor not at byte boundary)
        // R5.8: Statically known fixed-size (10 bytes)
        if data.len() < 10 { return Err(Error::UnexpectedEof); }
        Ok(Self { data })
    }

    pub fn val_be(&self) -> (u32, usize) {
        // R1.3.1: Unaligned access helper (Inherits Big Endian)
        (u32::from_be_bytes(self.data[0..4].try_into().unwrap()), 0)
    }
    
    pub fn val_le(&self) -> (u32, usize) {
        // R1.2.3: Per-field override (Little Endian)
        (u32::from_le_bytes(self.data[4..8].try_into().unwrap()), 4)
    }
    
    pub fn flags(&self) -> (u8, usize) {
        ((self.data[8] >> 5) & 0x07, 8)
    }
    
    pub fn aligned_val(&self) -> (u8, usize) {
        // R1.4.1: Access aligned field. 
        (self.data[9], 9)
    }
}
```

### 1.3 Unions (R1.6)

**DSL Syntax:**
Tagged unions with deterministic guards. Supports single-field and multi-field (tuple) matching.

```binparse
struct IcmpPacket {
    type: u8,
    code: u8,
    checksum: u16,
    
    // R1.6.1: Deterministic selection based on prior fields
    body: union(type) {
        // All variants use @greedy, so IcmpPacket MUST be called with a length limit (R4.3)
        0 | 8 => Echo { 
            id: u16, 
            seq: u16, 
            payload: @greedy [u8]
        },
        3 => DestUnreach { 
            unused: u32, 
            orig_header: @greedy [u8]
        },
        // R1.6.4 Fallback
        _ => Raw { data: @greedy [u8] },
    }
}

struct TupleUnionExample {
    major: u8,
    minor: u8,
    
    // R1.6.2: Tuple Union (Multi-field selection)
    // Avoids nested unions
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

// R1.6.5: Union variants represented as internal structs and an Enum
pub struct Echo<'a> { data: &'a [u8] }
pub struct DestUnreach<'a> { data: &'a [u8] }
pub struct Raw<'a> { data: &'a [u8] }

pub enum IcmpPacket_body<'a> {
    Echo(Echo<'a>),
    DestUnreach(DestUnreach<'a>),
    Raw(Raw<'a>),
}

impl<'a> IcmpPacket<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        if data.len() < 4 { return Err(Error::UnexpectedEof); }
        // R5.3: Shallow validation - we don't parse the union until accessed
        Ok(Self { data })
    }

    pub fn icmp_type(&self) -> (u8, usize) { (self.data[0], 0) }
    pub fn code(&self) -> (u8, usize) { (self.data[1], 1) }
    pub fn checksum(&self) -> (u16, usize) { (u16::from_be_bytes([self.data[2], self.data[3]]), 2) }

    pub fn body(&self) -> Result<(IcmpPacket_body<'a>, usize, usize), Error> {
        // R1.6.5: Union parsing is lazy (O(1) access to variant data start)
        let variant_data = &self.data[4..];
        let len = variant_data.len();
        match self.icmp_type().0 {
            0 | 8 => Ok((IcmpPacket_body::Echo(Echo::parse(variant_data)?), 4, len)),
            3 => Ok((IcmpPacket_body::DestUnreach(DestUnreach::parse(variant_data)?), 4, len)),
            _ => Ok((IcmpPacket_body::Raw(Raw::parse(variant_data)?), 4, len)),
        }
    }
}

pub struct TupleUnionExample<'a> {
    data: &'a [u8],
}

impl<'a> TupleUnionExample<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        if data.len() < 2 { return Err(Error::UnexpectedEof); }
        Ok(Self { data })
    }

    pub fn major(&self) -> (u8, usize) { (self.data[0], 0) }
    pub fn minor(&self) -> (u8, usize) { (self.data[1], 1) }

    pub fn version_data(&self) -> Result<(VersionData<'a>, usize, usize), Error> {
        // R1.6.2: Multi-field selection (Tuple Union)
        let len = self.data[2..].len();
        match (self.major().0, self.minor().0) {
            (1, 0) => Ok((VersionData::V1_0(V1_0::parse(&self.data[2..])?), 2, len)),
            (1, 1) => Ok((VersionData::V1_1(V1_1::parse(&self.data[2..])?), 2, len)),
            (2, _) => Ok((VersionData::V2_Any(V2_Any::parse(&self.data[2..])?), 2, len)),
            _ => Ok((VersionData::Unknown, 2, len)),
        }
    }
}
```

### 1.4 Bit Literals & Constants (R1.7)

**DSL Syntax:**
Use binary literals `b...` (or `0b...`), hexadecimal literals `x...` (or `0x...`), and decimal literals for values and assignments to define constant fields or compare bit values.

```binparse
struct ConstBitExample {
    // R1.7.1: Constant constraint
    // The parser must verify 'reserved' is exactly 0b000
    reserved = b000, 
    
    // Hex literal example
    magic = xFF,

    // Decimal literal example
    version = 10,

    // R1.7: Bit literal in conditional
    mode: b<3>,
    if (mode == b101) {
        special_param: u8,
    }
}
```

**Expected Rust:**
```rust
pub struct ConstBitExample<'a> {
    data: &'a [u8],
}

impl<'a> ConstBitExample<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        // Minimum size to read constants and 'mode'
        if data.len() < 3 { return Err(Error::UnexpectedEof); }
        
        // R1.7.1: Constant constraint validation
        let reserved = (data[0] >> 5) & 0x07;
        if reserved != 0b000 { return Err(Error::InvalidConst); }
        
        if data[1] != 0xFF { return Err(Error::InvalidConst); } // magic
        if data[2] != 10 { return Err(Error::InvalidConst); }   // version
        
        // mode is b<3> at bits 21-23 of the first 3 bytes? 
        // No, let's assume it follows the bits.
        
        Ok(Self { data })
    }

    pub fn mode(&self) -> (u8, usize) { 
        // Assuming mode follows 'reserved' (b<3>), 'magic' (u8), 'version' (u8)
        // This would be in the 4th byte if those were byte-aligned.
        // But the DSL says reserved: b<3>, then magic = xFF (u8)...
        // The generator handles the bit-offset tracking.
        ((self.data[3] >> 5) & 0x07, 3) 
    }

    pub fn special_param(&self) -> Option<(u8, usize)> {
        // R1.7: Bit literal used in conditional expression
        if self.mode().0 == 0b101 {
            Some((self.data[4], 4)) 
        } else {
            None
        }
    }
}

### 1.5 Concatenated Fields (R1.5)

**DSL Syntax:**
Combine disjoint bit chunks into a single field.

```binparse
struct FragmentedField {
    // R1.5: Field as concatenation of bits
    // bit_field will be u16 (4 + 8 = 12 bits)
    bit_field: concat(
        chunk_a: b<4>, 
        @skip reserved: b<4>, 
        chunk_b: b<8>
    ),
}
```

**Expected Rust:**
```rust
pub struct FragmentedField<'a> {
    data: &'a [u8],
}

impl<'a> FragmentedField<'a> {
    pub fn bit_field(&self) -> (u16, usize) {
        let chunk_a = (self.data[0] >> 4) as u16;
        let chunk_b = self.data[1] as u16;
        ((chunk_a << 8) | chunk_b, 0)
    }
}
```
```


## 2. Variable Length & Dynamic Sizing

### 2.1 Length Prefixes & Expressions (R2.1, R2.2)

**DSL Syntax:**
Arithmetic expressions in array definitions.

```binparse
struct Tlv {
    tag: u8,
    len: u16,
    
    // R2.1: Size determined by previous field
    // R2.2: Expression support
    // R5.6: Requires offset caching in struct
    value: [u8; (len * 2) - 4], 
    
    // R5.6.1: Opt-out caching (recalculated on access)
    // R2.5: Unsafe override for example (usually TLV has no trailer)
    @no_cache
    trailer: @greedy(unsafe_eof) [u8],
}
```

**Expected Rust:**
```rust
pub struct Tlv<'a> {
    data: &'a [u8],
    // R5.6: Boundary cache for O(1) access to 'trailer'
    value_end: usize,
}

impl<'a> Tlv<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        let mut cursor = 0;
        if data.len() < 3 { return Err(Error::UnexpectedEof); }
        
        cursor += 1; // tag: u8
        let len = u16::from_be_bytes(data[cursor..cursor+2].try_into().unwrap());
        cursor += 2;
        
        // R2.2: Expression-based sizing: (len * 2) - 4
        // R7.3: Arithmetic safety check
        let value_len = (len as usize).checked_mul(2)
            .and_then(|x| x.checked_sub(4))
            .ok_or(Error::BadLength)?;
            
        if data.len() < cursor + value_len { return Err(Error::UnexpectedEof); }
        cursor += value_len;
        let value_end = cursor;
        
        Ok(Self { data, value_end })
    }

    pub fn value(&self) -> (&'a [u8], usize, usize) {
        (&self.data[3..self.value_end], 3, self.value_end - 3)
    }

    pub fn trailer(&self) -> (&'a [u8], usize, usize) {
        // R2.5: Greedy field consumes the rest of the buffer
        let data = &self.data[self.value_end..];
        (data, self.value_end, data.len())
    }
}
```

### 2.2 Sentinels & Opaque (R2.4, R2.7)

**DSL Syntax:**
`until` keyword and `@opaque` attribute.

```binparse
struct CString {
    // R2.4: Sentinel terminated
    // R2.4.1: Use optimized memchr
    content: @until(0x00) [u8],
}

struct Container {
    len: u16,
    // R2.7: @opaque (skips 'len' bytes, does not validate Inner contents)
    inner: @opaque [InnerPacket; len],
}
```

**Expected Rust:**
```rust
pub struct CString<'a> {
    data: &'a [u8],
    content_end: usize,
}

impl<'a> CString<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        // R2.4.1: Optimized search (e.g. memchr) for byte sentinel
        let content_end = data.iter().position(|&b| b == 0x00)
            .ok_or(Error::UnexpectedEof)?;
        
        Ok(Self { data, content_end })
    }

    pub fn content(&self) -> (&'a [u8], usize, usize) {
        (&self.data[..self.content_end], 0, self.content_end)
    }
}

pub struct Container<'a> {
    data: &'a [u8],
    inner_end: usize,
}

impl<'a> Container<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        if data.len() < 2 { return Err(Error::UnexpectedEof); }
        let len = u16::from_be_bytes(data[0..2].try_into().unwrap()) as usize;
        
        // R2.7: @opaque skips validation of InnerPacket during parse()
        if data.len() < 2 + len { return Err(Error::UnexpectedEof); }
        let inner_end = 2 + len;
        
        Ok(Self { data, inner_end })
    }

    pub fn inner(&self) -> Result<(InnerPacket<'a>, usize, usize), Error> {
        // R2.7: Actual parsing of InnerPacket happens lazily here
        let data = &self.data[2..self.inner_end];
        Ok((InnerPacket::parse(data)?, 2, data.len()))
    }
}
```

### 2.3 Self-Limiting Scopes (R2.8)

**DSL Syntax:**
Use `@len(field)` on the struct or a specific field to restrict the parsing context.

```binparse
// R2.8.1: Struct-level scope
// The 'total_len' field defines the logical end of the struct's data.
@len(total_len)
struct ScopedPacket {
    total_len: u16,
    
    header: Header,
    
    // R2.5: Greedy now consumes up to `total_len`, not EOF
    // Safe from reading trailing garbage/padding
    payload: @greedy [u8],
}

struct FieldScopePacket {
    sub_len: u16,
    
    // R2.8.2: Field-level scope
    // Restricts the 'inner' field parser to consume at most 'sub_len' bytes.
    // Equivalent to slicing the input before calling Inner::parse.
    @len(sub_len)
    inner: InnerPacket,
}

**Expected Rust:**
```rust
impl<'a> ScopedPacket<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        if data.len() < 2 { return Err(Error::UnexpectedEof); }
        let total_len = u16::from_be_bytes(data[0..2].try_into().unwrap()) as usize;
        
        // R2.8.1: Enforce struct-level scope limit
        if data.len() < total_len {
            return Err(Error::UnexpectedEof);
        }
        let scope_data = &data[..total_len];
        
        // ... parse fields within scope_data ...
        Ok(Self { data: scope_data })
    }
}

pub struct FieldScopePacket<'a> {
    data: &'a [u8],
    inner_len: usize,
}

impl<'a> FieldScopePacket<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        if data.len() < 2 { return Err(Error::UnexpectedEof); }
        let sub_len = u16::from_be_bytes(data[0..2].try_into().unwrap()) as usize;
        
        // R2.8.2: Field-level scope validates availability
        if data.len() < 2 + sub_len { return Err(Error::UnexpectedEof); }
        Ok(Self { data, inner_len: sub_len })
    }

    pub fn inner(&self) -> Result<(InnerPacket<'a>, usize, usize), Error> {
        // R2.8.2: Restricts the 'inner' parser scope by slicing
        let data = &self.data[2..2 + self.inner_len];
        Ok((InnerPacket::parse(data)?, 2, self.inner_len))
    }
}
```

---

## 3. Conditional Presence

### 3.2 Arrays of Structs & Iteration Limits (R3.3, R3.4)

**DSL Syntax:**
Arrays of complex types with count-based sizing and BPF-friendly iteration limits.

```binparse
struct Record {
    id: u32,
    value: u32,
}

struct Table {
    count: u16,
    
    // R3.3.1: Count-based array of structs
    // R3.3.2: Max iterations for BPF safety
    // R3.4: Allocation-free iterator
    @max_iter(1024)
    records: [Record; count],
}
```

**Expected Rust:**
```rust
pub struct Table<'a> {
    data: &'a [u8],
    count: usize,
}

impl<'a> Table<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        if data.len() < 2 { return Err(Error::UnexpectedEof); }
        let count = u16::from_be_bytes(data[0..2].try_into().unwrap()) as usize;
        
        // R3.3.2: Optional validation of @max_iter at runtime/compile-time
        if count > 1024 { return Err(Error::BadLength); }
        
        // R3.4: Validate total size for allocation-free iteration
        let total_size = count * 8; // Record is 8 bytes
        if data.len() < 2 + total_size { return Err(Error::UnexpectedEof); }
        
        Ok(Self { data, count })
    }

    pub fn records(&self) -> (impl Iterator<Item = Record<'a>> + 'a, usize, usize) {
        // R3.4: Zero-copy iterator over the records
        let iter = (0..self.count).map(move |i| {
            let start = 2 + (i * 8);
            Record::parse(&self.data[start..start + 8]).unwrap()
        });
        (iter, 2, self.count * 8)
    }
}
```

---

## 4. Composition & Encapsulation

### 4.1 Nested Protocols & Context (R4.3)

**DSL Syntax:**
Passing arguments to nested structs.

```binparse
struct Parent {
    total_len: u16,
    // R4.3.1: Context propagation (limit child to total_len)
    @len(total_len)
    child: Child,
}


// R2.5: Since this struct has a @greedy field
struct Child {
    id: u8,
    // R2.5: Greedy consumes only up to 'limit'
    payload: @greedy(unsafe_eof) [u8], 
}
```

**Expected Rust:**
```rust
pub struct Parent<'a> {
    data: &'a [u8],
    total_len: usize,
}

impl<'a> Parent<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        if data.len() < 2 { return Err(Error::UnexpectedEof); }
        let total_len = u16::from_be_bytes(data[0..2].try_into().unwrap()) as usize;
        
        // R4.3.1: Context-defined limit validation
        if data.len() < total_len { return Err(Error::UnexpectedEof); }
        Ok(Self { data, total_len })
    }

    pub fn child(&self) -> Result<(Child<'a>, usize, usize), Error> {
        // R4.3.2: Mandatory Scope Propagation (limits Child to Parent's total_len)
        let data = &self.data[2..self.total_len];
        Ok((Child::parse(data)?, 2, data.len()))
    }
}

pub struct Child<'a> {
    data: &'a [u8],
}

impl<'a> Child<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        if data.is_empty() { return Err(Error::UnexpectedEof); }
        Ok(Self { data })
    }

    pub fn payload(&self) -> (&'a [u8], usize, usize) {
        // R2.5: Greedy field consumes the remainder of its provided slice
        let data = &self.data[1..];
        (data, 1, data.len())
    }
}

### 4.2 Cross-Layer Constraints (R4.5)

**DSL Syntax:**
Use `@check` to define invariants that span across protocol layers.

```binparse
struct Outer {
    inner_len: u16,
    
    @len(inner_len)
    inner: Inner,
    
    // R4.5: Assert consistency between Outer and Inner
    @check(inner_len == inner.total_length)
}
```

**Expected Rust:**
```rust
impl<'a> Outer<'a> {
    pub fn inner(&self) -> Result<(Inner<'a>, usize, usize), Error> {
        let len = self.inner_len().0 as usize;
        let data = &self.data[2..2 + len];
        let inner = Inner::parse(data)?;
        
        // R4.5: Enforcement of cross-layer constraints during lazy access
        if (self.inner_len().0 as u32) != inner.total_length().0 {
            return Err(Error::ChecksumMismatch); // Or appropriate custom error
        }
        
        Ok((inner, 2, len))
    }
}
```
```

---

## 5. Performance & Memory Model

Refer to `instructions.md` for R5 requirements. The implementation ensures zero-copy parsing and lazy evaluation.

---

## 6. Developer Interface & Hooks

### 6.1 Custom Hooks (R6.1)

**DSL Syntax:**

```binparse
struct SecureData {
    // R6.1: Custom transformation (Decryption)
    iv: @transform(fn("crate::aes_decrypt"), usize) [u8; 16],

    // R2.3: VarInt via custom parser
    @parse_with(fn("crate::varint::parse"), u64)
    length: @greedy(unsafe_eof) [u8],
}
```

**Expected Rust:**
```rust
pub struct SecureData<'a> {
    data: &'a [u8],
    length: u64, // Result of custom VarInt parse
    length_len: usize,
}

impl<'a> SecureData<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        if data.len() < 16 { return Err(Error::UnexpectedEof); }
        
        // R2.3, R6.1: VarInt via custom parser hook
        // R6.1.1: Hooks must report consumed length and handle errors
        let (length, consumed) = crate::varint::parse(&data[16..])
            .map_err(|_| Error::BadLength)?;
            
        Ok(Self { data, length, length_len: consumed })
    }

    pub fn iv(&self) -> ([u8; 16], usize) {
        // R6.1: Custom transformation (Decryption) applied on access
        (crate::aes_decrypt(self.data[0..16].try_into().unwrap()), 0)
    }

    pub fn length(&self) -> (u64, usize, usize) {
        (self.length, 16, self.length_len)
    }
}
```

---

## 7. Safety

**Expected Rust Behavior:**
- **R7.1 Malformed Input:** All array indexing uses `.get()` or checked slicing. Returns `Result::Err`.
- **R7.3 Arithmetic:** Uses `.checked_add`, `.checked_mul`.
- **R5.1 No Alloc:** Structs contain only `usize` and `&[u8]`.

## 8. Custom Error Types (R8)

### 8.1 Error Definition

**DSL Syntax:**
A single `error` block defines the custom error variants available to the parser.

```binparse
error {
    // R8.1: Custom variants with primitive fields
    // R8.2: Bit-fields map to smallest enclosing primitive (e.g. b<3> -> u8)
    MISSING_THIS_FLAG { found: b<3>, expected: b<3> },
    
    // Primitive integer types allowed
    INVALID_VERSION { val: u8 },
    
    // Unit variants allowed
    CHECKSUM_MISMATCH,
}

struct Packet {
    type: b<3>,
    variant: union(type) {
        b010 => Something {},
        _ => @error(MISSING_THIS_FLAG { found: type, expected: b010 })
    }
}
```

**Expected Rust:**
```rust
#[derive(Debug)]
pub enum Error {
    // R8.3: Automatic IO error wrapping
    Io(std::io::Error),
    
    // Standard parsing errors
    UnexpectedEof,
    BadLength,
    
    // Generated custom variants
    MissingThisFlag { found: u8, expected: u8 },
    InvalidVersion { val: u8 },
    ChecksumMismatch,
}

// Standard conversion for R8.3
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl<'a> Packet<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        if data.is_empty() { return Err(Error::UnexpectedEof); }
        
        let packet_type = (data[0] >> 5) & 0x07;
        match packet_type {
            0b010 => {
                // Success case
                Ok(Self { data })
            }
            _ => {
                // R8.1: Returning a custom error variant defined in the DSL
                Err(Error::MissingThisFlag { 
                    found: packet_type, 
                    expected: 0b010 
                })
            }
        }
    }
}
```

---

## 9. Edge Cases

Refer to `instructions.md` for R9 requirements (Memory model, Bit-ordering, Recursion, etc.).
