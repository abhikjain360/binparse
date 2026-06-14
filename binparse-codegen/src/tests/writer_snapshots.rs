use super::*;

fn generate_writers_err(dsl: &str) -> Error {
    let ast = binparse_dsl_parse::parse_str(dsl).expect("failed to parse DSL");
    CodeGen::generate_writers(&ast).expect_err("expected writer codegen to fail")
}

fn assert_generated_writers_eq(dsl: &str, snapshot: &str) {
    let actual = normalized_items(&generate_writers(dsl)).join("\n");
    let path = snapshot_path(snapshot);
    if std::env::var_os("BLESS").is_some() {
        std::fs::write(&path, format!("{actual}\n"))
            .unwrap_or_else(|e| panic!("failed to write snapshot {snapshot}: {e}"));
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!("missing snapshot {snapshot}; rerun with BLESS=1 to regenerate")
    });
    let expected = expected.strip_suffix('\n').unwrap_or(&expected);
    if actual != expected {
        panic!(
            "generated writer code does not match snapshot {snapshot}; rerun with BLESS=1 to regenerate\n\n--- expected ---\n{expected}\n--- actual ---\n{actual}"
        );
    }
}

#[test]
fn writer_fixed_primitives() {
    assert_generated_writers_eq(
        r#"@endian(big) struct Mixed { a: u8, @endian(little) b: u16, c: u32 }"#,
        "writer_fixed_primitives",
    );
}

#[test]
fn writer_bitfields() {
    assert_generated_writers_eq(
        r#"struct Ip { version: b<4>, ihl: b<4>, ttl: u8, total: u16 }"#,
        "writer_bitfields",
    );
}

#[test]
fn writer_byte_array() {
    assert_generated_writers_eq(
        r#"struct Eth { dst: [u8; 6], src: [u8; 6], ethertype: u16 }"#,
        "writer_byte_array",
    );
}

#[test]
fn writer_struct_ref() {
    assert_generated_writers_eq(
        r#"@endian(big) struct Inner { a: u8, b: u16 } @endian(big) struct Outer { tag: u8, inner: Inner, trailer: u8 }"#,
        "writer_struct_ref",
    );
}

#[test]
fn writer_dynamic_tail() {
    assert_generated_writers_eq(
        r#"struct Frame { kind: u8, len: u8, payload: [u8; len] }"#,
        "writer_dynamic_tail",
    );
}

#[test]
fn writer_affine_dynamic_tail() {
    assert_generated_writers_eq(
        r#"@endian(big) struct Udp { src: u16, dst: u16, length: u16, checksum: u16, payload: [u8; length - 8] }"#,
        "writer_affine_dynamic_tail",
    );
}

#[test]
fn writer_dynamic_region_with_trailer() {
    assert_generated_writers_eq(
        r#"@endian(big) struct Frame { kind: u8, len: u8, payload: [u8; len], crc: u16, tail: u8 }"#,
        "writer_dynamic_region_with_trailer",
    );
}

#[test]
fn writer_varint_hook_tail() {
    assert_generated_writers_eq(
        r#"struct VarFrame {
            tag: u8,
            @hook(binparse.hooks.leb128_unsigned, u64) @write_hook(binparse.hooks.write_leb128_unsigned, binparse.hooks.leb128_unsigned_len) len: [u8],
            body: [u8; len],
        }"#,
        "writer_varint_hook_tail",
    );
}

#[test]
fn writer_union() {
    assert_generated_writers_eq(
        r#"@endian(big) struct Packet {
            kind: u8,
            body: union(kind) {
                1 => Connect { keep_alive: u16 },
                2 => Connack { ack: u8, code: u8 },
                _ => Unknown { },
            },
        }"#,
        "writer_union",
    );
}

#[test]
fn writer_greedy_open_tail() {
    assert_generated_writers_eq(
        r#"struct Vlan {
            dst: [u8; 6],
            src: [u8; 6],
            tpid = x8100,
            pcp: b<3>,
            dei: b<1>,
            vid_hi: b<4>,
            vid_lo: u8,
            ethertype: u16,
            @greedy(unsafe_eof) payload: [u8],
        }"#,
        "writer_greedy_open_tail",
    );
}

#[test]
fn writer_hook_len_without_write_hook_errors() {
    let err = generate_writers_err(
        r#"struct VarFrame {
            tag: u8,
            @hook(binparse.hooks.leb128_unsigned, u64) len: [u8],
            body: [u8; len],
        }"#,
    );
    assert!(
        matches!(
            err,
            Error::Writer(crate::writer::Error::MissingWriteHook { .. })
        ),
        "expected MissingWriteHook, got {err:?}"
    );
    assert!(err.to_string().contains("write_hook"));
}

#[test]
fn writer_mid_struct_union_skipped() {
    let code = generate_writers(
        r#"@endian(big) struct Packet {
            kind: u8,
            body: union(kind) {
                1 => Connect { keep_alive: u16 },
                _ => Unknown { },
            },
            trailer: u8,
        }"#,
    );
    syn::parse_str::<syn::File>(&code).expect("generated writer code is not valid Rust");
    assert!(!code.contains("PacketWriter"));
}

#[test]
fn writer_until_tail_skipped() {
    let code = generate_writers(
        r#"struct Listing { count: u8, @until(x00) entries: [u8] }"#,
    );
    syn::parse_str::<syn::File>(&code).expect("generated writer code is not valid Rust");
    assert!(!code.contains("ListingWriter"));
}
