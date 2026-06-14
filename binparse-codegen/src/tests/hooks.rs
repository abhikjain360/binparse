use super::*;

#[test]
fn golden_fixed_hook() {
    assert_generated_eq(
        r#"struct WithFixedHook {
            prefix: u8,
            @hook(double_it, u32)
            value: u16,
            suffix: u8,
        }"#,
        "fixed_hook",
    );
}
#[test]
fn golden_len_bounded_hook() {
    assert_generated_eq(
        r#"struct WithLenHook {
            len: u8,
            @len(len) @hook(read_leb128, u64) value: [u8],
            after: u8,
        }"#,
        "len_bounded_hook",
    );
}
#[test]
fn golden_conditional_hook() {
    assert_generated_eq(
        r#"struct WithCondHook {
            kind: u8,
            if (kind == 1) {
                @hook(double_it, u32) value: u16,
            }
            tail: u8,
        }"#,
        "conditional_hook",
    );
}
#[test]
fn golden_vla_hook() {
    assert_generated_eq(
        r#"struct WithVlaHook {
            len: u8,
            @hook(parse_cstring, String)
            name: [u8],
        }"#,
        "vla_hook",
    );
}
#[test]
fn golden_cache_len_hook() {
    assert_generated_eq(
        r#"struct WithCachedHook {
            count: u8,
            @hook(parse_cstring, String) @cache(len) body: [u8],
            trailer: u16,
        }"#,
        "cache_len_hook",
    );
}
#[test]
fn golden_cache_value_hook() {
    assert_generated_eq(
        r#"struct Foo {
            count: u8,
            @hook(parse_cstring, String) @cache(value) body: [u8],
            trailer: u16,
        }"#,
        "cache_value_hook",
    );
}
#[test]
fn golden_cache_len_value_hook() {
    assert_generated_eq(
        r#"struct Foo {
            count: u8,
            @hook(parse_cstring, String) @cache body: [u8],
            trailer: u16,
        }"#,
        "cache_len_value_hook",
    );
}
#[test]
fn cache_with_unknown_arg_is_rejected() {
    let err = generate_err(
        r#"struct Foo {
            count: u8,
            @hook(parse_cstring, String) @cache(bogus) body: [u8],
        }"#,
    );
    assert!(
        err.to_string()
            .contains("@cache arguments must be 'len' or 'value'")
    );
}
#[test]
fn cache_value_on_non_hook_is_rejected() {
    let err = generate_err("struct Foo { count: u8, @cache(value) value: u16 }");
    assert!(
        err.to_string()
            .contains("@cache(value) is only supported on @hook or union fields")
    );
}
#[test]
fn cache_len_on_fixed_offset_is_ignored() {
    let code = generate("struct Foo { count: u8, @cache(len) value: u16 }");
    assert!(!code.contains("value_end_cache"));
    assert!(code.contains("fn value(&mut self) -> u16"));
}
