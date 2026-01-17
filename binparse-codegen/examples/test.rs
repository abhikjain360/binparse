const DSL_STRING: &str = r#"
struct MyPacket {
    ty: u8,
    code: u8,
    checksum: u16,
    something: u16,
    conct: concat(u16, u16),
    payload: union(ty, something) {
        (0, 0) | (0, 8) => Echo { id: u16, seq: u16 },
        (3, 0) => @endian(little) DestUnreachable { unused: u32 },
        (11, 2) => TimeExceeded { unused: u32 },
        _ => Unknown { },
    },
}

@endian(little)
struct LittleEndianPacket {
    header: u32,
    @endian(big) mixed: u16,
    data: u8,
}

@endian(big)
struct BigEndianPacket {
    value: u64,
}
"#;

fn main() {
    let ast = binparse_dsl_parse::parse_str(DSL_STRING)
        .inspect_err(|e| eprintln!("{e}"))
        .unwrap();
    let code = binparse_codegen::CodeGen::generate(&ast)
        .inspect_err(|e| eprintln!("{e}"))
        .unwrap();
    println!("{code}");
}
