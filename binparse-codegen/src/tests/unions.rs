use super::*;

#[test]
fn golden_union_single_discriminant() {
    assert_generated_eq(
        r#"struct Packet {
            ty: u8,
            payload: union(ty) {
                1 => Echo { id: u16 },
                _ => Unknown { },
            },
        }"#,
        "union_single_discriminant",
    );
}
#[test]
fn golden_union_tuple_discriminant_with_multiple_matchers() {
    assert_generated_eq(
        r#"struct Packet {
            ty: u8,
            code: u8,
            payload: union(ty, code) {
                (0, 0) | (0, 8) => Echo { id: u16 },
                _ => Unknown { },
            },
        }"#,
        "union_tuple_discriminant_with_multiple_matchers",
    );
}
#[test]
fn golden_union_error_variant() {
    assert_generated_eq(
        r#"error {
            UNKNOWN_TYPE { ty: u8 },
        }

        struct Packet {
            ty: u8,
            payload: union(ty) {
                1 => Echo { id: u16 },
                _ => @error(UNKNOWN_TYPE { ty: ty }),
            },
        }"#,
        "union_error_variant",
    );
}
#[test]
fn golden_cache_len_value_union() {
    assert_generated_eq(
        r#"struct Packet {
            ty: u8,
            len: u8,
            @len(len) @cache payload: union(ty) {
                1 => Echo { id: u16 },
                _ => Unknown { },
            },
        }"#,
        "cache_len_value_union",
    );
}
#[test]
fn discriminator_on_struct_ref_is_rejected() {
    let err = generate_err(
        "struct Inner { x: u8 } struct Foo { @discriminator inner: Inner, @payload p: [u8; 1] }",
    );
    assert!(
        err.to_string()
            .contains("@discriminator can only be applied to primitive and bitfield fields")
    );
}
#[test]
fn payload_on_primitive_is_rejected() {
    let err = generate_err("struct Foo { @payload x: u16 }");
    assert!(
        err.to_string()
            .contains("@payload can only be applied to byte-array or struct ref fields")
    );
}
#[test]
fn payload_on_non_u8_array_is_rejected() {
    let err = generate_err("struct Foo { @payload x: [u16; 2] }");
    assert!(
        err.to_string()
            .contains("@payload can only be applied to byte-array or struct ref fields")
    );
}
#[test]
fn multiple_payloads_are_rejected() {
    let err = generate_err("struct Foo { @payload a: [u8; 1], @payload b: [u8; 1] }");
    assert!(err.to_string().contains("at most one @payload field"));
}
#[test]
fn payload_inside_conditional_is_rejected() {
    let err = generate_err("struct Foo { f: u8, if (f > 0) { @payload p: [u8; 1] } }");
    assert!(
        err.to_string()
            .contains("@payload cannot be applied inside a conditional")
    );
}
#[test]
fn discriminator_on_skip_is_rejected() {
    let err = generate_err("struct Foo { @skip @discriminator x: u8, @payload p: [u8; 1] }");
    assert!(
        err.to_string()
            .contains("@discriminator cannot be applied to a @skip field")
    );
}
#[test]
fn discriminator_wrong_arg_count_is_rejected() {
    let err = generate_err("struct Foo { @discriminator(1) x: u8 }");
    assert!(err.to_string().contains("requires exactly 0 argument"));
}
#[test]
fn union_unknown_argument_is_rejected() {
    let err = generate_err(
        r#"struct Foo {
            payload: union(kind) {
                1 => A { x: u8 },
                _ => B { },
            },
        }"#,
    );
    assert!(
        err.to_string().contains(
            "expression 'kind' references field 'kind' which is unknown or not yet parsed"
        )
    );
}
#[test]
fn union_non_numeric_argument_is_rejected() {
    let err = generate_err(
        r#"struct Inner { x: u8 }
        struct Foo {
            inner: Inner,
            payload: union(inner) {
                1 => A { x: u8 },
                _ => B { },
            },
        }"#,
    );
    assert!(
        err.to_string()
            .contains("expression 'inner' references field 'inner' which is not a numeric field")
    );
}
#[test]
fn union_without_catch_all_is_rejected() {
    let err = generate_err(
        r#"struct Packet {
            ty: u8,
            payload: union(ty) {
                1 => Echo { id: u16 },
            },
        }"#,
    );
    assert!(err.to_string().contains("union is not exhaustive"));
}
#[test]
fn union_with_wildcard_error_variant_is_exhaustive() {
    let code = generate(
        r#"error {
            UNKNOWN_TYPE,
        }

        struct Packet {
            ty: u8,
            payload: union(ty) {
                1 => Echo { id: u16 },
                _ => @error(UNKNOWN_TYPE),
            },
        }"#,
    );
    assert!(code.contains("Err(Error::UNKNOWN_TYPE)"));
}
#[test]
fn union_tuple_of_wildcards_is_exhaustive() {
    let code = generate(
        r#"struct Packet {
            ty: u8,
            code: u8,
            payload: union(ty, code) {
                (1, 1) => Echo { id: u16 },
                (_, _) => Unknown { },
            },
        }"#,
    );
    assert!(code.contains("(_, _) =>"));
}
#[test]
fn union_matcher_arity_mismatch_is_rejected() {
    let err = generate_err(
        r#"struct Packet {
            ty: u8,
            payload: union(ty) {
                (1, 2) => Echo { id: u16 },
                _ => Unknown { },
            },
        }"#,
    );
    assert!(
        err.to_string()
            .contains("matcher has 2 elements but union has 1 arguments")
    );
}
#[test]
fn union_literal_matcher_on_tuple_union_is_rejected() {
    let err = generate_err(
        r#"struct Packet {
            ty: u8,
            code: u8,
            payload: union(ty, code) {
                1 => Echo { id: u16 },
                _ => Unknown { },
            },
        }"#,
    );
    assert!(
        err.to_string()
            .contains("matcher has 1 elements but union has 2 arguments")
    );
}
#[test]
fn union_unknown_error_variant_is_rejected() {
    let err = generate_err(
        r#"struct Packet {
            ty: u8,
            payload: union(ty) {
                1 => Echo { id: u16 },
                _ => @error(UNKNOWN_TYPE),
            },
        }"#,
    );
    assert!(
        err.to_string()
            .contains("@error variant 'UNKNOWN_TYPE' is not declared in an error block")
    );
}
#[test]
fn union_error_variant_missing_field_is_rejected() {
    let err = generate_err(
        r#"error {
            UNKNOWN_TYPE { ty: u8, code: u8 },
        }

        struct Packet {
            ty: u8,
            payload: union(ty) {
                1 => Echo { id: u16 },
                _ => @error(UNKNOWN_TYPE { ty: ty }),
            },
        }"#,
    );
    assert!(
        err.to_string()
            .contains("@error variant 'UNKNOWN_TYPE' is missing field 'code'")
    );
}
#[test]
fn union_error_variant_unknown_field_is_rejected() {
    let err = generate_err(
        r#"error {
            UNKNOWN_TYPE { ty: u8 },
        }

        struct Packet {
            ty: u8,
            payload: union(ty) {
                1 => Echo { id: u16 },
                _ => @error(UNKNOWN_TYPE { ty: ty, extra: ty }),
            },
        }"#,
    );
    assert!(
        err.to_string()
            .contains("@error variant 'UNKNOWN_TYPE' has no declared field 'extra'")
    );
}
#[test]
fn unions_in_concat_get_distinct_names() {
    let code = generate(
        r#"struct Packet {
            a: u8,
            b: u8,
            pair: concat(
                union(a) { 1 => X { x: u8 }, _ => XOther { } },
                union(b) { 2 => Y { y: u16 }, _ => YOther { } }
            ),
        }"#,
    );
    assert!(code.contains("pub enum Packet_pair_0<'a>"));
    assert!(code.contains("pub enum Packet_pair_1<'a>"));
    assert!(code.contains("self.pair_0_union_check()?;"));
    assert!(code.contains("self.pair_1_union_check()?;"));
}
#[test]
fn error_struct_name_conflict_is_rejected() {
    let err = generate_err(
        r#"error {
            UNKNOWN_TYPE,
        }

        struct Error {
            ty: u8,
        }"#,
    );
    assert!(
        err.to_string()
            .contains("struct name 'Error' conflicts with the generated error enum")
    );
}
#[test]
fn error_struct_name_without_error_block_is_allowed() {
    let code = generate("struct Error { ty: u8 }");
    assert!(code.contains("pub struct Error<'a>"));
}
#[test]
fn parse_error_variant_name_is_rejected() {
    let err = generate_err("error { Parse }");
    assert!(
        err.to_string()
            .contains("error variant 'Parse' is reserved for wrapped parse errors")
    );
}
#[test]
fn duplicate_error_block_is_rejected() {
    let err = generate_err("error { A } error { B }");
    assert!(err.to_string().contains("duplicate error block"));
}
#[test]
fn duplicate_error_variant_is_rejected() {
    let err = generate_err("error { A, A }");
    assert!(err.to_string().contains("duplicate error variant 'A'"));
}
#[test]
fn union_variant_validation_is_generated_in_variant_parse() {
    let code = generate(
        r#"struct Packet {
            ty: u8,
            payload: union(ty) {
                1 => Echo { version = 4 },
                _ => Unknown { },
            },
        }"#,
    );
    assert!(code.contains("fn version_validate"));
    assert!(code.contains("self.version_validate()?;"));
    assert!(code.contains("me.version_recoverable_check()?;"));
}
