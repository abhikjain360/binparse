# Binparse DSL Roadmap

## Why This Over AI-Generated Code?

The goal is to provide **guarantees AI cannot**, **automation AI cannot maintain**, and **consistency across protocol families**.

---

## Core Parsing Features

### Hooks (Rust Integration)

- [x] `@hook(fn_name, ReturnTypePathWithAllRequiredGenerics)` attribute for custom parsing
  - [x] Hook returns `(value in correct type, bytes consumed)` tuple
  - [x] Hook has access to remaining slice
  - [x] Propagate dynamic length to subsequent fields
- [x] Built-in hooks library
  - [x] `cstring` - null-terminated string
  - [x] `leb128` - signed/unsigned LEB128

### Array Improvements

- [ ] Consume-rest arrays: `[u8; ..]` - take remaining bytes
- [ ] Sentinel-terminated arrays: `[u8; until 0x00]`
- [ ] Arrays with expression-based size: `[u8; header.len - 4]`
- [ ] Dynamic start offset support (currently `todo!()` in array.rs:224)

### Validation

- [ ] `@validate(expr)` attribute for field constraints
- [ ] `@range(min, max)` for numeric bounds
- [ ] Return `ParseError::ValidationFailed` on violation
- [ ] Magic number validation: `magic = 0x89504E47` (PNG signature)

### Alignment & Padding

- [ ] `@align(N)` - ensure field starts at N-byte boundary
- [ ] `@pad(N)` - skip N bytes
- [ ] `@pad_to(N)` - pad until offset is multiple of N

---

## Guarantees AI Cannot Provide

### No-Panic Guarantee

- [ ] All slice accesses bounds-checked at parse time
- [ ] No `.unwrap()` in generated code (use `?` propagation)
- [ ] Prove via `#[cfg(test)]` + `panic = "abort"` + fuzzing
- [ ] Document which error conditions are possible

### Compile-Time Verification

- [ ] Static offset computation when all fields are fixed-size
- [ ] Detect overlapping fields at codegen time
- [ ] Warn on suspicious patterns (e.g., length field after variable data)

### Fuzzing Integration

- [ ] Generate `Arbitrary` impl for each struct
- [ ] Generate `proptest` strategies
- [ ] Roundtrip property: `parse(serialize(x)) == x`
- [ ] Crash-resistance property: any `&[u8]` either parses or returns error

---

## Automation AI Cannot Maintain

### Serialization (Write Path)

- [ ] Generate `fn serialize(&self, buf: &mut Vec<u8>)`
- [ ] Or zerocopy writer: `fn write_to(&self, buf: &mut [u8]) -> usize`
- [ ] Endianness-aware writing
- [ ] Same hooks work for both parse and serialize

### Cross-Language Generation

- [ ] C header generation (`struct __attribute__((packed))`)
- [ ] Python `struct.unpack` generation
- [ ] TypeScript `DataView` generation
- [ ] Language-specific hook implementations

### Test Generation

- [ ] Generate valid packet examples from spec
- [ ] Generate edge cases (max lengths, boundary values)
- [ ] Generate invalid packets (truncated, bad magic, overflow)
- [ ] Snapshot tests for generated code

### Documentation Generation

- [ ] Markdown protocol documentation from DSL
- [ ] Field offset tables
- [ ] Packet diagrams (ASCII or SVG)
- [ ] Cross-reference between struct types

---

## Protocol Evolution Support

### Versioning

- [ ] `@since(version)` - field only present in version >= X
- [ ] `@deprecated(version)` - field removed in version >= X
- [ ] `@renamed(old_name, version)` - track renames
- [ ] Version-parameterized structs

### Diffing

- [ ] Compare two DSL specs, output changes
- [ ] Detect breaking changes (field removed, type changed)
- [ ] Detect compatible changes (field added at end)
- [ ] Migration guide generation

---

## Developer Experience

### Error Messages

- [ ] Source location in parse errors (field name, byte offset)
- [ ] "Expected X bytes for field `foo`, got Y"
- [ ] Hex dump of problematic region
- [ ] Suggest fixes for common mistakes

### IDE Support

- [ ] Tree-sitter grammar for syntax highlighting
- [ ] LSP server for go-to-definition, hover info
- [ ] Diagnostics (unknown types, invalid attributes)
- [ ] Auto-complete for attribute names

### Debug Visualization

- [ ] `#[derive(Debug)]` with meaningful output
- [ ] Hex view with field annotations
- [ ] Optional `Display` impl for human-readable output

---

## Performance

### Compile-Time Optimization

- [ ] Inline small field accessors
- [ ] Const-evaluate fixed offsets
- [ ] Avoid redundant bounds checks

### Runtime Optimization

- [ ] SIMD for array parsing where applicable
- [ ] Batch validation (check total length once upfront)
- [ ] Zero-allocation iterators (already have this)

---

## Immediate Priorities

| Priority | Feature               | Rationale                         |
| -------- | --------------------- | --------------------------------- |
| P0       | Hooks for VarInt      | Unlocks MQTT, Protobuf, WebSocket |
| P0       | Consume-rest arrays   | Unlocks variable payloads         |
| P1       | Serialization         | Symmetry; proves spec is complete |
| P1       | No-panic guarantee    | Key differentiator from AI        |
| P2       | Fuzzing integration   | Proves correctness at scale       |
| P2       | Validation attributes | Catches bad data early            |
| P3       | Cross-language        | Multiplies value of each spec     |
| P3       | Documentation gen     | Specs become source of truth      |
