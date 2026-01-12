const DSL_STRING: &'static str = r#"
struct Header {
    version: b<4>,
    ihl: b<4>,
    tos: u8,
    total_length: u16
}
"#;

fn main() {
    let ast = binparse_dsl_parse::parse_str(DSL_STRING).unwrap();
    let code = binparse_codegen::CodeGen::generate(&ast)
        .inspect_err(|e| eprintln!("{e}"))
        .unwrap();
    println!("{code}");
}
