const DSL_STRING: &str = r#"
struct IpAddr {
    a: u8,
    b: u8,
    c: u8,
    d: u8,
}

struct Header {
    version: b<4>,
    ihl: b<4>,
    dscp: b<6>,
    ecn: b<2>,
    total_length: u16,
    identification: u16,
    flags: b<3>,
    fragment_offset: concat(b<5>, u8),
}
"#;

fn main() {
    let ast = binparse_dsl_parse::parse_str(DSL_STRING)
        .inspect_err(|e| eprintln!("{e}"))
        .unwrap();
    println!("got the AST!!");
    let code = binparse_codegen::CodeGen::generate(&ast)
        .inspect_err(|e| eprintln!("{e}"))
        .unwrap();
    println!("{code}");
}
