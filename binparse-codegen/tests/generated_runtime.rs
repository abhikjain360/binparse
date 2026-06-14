use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn generated_code(dsl: &str) -> String {
    let ast = binparse_dsl_parse::parse_str(dsl).expect("failed to parse DSL");
    binparse_codegen::CodeGen::generate(&ast).expect("failed to generate code")
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("binparse-codegen should have a workspace parent")
        .to_path_buf()
}

fn write_runtime_crate(code: &str) -> PathBuf {
    let root = workspace_root();
    let test_dir = root
        .join("target")
        .join("generated-runtime-tests")
        .join(format!("runtime-{}", std::process::id()));

    let _ = fs::remove_dir_all(&test_dir);
    fs::create_dir_all(test_dir.join("src")).expect("failed to create runtime test crate");

    let binparse_path = root.join("binparse");
    fs::write(
        test_dir.join("Cargo.toml"),
        format!(
            r#"[package]
name = "generated-runtime-test"
version = "0.0.0"
edition = "2024"

[dependencies]
binparse = {{ path = "{}" }}

[workspace]
"#,
            binparse_path.display()
        ),
    )
    .expect("failed to write runtime Cargo.toml");

    fs::write(
        test_dir.join("src/lib.rs"),
        format!(
            r#"
fn double_it(value: u16, _ctx: binparse::HookContext<'_>) -> binparse::ParseResult<u32> {{
    Ok(u32::from(value) * 2)
}}

static COUNTING_TRANSFORM_CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn counting_double_it(value: u16, _ctx: binparse::HookContext<'_>) -> binparse::ParseResult<u32> {{
    COUNTING_TRANSFORM_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Ok(u32::from(value) * 2)
}}

fn failing_transform(_value: u16, ctx: binparse::HookContext<'_>) -> binparse::ParseResult<u32> {{
    Err(binparse::ParseError::HookFailed {{
        field: ctx.field,
        reason: "transform refused",
    }})
}}

fn parse_cstring(data: &[u8], ctx: binparse::HookContext<'_>) -> binparse::ParseResult<(String, usize)> {{
    binparse::hooks::cstring(data, ctx)
}}

fn read_leb128(data: &[u8], ctx: binparse::HookContext<'_>) -> binparse::ParseResult<(u64, usize)> {{
    binparse::hooks::leb128_unsigned(data, ctx)
}}

static COUNTING_HOOK_CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static COUNTING_VALUE_HOOK_CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static COUNTING_LEN_VALUE_HOOK_CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static COUNTING_UNION_HOOK_CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn counting_cstring(data: &[u8], ctx: binparse::HookContext<'_>) -> binparse::ParseResult<(String, usize)> {{
    COUNTING_HOOK_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    binparse::hooks::cstring(data, ctx)
}}

fn counting_value_cstring(data: &[u8], ctx: binparse::HookContext<'_>) -> binparse::ParseResult<(String, usize)> {{
    COUNTING_VALUE_HOOK_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    binparse::hooks::cstring(data, ctx)
}}

fn counting_len_value_cstring(data: &[u8], ctx: binparse::HookContext<'_>) -> binparse::ParseResult<(String, usize)> {{
    COUNTING_LEN_VALUE_HOOK_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    binparse::hooks::cstring(data, ctx)
}}

fn counting_union_cstring(data: &[u8], ctx: binparse::HookContext<'_>) -> binparse::ParseResult<(String, usize)> {{
    COUNTING_UNION_HOOK_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    binparse::hooks::cstring(data, ctx)
}}

fn lying_hook(data: &[u8], _ctx: binparse::HookContext<'_>) -> binparse::ParseResult<(u8, usize)> {{
    Ok((0, data.len() + 100))
}}

fn parse_dns_name(_data: &[u8], ctx: binparse::HookContext<'_>) -> binparse::ParseResult<(String, usize)> {{
    let msg = ctx.enclosing;
    let mut labels: Vec<String> = Vec::new();
    let mut pos = ctx.offset;
    let mut consumed = None;
    let mut jumps = 0;
    loop {{
        let len_byte = *msg.get(pos).ok_or(binparse::ParseError::NotEnoughData {{
            expected: pos + 1,
            got: msg.len(),
        }})?;
        if len_byte & 0xC0 == 0xC0 {{
            let second = *msg.get(pos + 1).ok_or(binparse::ParseError::NotEnoughData {{
                expected: pos + 2,
                got: msg.len(),
            }})?;
            if consumed.is_none() {{
                consumed = Some(pos + 2 - ctx.offset);
            }}
            jumps += 1;
            if jumps > 8 {{
                return Err(binparse::ParseError::HookFailed {{
                    field: ctx.field,
                    reason: "too many DNS compression jumps",
                }});
            }}
            pos = (usize::from(len_byte & 0x3F) << 8) | usize::from(second);
        }} else if len_byte == 0 {{
            let consumed = consumed.unwrap_or_else(|| pos + 1 - ctx.offset);
            return Ok((labels.join("."), consumed));
        }} else {{
            let end = pos + 1 + usize::from(len_byte);
            let label = msg.get(pos + 1..end).ok_or(binparse::ParseError::NotEnoughData {{
                expected: end,
                got: msg.len(),
            }})?;
            labels.push(String::from_utf8_lossy(label).to_string());
            pos = end;
        }}
    }}
}}

{code}

#[cfg(test)]
mod tests {{
    use super::*;

    fn assert_parse_no_panic<F>(name: &str, data: &[u8], parse: F)
    where
        F: Fn(&[u8]),
    {{
        for len in 0..=data.len() {{
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {{
                parse(&data[..len]);
            }}));
            assert!(result.is_ok(), "{{name}} panicked at len {{len}}");
        }}
    }}

    fn assert_dissect_no_panic<F>(name: &str, data: &[u8], dissect: F)
    where
        F: for<'b> Fn(&'b [u8]) -> binparse::FieldNode<'b>,
    {{
        for len in 0..=data.len() {{
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {{
                let tree = dissect(&data[..len]);
                let _ = tree.errors();
            }}));
            assert!(result.is_ok(), "{{name}} dissect panicked at len {{len}}");
        }}

        let mut corrupted = data.to_vec();
        if !corrupted.is_empty() {{
            corrupted[0] ^= 0xff;
            let tree = dissect(&corrupted);
            let _ = tree.errors();
        }}
        let mut oversized = data.to_vec();
        if !oversized.is_empty() {{
            oversized[0] = 0xff;
            let tree = dissect(&oversized);
            let _ = tree.errors();
        }}
    }}

    #[test]
    fn dissect_truncated_validated_keeps_decoded_prefix() {{
        let valid = [0x89, 0x50, 0x4e, 0x47, 0x45, 0x00, 0x14, 0b00_000011];

        let truncated = &valid[..6];
        let tree = Validated::dissect(truncated);
        assert_eq!(tree.children[0].name, "magic");
        assert_eq!(tree.children[0].value, binparse::Value::UInt(0x89504e47));
        let last = tree.children.last().unwrap();
        assert_eq!(last.name, "total_len");
        assert!(matches!(last.status, binparse::Status::Error(_)));
        assert!(matches!(tree.status, binparse::Status::Error(_)));
        assert!(!tree.errors().is_empty());

        assert_dissect_no_panic("Validated", &valid, |d| Validated::dissect(d));
    }}

    #[test]
    fn dissect_bad_magic_reports_validation_error_and_continues() {{
        let mut bad = [0x89, 0x50, 0x4e, 0x47, 0x45, 0x00, 0x14, 0b00_000011];
        bad[0] = 0x88;
        let tree = Validated::dissect(&bad);
        let magic = &tree.children[0];
        assert_eq!(magic.name, "magic");
        assert_eq!(
            magic.status,
            binparse::Status::Error(binparse::ParseError::ValidationFailed {{
                field: "Validated.magic",
                actual: 0x88504e47,
            }})
        );
        let names: Vec<_> = tree.children.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["magic", "version", "ihl", "total_len", "reserved", "flags"]);
        assert_eq!(tree.children[3].value, binparse::Value::UInt(20));
        assert_eq!(tree.status, binparse::Status::Ok);
        let errors = tree.errors();
        assert_eq!(errors[0].0, "Validated.magic");
    }}

    #[test]
    fn dissect_bad_length_is_recoverable() {{
        let mut bad = [0x89, 0x50, 0x4e, 0x47, 0x45, 0x00, 0x14, 0b00_000011];
        bad[6] = 0x13;
        let tree = Validated::dissect(&bad);
        let total_len = &tree.children[3];
        assert_eq!(total_len.name, "total_len");
        assert!(matches!(
            total_len.status,
            binparse::Status::Error(binparse::ParseError::ValidationFailed {{ .. }})
        ));
        assert_eq!(tree.children.len(), 6);
        assert_eq!(tree.status, binparse::Status::Ok);
    }}

    #[test]
    fn dissect_unknown_variant_surfaces_failed_node() {{
        let data = [9, 0xaa];
        let tree = Dispatch::dissect(&data);
        assert_eq!(tree.children[0].name, "kind");
        let body = &tree.children[1];
        assert_eq!(body.name, "body");
        assert_eq!(body.status, binparse::Status::Failed("UNKNOWN_KIND"));
        assert_eq!(tree.status, binparse::Status::Ok);
        assert_dissect_no_panic("Dispatch", &[1, 3, 0xaa, 0xbb, 0xcc, 0x99], |d| Dispatch::dissect(d));
    }}

    #[test]
    fn dissect_heavy_specs_never_panic_on_prefixes_and_corruption() {{
        let cond = [0x46, 0xaa, 0xbb, 0xcc, 0xdd, 0x11];
        assert_dissect_no_panic("Ipv4WithOptions", &cond, |d| Ipv4WithOptions::dissect(d));

        let cond_else = [1, 7, 9];
        assert_dissect_no_panic("CondElse", &cond_else, |d| CondElse::dissect(d));

        let concat_union = [1, 2, 0x42, 0x12, 0x34, 2, 0xaa, 0xbb, 0x99];
        assert_dissect_no_panic("ConcatUnion", &concat_union, |d| ConcatUnion::dissect(d));

        let icmp = [8, 0, 0x12, 0x34, 0x00, 0x01];
        assert_dissect_no_panic("Icmp", &icmp, |d| Icmp::dissect(d));

        let varint = [7, 0xE5, 0x8E, 0x26, 9];
        assert_dissect_no_panic("Varint", &varint, |d| Varint::dissect(d));

        let dns = [
            0xAB, 0xCD, 3, b'a', b'b', b'c', 2, b'd', b'e', 0, 0x00, 0x01, 0xC0, 0x02, 0x00, 0x1C,
        ];
        assert_dissect_no_panic("DnsMsg", &dns, |d| DnsMsg::dissect(d));

        let padded = [1, 0, 0, 2, 0x12, 0x34, 0x56, 0x78];
        assert_dissect_no_panic("Padded", &padded, |d| Padded::dissect(d));

        let cstr = [b'h', b'i', 0, 7];
        assert_dissect_no_panic("CStr", &cstr, |d| CStr::dissect(d));

        let greedy = [1, 0x02, 0x03, 4, 0x05, 0x06];
        assert_dissect_no_panic("GreedyStructs", &greedy, |d| GreedyStructs::dissect(d));

        let opts = [0u8; 9];
        assert_dissect_no_panic("Opts", &opts, |d| Opts::dissect(d));

        let bounded = [7, 5, 0x01, 0x02, 0x03, 0xaa, 0xbb, 0x99];
        assert_dissect_no_panic("Bounded", &bounded, |d| Bounded::dissect(d));

        let bounded_union = [1, 5, 0xaa, 0x00, 0x10, 0xcc, 0xdd, 0x99];
        assert_dissect_no_panic("BoundedUnion", &bounded_union, |d| BoundedUnion::dissect(d));

        let bounded_greedy = [3, 0x11, 0x22, 0x33, 0x77];
        assert_dissect_no_panic("BoundedGreedy", &bounded_greedy, |d| BoundedGreedy::dissect(d));

        let bounded_until = [4, 0x11, 0x22, 0x00, 0x44, 0x77];
        assert_dissect_no_panic("BoundedUntil", &bounded_until, |d| BoundedUntil::dissect(d));
    }}

    #[test]
    fn baseline_valid_packet_decodes() {{
        let data = [
            1, 0x34, 0x12, 0x01, 0x02, 0x03, 0x04, 0b1010_1101, 9, 8, 7, 0xaa, 0x01, 0x02,
            0x78, 0x56, 0x55, 0xcd, 0xab, 0xfe,
        ];
        let (mut packet, rem) = Baseline::parse(&data).unwrap();
        assert!(rem.is_empty());
        assert_eq!(packet.n(), 1);
        assert_eq!(packet.word(), 0x1234);
        assert_eq!(packet.be(), 0x0102_0304);
        assert_eq!(packet.flag_a(), 5);
        assert_eq!(packet.flag_b(), 13);

        let fixed = packet
            .fixed()
            .unwrap()
            .collect::<binparse::ParseResult<Vec<_>>>()
            .unwrap();
        assert_eq!(fixed, vec![9, 8, 7]);

        let mut inner = packet.inner().unwrap();
        assert_eq!(inner.a(), 0xaa);
        assert_eq!(inner.b(), 0x0102);

        let dyns = packet
            .dyns()
            .unwrap()
            .collect::<binparse::ParseResult<Vec<_>>>()
            .unwrap();
        assert_eq!(dyns, vec![0x5678]);
        assert_eq!(packet.pair(), (0x55, 0xabcd));

        match packet.payload().unwrap() {{
            Baseline_payload::One(mut one) => assert_eq!(one.x(), 0xfe),
            Baseline_payload::Unknown(_) => panic!("expected One payload"),
        }}
    }}

    #[test]
    fn offsets_report_absolute_bit_ranges() {{
        let data = [
            1, 0x34, 0x12, 0x01, 0x02, 0x03, 0x04, 0b1010_1101, 9, 8, 7, 0xaa, 0x01, 0x02,
            0x78, 0x56, 0x55, 0xcd, 0xab, 0xfe,
        ];
        let (mut packet, _) = Baseline::parse(&data).unwrap();
        assert_eq!(packet.n_start_offset(), binparse::Len::ZERO);
        assert_eq!(packet.n_bit_range(), 0..8);
        assert_eq!(packet.word_bit_range(), 8..24);
        assert_eq!(packet.be_bit_range(), 24..56);
        assert_eq!(packet.flag_a_bit_range(), 56..59);
        assert_eq!(packet.flag_b_bit_range(), 59..64);
        assert_eq!(packet.fixed_bit_range(), 64..88);
        assert_eq!(packet.inner_bit_range(), 88..112);
        assert_eq!(packet.dyns_bit_range(), 112..128);
        assert_eq!(packet.pair_bit_range(), 128..152);
        assert_eq!(packet.payload_bit_range(), 152..160);
        assert_eq!(packet.payload_end_offset(), binparse::Len {{ byte: 20, bit: 0 }});

        let mut inner = packet.inner().unwrap();
        assert_eq!(inner.a_bit_range(), 0..8);
        assert_eq!(inner.b_bit_range(), 8..24);
        let inner_base = packet.inner_start_offset().bits();
        assert_eq!(inner_base + inner.b_bit_range().start, 96);
        assert_eq!(inner_base + inner.b_bit_range().end, 112);
    }}

    #[test]
    fn cross_byte_bitfield_offsets_and_values() {{
        let data = [0xad, 0xad];
        let (mut packet, rem) = CrossByte::parse(&data).unwrap();
        assert!(rem.is_empty());
        assert_eq!(packet.high(), 21);
        assert_eq!(packet.mid(), 45);
        assert_eq!(packet.low(), 13);
        assert_eq!(packet.high_bit_range(), 0..5);
        assert_eq!(packet.mid_bit_range(), 5..11);
        assert_eq!(packet.low_bit_range(), 11..16);
        assert_eq!(packet.mid_start_offset(), binparse::Len {{ byte: 0, bit: 5 }});
        assert_eq!(packet.mid_end_offset(), binparse::Len {{ byte: 1, bit: 3 }});
        assert_parse_no_panic("CrossByte", &data, |data| {{
            if let Ok((mut packet, _)) = CrossByte::parse(data) {{
                let _ = packet.field_tree();
            }}
        }});
    }}

    #[test]
    fn size_expression_valid_packet_decodes() {{
        let data = [0, 0, 0, 0, 0, 0, 0, 2, 1, 2, 3, 4];
        let (mut packet, rem) = SizeExpr::parse(&data).unwrap();
        assert!(rem.is_empty());
        let xs = packet
            .xs()
            .unwrap()
            .collect::<binparse::ParseResult<Vec<_>>>()
            .unwrap();
        assert_eq!(xs, vec![1, 2, 3, 4]);
        assert_eq!(packet.xs_bit_range(), 64..96);
    }}

    #[test]
    fn size_expression_overflow_saturates_instead_of_panicking() {{
        let data = [0xff; 8];
        let err = SizeExpr::parse(&data).map(|_| ()).unwrap_err();
        assert_eq!(
            err,
            binparse::ParseError::NotEnoughData {{
                expected: usize::MAX,
                got: 8,
            }}
        );
        assert_parse_no_panic("SizeExpr", &data, |data| {{
            if let Ok((mut packet, _)) = SizeExpr::parse(data) {{
                let _ = packet.field_tree();
            }}
        }});
    }}

    #[test]
    fn huge_array_count_errors_instead_of_overflowing() {{
        let data = [0xff; 8];
        let err = Huge::parse(&data).map(|_| ()).unwrap_err();
        assert_eq!(
            err,
            binparse::ParseError::NotEnoughData {{
                expected: usize::MAX,
                got: 8,
            }}
        );
        assert_parse_no_panic("Huge", &data, |data| {{
            if let Ok((mut packet, _)) = Huge::parse(data) {{
                let _ = packet.field_tree();
            }}
        }});
    }}

    #[test]
    fn baseline_parse_does_not_panic_on_truncation() {{
        let data = [
            1, 0x34, 0x12, 0x01, 0x02, 0x03, 0x04, 0b1010_1101, 9, 8, 7, 0xaa, 0x01, 0x02,
            0x78, 0x56, 0x55, 0xcd, 0xab, 0xfe,
        ];
        assert_parse_no_panic("Baseline", &data, |data| {{
            if let Ok((mut packet, _)) = Baseline::parse(data) {{
                let _ = packet.field_tree();
            }}
        }});
    }}

    #[test]
    fn hooks_decode_and_do_not_panic_on_truncation() {{
        let data = [3, 0, 2, b'h', b'i', 0];
        let (mut packet, rem) = Hooked::parse(&data).unwrap();
        assert!(rem.is_empty());
        assert_eq!(packet.prefix(), 3);
        assert_eq!(packet.value().unwrap(), 4);
        assert_eq!(packet.name().unwrap(), "hi");
        assert_eq!(packet.value_bit_range(), 8..24);
        assert_eq!(packet.name_bit_range(), 24..48);
        assert!(matches!(
            Hooked::parse(&[3, 0, 2, b'h', b'i']),
            Err(binparse::ParseError::NotEnoughData {{ .. }})
        ));
        assert_parse_no_panic("Hooked", &data, |data| {{
            if let Ok((mut packet, _)) = Hooked::parse(data) {{
                let _ = packet.field_tree();
            }}
        }});
    }}

    #[test]
    fn leb128_hook_decodes_and_errors_propagate() {{
        let data = [7, 0xE5, 0x8E, 0x26, 9];
        let (mut packet, rem) = Varint::parse(&data).unwrap();
        assert!(rem.is_empty());
        assert_eq!(packet.tag(), 7);
        assert_eq!(packet.value().unwrap(), 624485);
        assert_eq!(packet.after(), 9);
        assert_eq!(packet.value_bit_range(), 8..32);
        assert_eq!(packet.after_bit_range(), 32..40);

        assert!(matches!(
            Varint::parse(&[7, 0xE5, 0x8E]),
            Err(binparse::ParseError::NotEnoughData {{ .. }})
        ));

        let overlong = [7, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 9];
        assert!(matches!(
            Varint::parse(&overlong),
            Err(binparse::ParseError::HookFailed {{
                field: "Varint.value",
                ..
            }})
        ));

        assert_parse_no_panic("Varint", &data, |data| {{
            if let Ok((mut packet, _)) = Varint::parse(data) {{
                let _ = packet.field_tree();
            }}
        }});
    }}

    #[test]
    fn lying_hook_cannot_overrun_parent_slice() {{
        let data = [1, 2, 3];
        assert!(matches!(
            Lying::parse(&data),
            Err(binparse::ParseError::NotEnoughData {{ .. }})
        ));
        assert_parse_no_panic("Lying", &data, |data| {{
            if let Ok((mut packet, _)) = Lying::parse(data) {{
                let _ = packet.field_tree();
            }}
        }});
    }}

    #[test]
    fn dns_name_hook_resolves_compression_with_offsets() {{
        let data = [
            0xAB, 0xCD,
            3, b'a', b'b', b'c', 2, b'd', b'e', 0,
            0x00, 0x01,
            0xC0, 0x02,
            0x00, 0x1C,
        ];
        let (mut packet, rem) = DnsMsg::parse(&data).unwrap();
        assert!(rem.is_empty());
        assert_eq!(packet.id(), 0xABCD);
        assert_eq!(packet.qname().unwrap(), "abc.de");
        assert_eq!(packet.qtype(), 1);
        assert_eq!(packet.aname().unwrap(), "abc.de");
        assert_eq!(packet.atype(), 0x1C);
        assert_eq!(packet.qname_bit_range(), 16..80);
        assert_eq!(packet.aname_bit_range(), 96..112);
        assert_eq!(packet.atype_bit_range(), 112..128);

        let mut looping = data;
        looping[12] = 0xC0;
        looping[13] = 12;
        assert!(matches!(
            DnsMsg::parse(&looping),
            Err(binparse::ParseError::HookFailed {{
                field: "DnsMsg.aname",
                ..
            }})
        ));

        let mut dangling = data;
        dangling[13] = 200;
        assert!(matches!(
            DnsMsg::parse(&dangling),
            Err(binparse::ParseError::NotEnoughData {{ .. }})
        ));

        assert_parse_no_panic("DnsMsg", &data, |data| {{
            if let Ok((mut packet, _)) = DnsMsg::parse(data) {{
                let _ = packet.field_tree();
            }}
        }});
    }}

    #[test]
    fn cache_len_memoizes_hook_end_offset() {{
        // count=7, body="hi\0" (cstring hook consumes 3), then a/b/c are u16 each.
        let data = [7, b'h', b'i', 0, 0x00, 0x0a, 0x00, 0x0b, 0x00, 0x0c];
        let (mut packet, rem) = Cached::parse(&data).unwrap();
        assert!(rem.is_empty());

        // Reset after parse: isolate the downstream offset re-walk, which is the
        // O(n^2) pathology @cache(len) fixes. Every getter/offset below calls
        // body_end_offset() transitively; without the cache each would re-run
        // the hook, with the cache the hook runs at most once (first miss).
        COUNTING_HOOK_CALLS.store(0, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(packet.count(), 7);
        assert_eq!(packet.a(), 0x000a);
        assert_eq!(packet.b(), 0x000b);
        assert_eq!(packet.c(), 0x000c);
        let _ = packet.a_bit_range();
        let _ = packet.b_bit_range();
        let _ = packet.c_bit_range();
        let _ = packet.body_bit_range();
        let _ = packet.c_end_offset();
        let _ = packet.b_end_offset();
        let _ = packet.a_end_offset();
        let offset_calls = COUNTING_HOOK_CALLS.load(std::sync::atomic::Ordering::Relaxed);
        assert!(
            offset_calls <= 1,
            "@cache(len) should memoize the end-offset across downstream offset \
             re-walks; hook ran {{offset_calls}} times (expected <= 1)"
        );

        // Reading the value itself still runs the hook (value is not cached),
        // but exactly once per body() call.
        assert_eq!(packet.body().unwrap(), "hi");
    }}

    #[test]
    fn cache_value_memoizes_hook_value_and_consumed_len() {{
        let data = [7, b'h', b'i', 0, 0x00, 0x0a];
        COUNTING_VALUE_HOOK_CALLS.store(0, std::sync::atomic::Ordering::Relaxed);
        let (mut packet, rem) = CachedValue::parse(&data).unwrap();
        assert!(rem.is_empty());
        assert_eq!(COUNTING_VALUE_HOOK_CALLS.load(std::sync::atomic::Ordering::Relaxed), 1);

        assert_eq!(packet.body().unwrap().as_str(), "hi");
        assert_eq!(packet.body().unwrap().as_str(), "hi");
        assert_eq!(packet.a(), 0x000a);
        let _ = packet.a_bit_range();
        assert_eq!(COUNTING_VALUE_HOOK_CALLS.load(std::sync::atomic::Ordering::Relaxed), 1);
    }}

    #[test]
    fn bare_cache_memoizes_value_and_end_offset() {{
        let data = [7, b'h', b'i', 0, 0x00, 0x0a];
        COUNTING_LEN_VALUE_HOOK_CALLS.store(0, std::sync::atomic::Ordering::Relaxed);
        let (mut packet, rem) = CachedLenValue::parse(&data).unwrap();
        assert!(rem.is_empty());
        assert_eq!(COUNTING_LEN_VALUE_HOOK_CALLS.load(std::sync::atomic::Ordering::Relaxed), 1);

        assert_eq!(packet.body().unwrap().as_str(), "hi");
        let _ = packet.body_bit_range();
        let _ = packet.a_end_offset();
        assert_eq!(packet.a(), 0x000a);
        assert_eq!(COUNTING_LEN_VALUE_HOOK_CALLS.load(std::sync::atomic::Ordering::Relaxed), 1);
    }}

    #[test]
    fn cache_value_memoizes_fixed_hook_value() {{
        let data = [3, 0, 2, 9];
        COUNTING_TRANSFORM_CALLS.store(0, std::sync::atomic::Ordering::Relaxed);
        let (mut packet, rem) = CachedFixedHook::parse(&data).unwrap();
        assert!(rem.is_empty());
        assert_eq!(COUNTING_TRANSFORM_CALLS.load(std::sync::atomic::Ordering::Relaxed), 1);

        assert_eq!(*packet.value().unwrap(), 4);
        assert_eq!(*packet.value().unwrap(), 4);
        let tree = packet.field_tree();
        let value_node = tree.children.iter().find(|c| c.name == "value").unwrap();
        assert_eq!(value_node.value, binparse::Value::UInt(4));
        assert_eq!(COUNTING_TRANSFORM_CALLS.load(std::sync::atomic::Ordering::Relaxed), 1);
    }}

    #[test]
    fn cached_union_preserves_inner_hook_cache_from_parse_to_getter() {{
        let data = [1, 3, b'h', b'i', 0, 9];
        COUNTING_UNION_HOOK_CALLS.store(0, std::sync::atomic::Ordering::Relaxed);
        let (mut packet, rem) = CachedUnion::parse(&data).unwrap();
        assert!(rem.is_empty());
        assert_eq!(COUNTING_UNION_HOOK_CALLS.load(std::sync::atomic::Ordering::Relaxed), 1);

        match packet.body().unwrap() {{
            CachedUnion_body::Msg(msg) => assert_eq!(msg.value().unwrap().as_str(), "hi"),
            _ => unreachable!(),
        }}
        let _ = packet.body_bit_range();
        assert_eq!(packet.trailer(), 9);
        assert_eq!(COUNTING_UNION_HOOK_CALLS.load(std::sync::atomic::Ordering::Relaxed), 1);
    }}

    #[test]
    fn fixed_hook_ok_decodes_transformed_value() {{
        let data = [3, 0, 2, b'h', b'i', 0];
        let (mut packet, _) = Hooked::parse(&data).unwrap();
        assert_eq!(packet.value().unwrap(), 4);
        let tree = packet.field_tree();
        let value_node = tree.children.iter().find(|c| c.name == "value").unwrap();
        assert_eq!(value_node.value, binparse::Value::UInt(4));
        assert!(matches!(value_node.status, binparse::Status::Ok));
    }}

    #[test]
    fn fixed_hook_err_surfaces_in_parse_and_is_recoverable_in_dissect() {{
        let data = [1, 0, 5, 2];
        // failing_transform always errors, so parse() must surface it.
        assert!(matches!(
            FailTransform::parse(&data),
            Err(binparse::ParseError::HookFailed {{ field: "FailTransform.value", .. }})
        ));

        let tree = FailTransform::dissect(&data);
        let errors = tree.errors();
        assert_eq!(errors.len(), 1, "exactly the value node errors");
        assert_eq!(errors[0].0, "FailTransform.value");
        let value_node = tree.children.iter().find(|c| c.name == "value").unwrap();
        assert_eq!(value_node.bit_range, 8..24, "error node keeps full bit range");
        // later field still decoded because the error is recoverable.
        let suffix = tree
            .children
            .iter()
            .find(|c| c.name == "suffix")
            .expect("suffix decoded after recoverable value error");
        assert_eq!(suffix.value, binparse::Value::UInt(2));

        assert_dissect_no_panic("FailTransform", &data, |d| FailTransform::dissect(d));
    }}

    #[test]
    fn len_hook_consuming_less_than_window_exposes_rest() {{
        // window=4: leb128 value 0x05 consumes 1 byte, 3 trailing window bytes are rest.
        let data = [4, 0x05, 0xAA, 0xBB, 0xCC, 0x99];
        let (mut packet, rem) = LenVarint::parse(&data).unwrap();
        assert!(rem.is_empty());
        assert_eq!(packet.len(), 4);
        assert_eq!(packet.value().unwrap(), 5);
        assert_eq!(packet.value_rest().unwrap(), &[0xAA, 0xBB, 0xCC]);
        assert_eq!(packet.value_bit_range(), 8..40);
        assert_eq!(packet.after(), 0x99);
        assert_eq!(packet.after_bit_range(), 40..48);

        let tree = packet.field_tree();
        let value_node = tree.children.iter().find(|c| c.name == "value").unwrap();
        let rest = value_node
            .children
            .iter()
            .find(|c| c.name == "rest")
            .expect("rest child present");
        assert_eq!(rest.value, binparse::Value::Bytes(&[0xAA, 0xBB, 0xCC]));
        assert_eq!(rest.bit_range, 16..40);
    }}

    #[test]
    fn len_hook_consuming_exactly_window_has_no_rest() {{
        // value 0xE5 0x8E 0x26 = 624485 consumes all 3 window bytes.
        let data = [3, 0xE5, 0x8E, 0x26, 0x77];
        let (mut packet, rem) = LenVarint::parse(&data).unwrap();
        assert!(rem.is_empty());
        assert_eq!(packet.value().unwrap(), 624485);
        assert!(packet.value_rest().unwrap().is_empty());
        assert_eq!(packet.after(), 0x77);

        let tree = packet.field_tree();
        let value_node = tree.children.iter().find(|c| c.name == "value").unwrap();
        assert!(value_node.children.iter().all(|c| c.name != "rest"));
    }}

    #[test]
    fn len_hook_lying_hook_cannot_overrun_window() {{
        // window=1 but leb128 byte requires a continuation it cannot read -> NotEnoughData.
        let data = [1, 0x80, 0x01, 0x99];
        assert!(matches!(
            LenVarint::parse(&data),
            Err(binparse::ParseError::NotEnoughData {{ .. }})
        ));
        assert_parse_no_panic("LenVarint", &data, |data| {{
            if let Ok((mut packet, _)) = LenVarint::parse(data) {{
                let _ = packet.field_tree();
            }}
        }});
        assert_dissect_no_panic("LenVarint", &data, |d| LenVarint::dissect(d));
    }}

    #[test]
    fn len_hook_dns_backward_pointer_works_under_bound() {{
        // qname is bounded to 8 bytes but compression jumps backward to offset 2.
        let data = [
            0xAB, 0xCD,
            3, b'a', b'b', b'c', 2, b'd', b'e', 0,
            0xC0, 0x02,
            0x00, 0x1C,
        ];
        let (mut packet, rem) = LenDns::parse(&data).unwrap();
        assert!(rem.is_empty());
        assert_eq!(packet.id(), 0xABCD);
        assert_eq!(packet.qname().unwrap(), "abc.de");
        // qname window is exactly 8 bytes regardless of consumed.
        assert_eq!(packet.qname_bit_range(), 16..80);
        assert_eq!(packet.aname().unwrap(), "abc.de");
        assert_eq!(packet.atype(), 0x1C);

        assert_dissect_no_panic("LenDns", &data, |d| LenDns::dissect(d));
    }}

    #[test]
    fn len_hook_truncation_and_dissect_never_panic() {{
        let data = [4, 0x05, 0xAA, 0xBB, 0xCC, 0x99];
        assert_parse_no_panic("LenVarint", &data, |data| {{
            if let Ok((mut packet, _)) = LenVarint::parse(data) {{
                let _ = packet.value();
                let _ = packet.value_rest();
                let _ = packet.field_tree();
            }}
        }});
        assert_dissect_no_panic("LenVarint", &data, |d| LenVarint::dissect(d));
    }}

    #[test]
    fn hook_in_conditional_present_path_decodes() {{
        let data = [1, 0, 5, b'h', b'i', 0, 0x42];
        let (mut packet, rem) = CondHook::parse(&data).unwrap();
        assert!(rem.is_empty());
        assert_eq!(packet.kind(), 1);
        assert_eq!(packet.doubled().unwrap().unwrap(), 10);
        assert_eq!(packet.name().unwrap().unwrap(), "hi");
        assert_eq!(packet.tail(), 0x42);

        let tree = packet.field_tree();
        let doubled = tree.children.iter().find(|c| c.name == "doubled").unwrap();
        assert_eq!(doubled.value, binparse::Value::UInt(10));
    }}

    #[test]
    fn hook_in_conditional_absent_path_skips() {{
        let data = [0, 0x42];
        let (mut packet, rem) = CondHook::parse(&data).unwrap();
        assert!(rem.is_empty());
        assert_eq!(packet.kind(), 0);
        assert!(packet.doubled().is_none());
        assert!(packet.name().is_none());
        assert_eq!(packet.tail(), 0x42);

        let tree = packet.field_tree();
        let doubled = tree.children.iter().find(|c| c.name == "doubled").unwrap();
        assert_eq!(doubled.value, binparse::Value::Absent);

        assert_parse_no_panic("CondHook", &[1, 0, 5, b'h', b'i', 0, 0x42], |data| {{
            if let Ok((mut packet, _)) = CondHook::parse(data) {{
                let _ = packet.field_tree();
            }}
        }});
        assert_dissect_no_panic("CondHook", &[1, 0, 5, b'h', b'i', 0, 0x42], |d| CondHook::dissect(d));
    }}

    #[test]
    fn struct_array_decodes_and_does_not_panic_on_truncation() {{
        let data = [2, 1, 0x02, 0x03, 4, 0x05, 0x06];
        let (mut packet, rem) = StructArray::parse(&data).unwrap();
        assert!(rem.is_empty());
        let mut items = packet
            .items()
            .unwrap()
            .collect::<binparse::ParseResult<Vec<_>>>()
            .unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].a(), 1);
        assert_eq!(items[0].b(), 0x0203);
        assert_eq!(items[1].a(), 4);
        assert_eq!(items[1].b(), 0x0506);
        assert_eq!(packet.items_bit_range(), 8..56);
        assert_parse_no_panic("StructArray", &data, |data| {{
            if let Ok((mut packet, _)) = StructArray::parse(data) {{
                let _ = packet.field_tree();
            }}
        }});

        let short = [10, 1, 0x02];
        assert_parse_no_panic("StructArray short", &short, |data| {{
            if let Ok((mut packet, _)) = StructArray::parse(data) {{
                let _ = packet.field_tree();
            }}
        }});
    }}

    #[test]
    fn signed_integers_decode_with_endian_inheritance() {{
        let mut data = vec![0xff, 0xfe, 0xff];
        data.extend((-3i32).to_be_bytes());
        data.extend((-4i64).to_le_bytes());
        data.extend((-5i128).to_le_bytes());
        data.extend(5i16.to_le_bytes());
        data.extend((-5i16).to_le_bytes());
        data.extend([0x7f, 0x80]);
        let (mut packet, rem) = Signed::parse(&data).unwrap();
        assert!(rem.is_empty());
        assert_eq!(packet.a(), -1i8);
        assert_eq!(packet.b(), -2i16);
        assert_eq!(packet.c(), -3i32);
        assert_eq!(packet.d(), -4i64);
        assert_eq!(packet.e(), -5i128);
        let vals = packet
            .vals()
            .unwrap()
            .collect::<binparse::ParseResult<Vec<_>>>()
            .unwrap();
        assert_eq!(vals, vec![5i16, -5i16]);
        let small = packet
            .small()
            .unwrap()
            .collect::<binparse::ParseResult<Vec<_>>>()
            .unwrap();
        assert_eq!(small, vec![127i8, -128i8]);
        assert_eq!(packet.a_bit_range(), 0..8);
        assert_eq!(packet.b_bit_range(), 8..24);
        assert_parse_no_panic("Signed", &data, |data| {{
            if let Ok((mut packet, _)) = Signed::parse(data) {{
                let _ = packet.field_tree();
            }}
        }});
    }}

    #[test]
    fn ipv4_version_and_ihl_decode_msb_first() {{
        let data = [0x45];
        let (mut packet, rem) = Ipv4Start::parse(&data).unwrap();
        assert!(rem.is_empty());
        assert_eq!(packet.version(), 4);
        assert_eq!(packet.ihl(), 5);
        assert_eq!(packet.version_bit_range(), 0..4);
        assert_eq!(packet.ihl_bit_range(), 4..8);
        assert_parse_no_panic("Ipv4Start", &data, |data| {{
            if let Ok((mut packet, _)) = Ipv4Start::parse(data) {{
                let _ = packet.field_tree();
            }}
        }});
    }}

    #[test]
    fn tcp_flags_decode_without_hooks() {{
        let data = [0x50, 0b0001_1000];
        let (mut packet, rem) = TcpFlags::parse(&data).unwrap();
        assert!(rem.is_empty());
        assert_eq!(packet.data_offset(), 5);
        assert_eq!(packet.reserved(), 0);
        assert_eq!(packet.ns(), 0);
        assert_eq!(packet.cwr(), 0);
        assert_eq!(packet.ece(), 0);
        assert_eq!(packet.urg(), 0);
        assert_eq!(packet.ack(), 1);
        assert_eq!(packet.psh(), 1);
        assert_eq!(packet.rst(), 0);
        assert_eq!(packet.syn(), 0);
        assert_eq!(packet.fin(), 0);
        assert_eq!(packet.ack_bit_range(), 11..12);
        assert_parse_no_panic("TcpFlags", &data, |data| {{
            if let Ok((mut packet, _)) = TcpFlags::parse(data) {{
                let _ = packet.field_tree();
            }}
        }});
    }}

    #[test]
    fn validated_packet_decodes() {{
        let data = [0x89, 0x50, 0x4e, 0x47, 0x45, 0x00, 0x14, 0b00_000011];
        let (mut packet, rem) = Validated::parse(&data).unwrap();
        assert!(rem.is_empty());
        assert_eq!(packet.magic(), 0x89504e47);
        assert_eq!(packet.version(), 4);
        assert_eq!(packet.ihl(), 5);
        assert_eq!(packet.total_len(), 20);
        assert_eq!(packet.reserved(), 0);
        assert_eq!(packet.flags(), 3);
        assert_parse_no_panic("Validated", &data, |data| {{
            if let Ok((mut packet, _)) = Validated::parse(data) {{
                let _ = packet.field_tree();
            }}
        }});
    }}

    #[test]
    fn validation_failures_report_field_and_actual_value() {{
        let valid = [0x89, 0x50, 0x4e, 0x47, 0x45, 0x00, 0x14, 0b00_000011];

        let mut bad_magic = valid;
        bad_magic[0] = 0x88;
        assert_eq!(
            Validated::parse(&bad_magic).map(|_| ()).unwrap_err(),
            binparse::ParseError::ValidationFailed {{
                field: "Validated.magic",
                actual: 0x88504e47,
            }}
        );

        let mut bad_version = valid;
        bad_version[4] = 0x55;
        assert_eq!(
            Validated::parse(&bad_version).map(|_| ()).unwrap_err(),
            binparse::ParseError::ValidationFailed {{
                field: "Validated.version",
                actual: 5,
            }}
        );

        let mut bad_len = valid;
        bad_len[6] = 0x13;
        assert_eq!(
            Validated::parse(&bad_len).map(|_| ()).unwrap_err(),
            binparse::ParseError::ValidationFailed {{
                field: "Validated.total_len",
                actual: 19,
            }}
        );

        let mut bad_reserved = valid;
        bad_reserved[7] = 0b01_000011;
        assert_eq!(
            Validated::parse(&bad_reserved).map(|_| ()).unwrap_err(),
            binparse::ParseError::ValidationFailed {{
                field: "Validated.reserved",
                actual: 1,
            }}
        );

        let mut bad_flags = valid;
        bad_flags[7] = 0b00_000111;
        assert_eq!(
            Validated::parse(&bad_flags).map(|_| ()).unwrap_err(),
            binparse::ParseError::ValidationFailed {{
                field: "Validated.flags",
                actual: 7,
            }}
        );
    }}

    #[test]
    fn truncation_is_reported_before_validation() {{
        let bad_magic = [0x88, 0x50, 0x4e];
        assert_eq!(
            Validated::parse(&bad_magic).map(|_| ()).unwrap_err(),
            binparse::ParseError::NotEnoughData {{
                expected: 4,
                got: 3,
            }}
        );
    }}

    #[test]
    fn ipv4_options_decode_when_ihl_exceeds_five() {{
        let data = [0x46, 0xaa, 0xbb, 0xcc, 0xdd, 0x11];
        let (mut packet, rem) = Ipv4WithOptions::parse(&data).unwrap();
        assert!(rem.is_empty());
        assert_eq!(packet.version(), 4);
        assert_eq!(packet.ihl(), 6);
        let options = packet
            .options()
            .expect("options should be present")
            .unwrap()
            .collect::<binparse::ParseResult<Vec<_>>>()
            .unwrap();
        assert_eq!(options, vec![0xaa, 0xbb, 0xcc, 0xdd]);
        assert_eq!(packet.proto(), 0x11);
        assert_eq!(packet.options_bit_range(), 8..40);
        assert_eq!(packet.proto_bit_range(), 40..48);
        assert_parse_no_panic("Ipv4WithOptions", &data, |data| {{
            if let Ok((mut packet, _)) = Ipv4WithOptions::parse(data) {{
                let _ = packet.field_tree();
            }}
        }});
    }}

    #[test]
    fn ipv4_options_absent_when_ihl_is_five() {{
        let data = [0x45, 0x11];
        let (mut packet, rem) = Ipv4WithOptions::parse(&data).unwrap();
        assert!(rem.is_empty());
        assert!(packet.options().is_none());
        assert_eq!(packet.proto(), 0x11);
        assert_eq!(packet.proto_bit_range(), 8..16);
        assert_parse_no_panic("Ipv4WithOptions absent", &data, |data| {{
            if let Ok((mut packet, _)) = Ipv4WithOptions::parse(data) {{
                let _ = packet.field_tree();
            }}
        }});
    }}

    #[test]
    fn ipv4_options_truncation_errors_instead_of_panicking() {{
        let data = [0x46, 0xaa, 0xbb];
        assert_eq!(
            Ipv4WithOptions::parse(&data).map(|_| ()).unwrap_err(),
            binparse::ParseError::NotEnoughData {{
                expected: 5,
                got: 3,
            }}
        );
    }}

    #[test]
    fn tcp_options_decode_based_on_data_offset() {{
        let data = [0x60, 0x01, 0x02, 0x03, 0x04];
        let (mut packet, rem) = TcpStart::parse(&data).unwrap();
        assert!(rem.is_empty());
        assert_eq!(packet.data_offset(), 6);
        let options = packet
            .options()
            .expect("options should be present")
            .unwrap()
            .collect::<binparse::ParseResult<Vec<_>>>()
            .unwrap();
        assert_eq!(options, vec![1, 2, 3, 4]);

        let no_options = [0x50];
        let (mut packet, rem) = TcpStart::parse(&no_options).unwrap();
        assert!(rem.is_empty());
        assert!(packet.options().is_none());
        assert_parse_no_panic("TcpStart", &data, |data| {{
            if let Ok((mut packet, _)) = TcpStart::parse(data) {{
                let _ = packet.field_tree();
            }}
        }});
    }}

    #[test]
    fn conditional_else_branch_updates_offsets() {{
        let then_data = [1, 7, 9];
        let (mut packet, rem) = CondElse::parse(&then_data).unwrap();
        assert!(rem.is_empty());
        assert_eq!(packet.small(), Some(7));
        assert_eq!(packet.big(), None);
        assert_eq!(packet.tail(), 9);
        assert_eq!(packet.tail_bit_range(), 16..24);

        let else_data = [0, 0x12, 0x34, 9];
        let (mut packet, rem) = CondElse::parse(&else_data).unwrap();
        assert!(rem.is_empty());
        assert_eq!(packet.small(), None);
        assert_eq!(packet.big(), Some(0x1234));
        assert_eq!(packet.tail(), 9);
        assert_eq!(packet.tail_bit_range(), 24..32);

        assert_parse_no_panic("CondElse then", &then_data, |data| {{
            if let Ok((mut packet, _)) = CondElse::parse(data) {{
                let _ = packet.field_tree();
            }}
        }});
        assert_parse_no_panic("CondElse else", &else_data, |data| {{
            if let Ok((mut packet, _)) = CondElse::parse(data) {{
                let _ = packet.field_tree();
            }}
        }});
    }}

    #[test]
    fn greedy_rest_consumes_remaining_bytes() {{
        let data = [5, 1, 2, 3];
        let (mut packet, rem) = Rest::parse(&data).unwrap();
        assert!(rem.is_empty());
        let tail = packet
            .tail()
            .unwrap()
            .collect::<binparse::ParseResult<Vec<_>>>()
            .unwrap();
        assert_eq!(tail, vec![1, 2, 3]);
        assert_eq!(packet.tail_bit_range(), 8..32);

        let (mut packet, rem) = Rest::parse(&[7]).unwrap();
        assert!(rem.is_empty());
        let tail = packet
            .tail()
            .unwrap()
            .collect::<binparse::ParseResult<Vec<_>>>()
            .unwrap();
        assert!(tail.is_empty());

        assert_parse_no_panic("Rest", &data, |data| {{
            if let Ok((mut packet, _)) = Rest::parse(data) {{
                let _ = packet.field_tree();
            }}
        }});
    }}

    #[test]
    fn greedy_rest_multibyte_requires_whole_elements() {{
        let data = [9, 0x12, 0x34, 0x56, 0x78];
        let (mut packet, rem) = RestWide::parse(&data).unwrap();
        assert!(rem.is_empty());
        let words = packet
            .words()
            .unwrap()
            .collect::<binparse::ParseResult<Vec<_>>>()
            .unwrap();
        assert_eq!(words, vec![0x1234, 0x5678]);
        assert_eq!(packet.words_bit_range(), 8..40);

        assert_eq!(
            RestWide::parse(&[9, 0x12, 0x34, 0x56]).map(|_| ()).unwrap_err(),
            binparse::ParseError::NotEnoughData {{
                expected: 5,
                got: 4,
            }}
        );
        assert_parse_no_panic("RestWide", &data, |data| {{
            if let Ok((mut packet, _)) = RestWide::parse(data) {{
                let _ = packet.field_tree();
            }}
        }});
    }}

    #[test]
    fn until_array_stops_at_sentinel() {{
        let data = [b'h', b'i', 0, 7];
        let (mut packet, rem) = CStr::parse(&data).unwrap();
        assert!(rem.is_empty());
        let name = packet
            .name()
            .unwrap()
            .collect::<binparse::ParseResult<Vec<_>>>()
            .unwrap();
        assert_eq!(name, vec![b'h', b'i']);
        assert_eq!(packet.after(), 7);
        assert_eq!(packet.name_bit_range(), 0..24);
        assert_eq!(packet.after_bit_range(), 24..32);

        let (mut packet, rem) = CStr::parse(&[0, 7]).unwrap();
        assert!(rem.is_empty());
        let name = packet
            .name()
            .unwrap()
            .collect::<binparse::ParseResult<Vec<_>>>()
            .unwrap();
        assert!(name.is_empty());
        assert_eq!(packet.after(), 7);

        assert_eq!(
            CStr::parse(&[1, 2, 3]).map(|_| ()).unwrap_err(),
            binparse::ParseError::NotEnoughData {{
                expected: 4,
                got: 3,
            }}
        );
        assert_parse_no_panic("CStr", &data, |data| {{
            if let Ok((mut packet, _)) = CStr::parse(data) {{
                let _ = packet.field_tree();
            }}
        }});
    }}

    #[test]
    fn greedy_struct_array_decodes_fixed_elements() {{
        let data = [1, 0x02, 0x03, 4, 0x05, 0x06];
        let (mut packet, rem) = GreedyStructs::parse(&data).unwrap();
        assert!(rem.is_empty());
        let mut items = packet
            .items()
            .unwrap()
            .collect::<binparse::ParseResult<Vec<_>>>()
            .unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].a(), 1);
        assert_eq!(items[0].b(), 0x0203);
        assert_eq!(items[1].a(), 4);
        assert_eq!(items[1].b(), 0x0506);
        assert_eq!(packet.items_bit_range(), 0..48);

        assert_eq!(
            GreedyStructs::parse(&[1, 2, 3, 4]).map(|_| ()).unwrap_err(),
            binparse::ParseError::NotEnoughData {{
                expected: 6,
                got: 4,
            }}
        );
        assert_parse_no_panic("GreedyStructs", &data, |data| {{
            if let Ok((mut packet, _)) = GreedyStructs::parse(data) {{
                let _ = packet.field_tree();
            }}
        }});
    }}

    #[test]
    fn max_iter_bounds_sized_array() {{
        let data = [3, 1, 2, 3];
        let (mut packet, rem) = Capped::parse(&data).unwrap();
        assert!(rem.is_empty());
        let vals = packet
            .vals()
            .unwrap()
            .collect::<binparse::ParseResult<Vec<_>>>()
            .unwrap();
        assert_eq!(vals, vec![1, 2, 3]);

        let exceeded = [5, 1, 2, 3, 4, 5];
        assert_eq!(
            Capped::parse(&exceeded).map(|_| ()).unwrap_err(),
            binparse::ParseError::MaxIterationsExceeded {{
                field: "Capped.vals",
                max: 4,
            }}
        );

        assert_eq!(
            Capped::parse(&[5, 1]).map(|_| ()).unwrap_err(),
            binparse::ParseError::NotEnoughData {{
                expected: 6,
                got: 2,
            }}
        );
        assert_parse_no_panic("Capped", &exceeded, |data| {{
            if let Ok((mut packet, _)) = Capped::parse(data) {{
                let _ = packet.field_tree();
            }}
        }});
    }}

    #[test]
    fn greedy_dynamic_struct_array_parses_until_exhausted() {{
        let data = [2, 9, 0];
        let (mut packet, rem) = Opts::parse(&data).unwrap();
        assert!(rem.is_empty());
        let mut opts = packet
            .opts()
            .unwrap()
            .collect::<binparse::ParseResult<Vec<_>>>()
            .unwrap();
        assert_eq!(opts.len(), 2);
        assert_eq!(opts[0].kind(), 2);
        assert_eq!(opts[0].body(), Some(9));
        assert_eq!(opts[1].kind(), 0);
        assert_eq!(opts[1].body(), None);
        assert_eq!(packet.opts_bit_range(), 0..24);

        let too_many = [0u8; 9];
        let (mut packet, _) = Opts::parse(&too_many).unwrap();
        assert_eq!(
            packet
                .opts()
                .unwrap()
                .collect::<binparse::ParseResult<Vec<_>>>()
                .map(|opts| opts.len())
                .unwrap_err(),
            binparse::ParseError::MaxIterationsExceeded {{
                field: "Opts.opts",
                max: 8,
            }}
        );

        let (mut packet, _) = Opts::parse(&[1]).unwrap();
        assert_eq!(
            packet
                .opts()
                .unwrap()
                .collect::<binparse::ParseResult<Vec<_>>>()
                .map(|opts| opts.len())
                .unwrap_err(),
            binparse::ParseError::NotEnoughData {{
                expected: 2,
                got: 1,
            }}
        );

        assert_parse_no_panic("Opts", &too_many, |data| {{
            if let Ok((mut packet, _)) = Opts::parse(data) {{
                let _ = packet.field_tree();
            }}
            if let Ok((mut packet, _)) = Opts::parse(data)
                && let Ok(opts) = packet.opts()
            {{
                for mut opt in opts.flatten() {{
                    let _ = opt.kind();
                    let _ = opt.body();
                }}
            }}
        }});
    }}

    #[test]
    fn padded_fields_decode_and_report_offsets() {{
        let data = [1, 0, 0, 2, 0x12, 0x34, 0x56, 0x78];
        let (mut packet, rem) = Padded::parse(&data).unwrap();
        assert!(rem.is_empty());
        assert_eq!(packet.a(), 1);
        assert_eq!(packet.b(), 2);
        assert_eq!(packet.c(), 0x1234);
        assert_eq!(packet.d(), 0x5678);
        assert_eq!(packet.a_bit_range(), 0..8);
        assert_eq!(packet.b_start_offset(), binparse::Len {{ byte: 3, bit: 0 }});
        assert_eq!(packet.b_bit_range(), 24..32);
        assert_eq!(packet.c_bit_range(), 32..48);
        assert_eq!(packet.d_bit_range(), 48..64);

        assert_eq!(
            Padded::parse(&data[..3]).map(|_| ()).unwrap_err(),
            binparse::ParseError::NotEnoughData {{
                expected: 4,
                got: 3,
            }}
        );
        assert_parse_no_panic("Padded", &data, |data| {{
            if let Ok((mut packet, _)) = Padded::parse(data) {{
                let _ = packet.field_tree();
            }}
        }});
    }}

    #[test]
    fn dynamic_pad_to_skips_to_boundary() {{
        let data = [1, 9, 0, 0, 7];
        let (mut packet, rem) = DynPadded::parse(&data).unwrap();
        assert!(rem.is_empty());
        assert_eq!(packet.tail(), 7);
        assert_eq!(packet.tail_bit_range(), 32..40);

        let aligned = [3, 9, 9, 9, 7];
        let (mut packet, rem) = DynPadded::parse(&aligned).unwrap();
        assert!(rem.is_empty());
        assert_eq!(packet.tail(), 7);
        assert_eq!(packet.tail_bit_range(), 32..40);

        assert_parse_no_panic("DynPadded", &data, |data| {{
            if let Ok((mut packet, _)) = DynPadded::parse(data) {{
                let _ = packet.field_tree();
            }}
        }});
    }}

    #[test]
    fn dynamic_align_errors_on_misaligned_offset() {{
        let data = [1, 9, 0xab, 0xcd];
        let (mut packet, rem) = DynAligned::parse(&data).unwrap();
        assert!(rem.is_empty());
        assert_eq!(packet.word(), 0xabcd);
        assert_eq!(packet.word_bit_range(), 16..32);

        let misaligned = [2, 9, 9, 0xab, 0xcd];
        assert_eq!(
            DynAligned::parse(&misaligned).map(|_| ()).unwrap_err(),
            binparse::ParseError::Misaligned {{
                field: "DynAligned.word",
                align: 2,
                offset: binparse::Len {{ byte: 3, bit: 0 }},
            }}
        );

        assert_parse_no_panic("DynAligned", &data, |data| {{
            if let Ok((mut packet, _)) = DynAligned::parse(data) {{
                let _ = packet.field_tree();
            }}
        }});
        assert_parse_no_panic("DynAligned misaligned", &misaligned, |data| {{
            if let Ok((mut packet, _)) = DynAligned::parse(data) {{
                let _ = packet.field_tree();
            }}
        }});
    }}

    #[test]
    fn skipped_fields_consume_bytes_and_stay_usable_in_expressions() {{
        let data = [0xad, 2, 0xaa, 0xbb, 0x5f];
        let (mut packet, rem) = SkipReserved::parse(&data).unwrap();
        assert!(rem.is_empty());
        assert_eq!(packet.flags(), 13);
        assert_eq!(packet.flags_bit_range(), 3..8);
        let payload = packet
            .payload()
            .unwrap()
            .collect::<binparse::ParseResult<Vec<_>>>()
            .unwrap();
        assert_eq!(payload, vec![0xaa, 0xbb]);
        assert_eq!(packet.payload_bit_range(), 16..32);
        assert_eq!(packet.pair(), (5,));
        assert_eq!(packet.pair_bit_range(), 32..40);
        assert_parse_no_panic("SkipReserved", &data, |data| {{
            if let Ok((mut packet, _)) = SkipReserved::parse(data) {{
                let _ = packet.field_tree();
            }}
        }});
    }}

    #[test]
    fn lsb_bit_order_decodes_with_field_override() {{
        let data = [0b1010_1101, 0b0100_0011];
        let (mut packet, rem) = LsbFlags::parse(&data).unwrap();
        assert!(rem.is_empty());
        assert_eq!(packet.low(), 5);
        assert_eq!(packet.high(), 21);
        assert_eq!(packet.top(), 4);
        assert_eq!(packet.bottom(), 3);
        assert_parse_no_panic("LsbFlags", &data, |data| {{
            if let Ok((mut packet, _)) = LsbFlags::parse(data) {{
                let _ = packet.field_tree();
            }}
        }});
    }}

    #[test]
    fn icmp_tuple_dispatch_decodes() {{
        let echo = [8, 0, 0x12, 0x34, 0x00, 0x01];
        let (mut packet, rem) = Icmp::parse(&echo).unwrap();
        assert!(rem.is_empty());
        match packet.body().unwrap() {{
            Icmp_body::Echo(mut echo) => {{
                assert_eq!(echo.id(), 0x1234);
                assert_eq!(echo.seq(), 1);
            }}
            _ => panic!("expected Echo body"),
        }}

        let unreach = [3, 1, 0xde, 0xad, 0xbe, 0xef];
        let (mut packet, _) = Icmp::parse(&unreach).unwrap();
        match packet.body().unwrap() {{
            Icmp_body::DestUnreach(mut unreach) => assert_eq!(unreach.unused(), 0xdead_beef),
            _ => panic!("expected DestUnreach body"),
        }}
        assert_eq!(packet.body_bit_range(), 16..48);

        let raw = [42, 7, 0xff];
        let (mut packet, rem) = Icmp::parse(&raw).unwrap();
        assert_eq!(rem, &[0xff]);
        match packet.body().unwrap() {{
            Icmp_body::Raw(_) => {{}}
            _ => panic!("expected Raw body"),
        }}

        assert_parse_no_panic("Icmp", &echo, |data| {{
            if let Ok((mut packet, _)) = Icmp::parse(data) {{
                let _ = packet.field_tree();
            }}
        }});
    }}

    #[test]
    fn union_dynamic_variant_decodes() {{
        let data = [1, 3, 0xaa, 0xbb, 0xcc, 0x99];
        let (mut packet, rem) = Dispatch::parse(&data).unwrap();
        assert_eq!(rem, &[0x99]);
        match packet.body().unwrap() {{
            Dispatch_body::Msg(mut msg) => {{
                assert_eq!(msg.len(), 3);
                let bytes = msg
                    .data()
                    .unwrap()
                    .collect::<binparse::ParseResult<Vec<_>>>()
                    .unwrap();
                assert_eq!(bytes, vec![0xaa, 0xbb, 0xcc]);
            }}
            _ => panic!("expected Msg body"),
        }}
        assert_eq!(packet.body_bit_range(), 8..40);
    }}

    #[test]
    fn union_dynamic_variant_truncation_errors_instead_of_panicking() {{
        let data = [1, 3, 0xaa];
        assert_eq!(
            Dispatch::parse(&data).map(|_| ()).unwrap_err(),
            binparse::ParseError::NotEnoughData {{ expected: 4, got: 2 }}
        );
        assert_parse_no_panic("Dispatch", &[1, 3, 0xaa, 0xbb, 0xcc, 0x99], |data| {{
            if let Ok((mut packet, _)) = Dispatch::parse(data) {{
                let _ = packet.field_tree();
            }}
        }});
    }}

    #[test]
    fn union_variant_validation_runs_at_parse() {{
        let bad = [2, 5];
        assert_eq!(
            Dispatch::parse(&bad).map(|_| ()).unwrap_err(),
            binparse::ParseError::ValidationFailed {{
                field: "Dispatch_body_Checked.version",
                actual: 5,
            }}
        );

        let good = [2, 4];
        let (mut packet, rem) = Dispatch::parse(&good).unwrap();
        assert!(rem.is_empty());
        match packet.body().unwrap() {{
            Dispatch_body::Checked(mut checked) => assert_eq!(checked.version(), 4),
            _ => panic!("expected Checked body"),
        }}
    }}

    #[test]
    fn union_error_variant_surfaces_declared_error() {{
        let data = [9, 0xaa];
        let (mut packet, rem) = Dispatch::parse(&data).unwrap();
        assert_eq!(rem, &[0xaa]);
        assert_eq!(packet.body_bit_range(), 8..8);
        match packet.body() {{
            Err(Error::UNKNOWN_KIND {{ kind }}) => assert_eq!(kind, 9),
            _ => panic!("expected UNKNOWN_KIND error"),
        }}
        assert_parse_no_panic("Dispatch", &data, |data| {{
            if let Ok((mut packet, _)) = Dispatch::parse(data) {{
                let _ = packet.field_tree();
            }}
        }});
    }}

    #[test]
    fn unions_in_concat_decode_independently() {{
        let data = [1, 2, 0x42, 0x12, 0x34, 2, 0xaa, 0xbb, 0x99];
        let (mut packet, rem) = ConcatUnion::parse(&data).unwrap();
        assert!(rem.is_empty());
        let (first, second, third) = packet.pair();
        assert_eq!(first, 0x42);
        match second.unwrap() {{
            ConcatUnion_pair_1::Word(mut word) => assert_eq!(word.w(), 0x1234),
            _ => panic!("expected Word"),
        }}
        match third.unwrap() {{
            ConcatUnion_pair_2::Bytes(mut bytes) => {{
                assert_eq!(bytes.n(), 2);
                let collected = bytes
                    .data()
                    .unwrap()
                    .collect::<binparse::ParseResult<Vec<_>>>()
                    .unwrap();
                assert_eq!(collected, vec![0xaa, 0xbb]);
            }}
            _ => panic!("expected Bytes"),
        }}
        assert_eq!(packet.tail(), 0x99);
        assert_eq!(packet.pair_bit_range(), 16..64);

        let empty = [0, 0, 0x42, 0x99];
        let (mut packet, rem) = ConcatUnion::parse(&empty).unwrap();
        assert!(rem.is_empty());
        match packet.pair().1.unwrap() {{
            ConcatUnion_pair_1::Empty(_) => {{}}
            _ => panic!("expected Empty"),
        }}
        assert_eq!(packet.tail(), 0x99);

        assert_eq!(
            ConcatUnion::parse(&data[..7]).map(|_| ()).unwrap_err(),
            binparse::ParseError::NotEnoughData {{ expected: 3, got: 2 }}
        );
        assert_parse_no_panic("ConcatUnion", &data, |data| {{
            if let Ok((mut packet, _)) = ConcatUnion::parse(data) {{
                let _ = packet.field_tree();
            }}
        }});
    }}

    #[test]
    fn len_bounded_struct_ref_decodes_within_bound() {{
        let data = [7, 5, 0x01, 0x02, 0x03, 0xaa, 0xbb, 0x99];
        let (mut packet, rem) = Bounded::parse(&data).unwrap();
        assert!(rem.is_empty());
        assert_eq!(packet.tag(), 7);
        assert_eq!(packet.len(), 5);
        let mut inner = packet.value().unwrap();
        assert_eq!(inner.a(), 0x01);
        assert_eq!(inner.b(), 0x0203);
        assert_eq!(packet.value_rest().unwrap(), &[0xaa, 0xbb]);
        assert_eq!(packet.after(), 0x99);
        assert_eq!(packet.value_bit_range(), 16..56);

        let exact = [7, 3, 0x01, 0x02, 0x03, 0x99];
        let (mut packet, rem) = Bounded::parse(&exact).unwrap();
        assert!(rem.is_empty());
        assert_eq!(packet.value().unwrap().b(), 0x0203);
        assert!(packet.value_rest().unwrap().is_empty());
        assert_eq!(packet.after(), 0x99);
    }}

    #[test]
    fn len_bounded_struct_ref_rejects_inner_overrun() {{
        let data = [7, 2, 0x01, 0x02, 0x99];
        let (mut packet, rem) = Bounded::parse(&data).unwrap();
        assert!(rem.is_empty());
        match packet.value() {{
            Err(err) => assert_eq!(
                err,
                binparse::ParseError::NotEnoughData {{ expected: 3, got: 2 }}
            ),
            Ok(_) => panic!("expected bounded inner parse to fail"),
        }}
        assert_eq!(
            packet.value_rest().unwrap_err(),
            binparse::ParseError::NotEnoughData {{ expected: 3, got: 2 }}
        );
        assert_eq!(packet.after(), 0x99);
    }}

    #[test]
    fn len_bounded_struct_ref_truncation_fails_parse() {{
        let data = [7, 5, 0x01, 0x02, 0x03, 0xaa, 0xbb, 0x99];
        assert_eq!(
            Bounded::parse(&data[..6]).map(|_| ()).unwrap_err(),
            binparse::ParseError::NotEnoughData {{ expected: 7, got: 6 }}
        );
        assert_parse_no_panic("Bounded", &data, |data| {{
            if let Ok((mut packet, _)) = Bounded::parse(data) {{
                let _ = packet.field_tree();
            }}
        }});
    }}

    #[test]
    fn len_bounded_union_decodes_within_bound_and_exposes_rest() {{
        let data = [1, 5, 0xaa, 0x00, 0x10, 0xcc, 0xdd, 0x99];
        let (mut packet, rem) = BoundedUnion::parse(&data).unwrap();
        assert!(rem.is_empty());
        assert_eq!(packet.tag(), 1);
        assert_eq!(packet.len(), 5);
        match packet.value().unwrap() {{
            BoundedUnion_value::Pair(mut p) => {{
                let mut inner = p.inner().unwrap();
                assert_eq!(inner.a(), 0xaa);
                assert_eq!(inner.b(), 0x0010);
            }}
            _ => panic!("expected Pair variant"),
        }}
        assert_eq!(packet.value_rest().unwrap(), &[0xcc, 0xdd]);
        assert_eq!(packet.after(), 0x99);
        assert_eq!(packet.value_bit_range(), 16..56);

        let tree = packet.field_tree();
        let value = tree.children.iter().find(|c| c.name == "value").unwrap();
        assert_eq!(value.value, binparse::Value::UnionVariant("Pair"));
        assert_eq!(value.children.last().unwrap().name, "rest");
        assert_eq!(
            value.children.last().unwrap().value,
            binparse::Value::Bytes(&[0xcc, 0xdd])
        );
    }}

    #[test]
    fn len_bounded_union_exact_fit_has_empty_rest() {{
        let data = [1, 3, 0xaa, 0x00, 0x10, 0x99];
        let (mut packet, rem) = BoundedUnion::parse(&data).unwrap();
        assert!(rem.is_empty());
        assert!(packet.value_rest().unwrap().is_empty());
        assert_eq!(packet.after(), 0x99);
    }}

    #[test]
    fn len_bounded_union_greedy_variant_consumes_window() {{
        let data = [2, 4, 0x01, 0x02, 0x03, 0x04, 0x99];
        let (mut packet, rem) = BoundedUnion::parse(&data).unwrap();
        assert!(rem.is_empty());
        match packet.value().unwrap() {{
            BoundedUnion_value::Blob(mut b) => {{
                let bytes = b
                    .bytes()
                    .unwrap()
                    .collect::<binparse::ParseResult<Vec<_>>>()
                    .unwrap();
                assert_eq!(bytes, vec![0x01, 0x02, 0x03, 0x04]);
            }}
            _ => panic!("expected Blob variant"),
        }}
        assert!(packet.value_rest().unwrap().is_empty());
        assert_eq!(packet.after(), 0x99);
    }}

    #[test]
    fn len_bounded_union_overrun_surfaces_at_getter() {{
        let data = [1, 2, 0xaa, 0xbb, 0x99];
        assert!(matches!(
            BoundedUnion::parse(&data),
            Err(binparse::ParseError::NotEnoughData {{ .. }})
        ));
    }}

    #[test]
    fn len_bounded_union_truncation_and_dissect_no_panic() {{
        let data = [1, 5, 0xaa, 0x00, 0x10, 0xcc, 0xdd, 0x99];
        assert_eq!(
            BoundedUnion::parse(&data[..5]).map(|_| ()).unwrap_err(),
            binparse::ParseError::NotEnoughData {{ expected: 7, got: 5 }}
        );
        assert_parse_no_panic("BoundedUnion", &data, |d| {{
            let _ = BoundedUnion::parse(d);
        }});
        assert_dissect_no_panic("BoundedUnion", &data, |d| BoundedUnion::dissect(d));
    }}

    #[test]
    fn len_bounded_greedy_array_consumes_window() {{
        let data = [3, 0x11, 0x22, 0x33, 0x77];
        let (mut packet, rem) = BoundedGreedy::parse(&data).unwrap();
        assert!(rem.is_empty());
        let body = packet
            .body()
            .unwrap()
            .collect::<binparse::ParseResult<Vec<_>>>()
            .unwrap();
        assert_eq!(body, vec![0x11, 0x22, 0x33]);
        assert!(packet.body_rest().unwrap().is_empty());
        assert_eq!(packet.after(), 0x77);

        let tree = packet.field_tree();
        let body_node = tree.children.iter().find(|c| c.name == "body").unwrap();
        assert_eq!(body_node.value, binparse::Value::Array);
        assert_eq!(body_node.children.len(), 3);
    }}

    #[test]
    fn len_bounded_until_array_sentinel_in_window_exposes_rest() {{
        let data = [4, 0x11, 0x22, 0x00, 0x44, 0x77];
        let (mut packet, rem) = BoundedUntil::parse(&data).unwrap();
        assert!(rem.is_empty());
        let body = packet
            .body()
            .unwrap()
            .collect::<binparse::ParseResult<Vec<_>>>()
            .unwrap();
        assert_eq!(body, vec![0x11, 0x22]);
        assert_eq!(packet.body_rest().unwrap(), &[0x44]);
        assert_eq!(packet.after(), 0x77);
    }}

    #[test]
    fn len_bounded_until_array_sentinel_missing_in_window_errors() {{
        let data = [3, 0x11, 0x22, 0x33, 0x77];
        assert!(matches!(
            BoundedUntil::parse(&data),
            Err(binparse::ParseError::NotEnoughData {{ .. }})
        ));
        assert_parse_no_panic("BoundedUntil", &data, |d| {{
            let _ = BoundedUntil::parse(d);
            let _ = BoundedUntil::dissect(d);
        }});
    }}

    #[test]
    fn field_tree_reports_names_paths_values_and_ranges() {{
        let data = [
            1, 0x34, 0x12, 0x01, 0x02, 0x03, 0x04, 0b1010_1101, 9, 8, 7, 0xaa, 0x01, 0x02,
            0x78, 0x56, 0x55, 0xcd, 0xab, 0xfe,
        ];
        let (mut packet, _) = Baseline::parse(&data).unwrap();
        let tree = packet.field_tree();
        assert_eq!(tree.name, "Baseline");
        assert_eq!(tree.path, "Baseline");
        assert_eq!(tree.type_name, "Baseline");
        assert_eq!(tree.bit_range, 0..160);
        assert_eq!(tree.byte_range, Some(0..20));
        assert_eq!(tree.value, binparse::Value::Struct);
        assert_eq!(tree.status, binparse::Status::Ok);
        let names: Vec<_> = tree.children.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(
            names,
            ["n", "word", "be", "flag_a", "flag_b", "fixed", "inner", "dyns", "pair", "payload"]
        );

        assert_eq!(tree.children[0].value, binparse::Value::UInt(1));
        assert_eq!(tree.children[0].bit_range, 0..8);
        assert_eq!(tree.children[0].path, "Baseline.n");
        assert_eq!(tree.children[1].value, binparse::Value::UInt(0x1234));
        assert_eq!(tree.children[2].value, binparse::Value::UInt(0x0102_0304));
        assert_eq!(tree.children[3].value, binparse::Value::UInt(5));
        assert_eq!(tree.children[3].bit_range, 56..59);
        assert_eq!(tree.children[3].byte_range, None);
        assert_eq!(tree.children[3].type_name, "b<3>");
        assert_eq!(tree.children[4].value, binparse::Value::UInt(13));

        let fixed = &tree.children[5];
        assert_eq!(fixed.value, binparse::Value::Array);
        assert_eq!(fixed.type_name, "[u8]");
        assert_eq!(fixed.bit_range, 64..88);
        assert_eq!(fixed.byte_range, Some(8..11));
        let elems: Vec<_> = fixed.children.iter().map(|c| c.value.clone()).collect();
        assert_eq!(
            elems,
            [binparse::Value::UInt(9), binparse::Value::UInt(8), binparse::Value::UInt(7)]
        );
        assert_eq!(fixed.children[2].path, "Baseline.fixed.2");
        assert_eq!(fixed.children[2].bit_range, 80..88);

        let mut inner = &tree.children[6];
        assert_eq!(inner.name, "inner");
        assert_eq!(inner.type_name, "Inner");
        assert_eq!(inner.value, binparse::Value::Struct);
        assert_eq!(inner.bit_range, 88..112);
        assert_eq!(inner.children[0].path, "Baseline.inner.a");
        assert_eq!(inner.children[0].value, binparse::Value::UInt(0xaa));
        assert_eq!(inner.children[1].bit_range, 96..112);
        assert_eq!(inner.children[1].value, binparse::Value::UInt(0x0102));

        let dyns = &tree.children[7];
        assert_eq!(dyns.children.len(), 1);
        assert_eq!(dyns.children[0].value, binparse::Value::UInt(0x5678));
        assert_eq!(dyns.children[0].bit_range, 112..128);

        let pair = &tree.children[8];
        assert_eq!(pair.type_name, "concat");
        assert_eq!(pair.value, binparse::Value::Struct);
        assert_eq!(pair.bit_range, 128..152);
        assert_eq!(pair.children[0].name, "pair_0");
        assert_eq!(pair.children[0].value, binparse::Value::UInt(0x55));
        assert_eq!(pair.children[0].path, "Baseline.pair.pair_0");
        assert_eq!(pair.children[1].value, binparse::Value::UInt(0xabcd));
        assert_eq!(pair.children[1].bit_range, 136..152);

        let payload = &tree.children[9];
        assert_eq!(payload.value, binparse::Value::UnionVariant("One"));
        assert_eq!(payload.type_name, "union");
        assert_eq!(payload.bit_range, 152..160);
        assert_eq!(payload.status, binparse::Status::Ok);
        assert_eq!(payload.children.len(), 1);
        let mut one = &payload.children[0];
        assert_eq!(one.name, "One");
        assert_eq!(one.path, "Baseline.payload.One");
        assert_eq!(one.children[0].value, binparse::Value::UInt(0xfe));
        assert_eq!(one.children[0].bit_range, 152..160);
    }}

    #[test]
    fn field_tree_reports_signed_values() {{
        let mut data = vec![0xff, 0xfe, 0xff];
        data.extend((-3i32).to_be_bytes());
        data.extend((-4i64).to_le_bytes());
        data.extend((-5i128).to_le_bytes());
        data.extend(5i16.to_le_bytes());
        data.extend((-5i16).to_le_bytes());
        data.extend([0x7f, 0x80]);
        let (mut packet, _) = Signed::parse(&data).unwrap();
        let tree = packet.field_tree();
        assert_eq!(tree.children[0].value, binparse::Value::Int(-1));
        assert_eq!(tree.children[0].type_name, "i8");
        assert_eq!(tree.children[1].value, binparse::Value::Int(-2));
        assert_eq!(tree.children[4].value, binparse::Value::Int(-5));
        let vals = &tree.children[5];
        assert_eq!(vals.type_name, "[i16]");
        assert_eq!(vals.children[0].value, binparse::Value::Int(5));
        assert_eq!(vals.children[1].value, binparse::Value::Int(-5));
        let small = &tree.children[6];
        assert_eq!(small.children[0].value, binparse::Value::Int(127));
        assert_eq!(small.children[1].value, binparse::Value::Int(-128));
    }}

    #[test]
    fn field_tree_marks_skip_and_padding_hidden() {{
        let data = [0xad, 2, 0xaa, 0xbb, 0x5f];
        let (mut packet, _) = SkipReserved::parse(&data).unwrap();
        let tree = packet.field_tree();
        let names: Vec<_> = tree.children.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["reserved", "flags", "skipped_len", "payload", "pair"]);
        assert!(tree.children[0].hidden);
        assert_eq!(tree.children[0].value, binparse::Value::UInt(5));
        assert!(!tree.children[1].hidden);
        assert!(tree.children[2].hidden);
        let payload = &tree.children[3];
        assert_eq!(payload.children.len(), 2);
        assert_eq!(payload.children[0].value, binparse::Value::UInt(0xaa));
        let pair = &tree.children[4];
        assert!(!pair.children[0].hidden);
        assert!(pair.children[1].hidden);
        assert_eq!(pair.children[1].bit_range, 36..40);

        let data = [1, 0, 0, 2, 0x12, 0x34, 0x56, 0x78];
        let (mut packet, _) = Padded::parse(&data).unwrap();
        let tree = packet.field_tree();
        let names: Vec<_> = tree.children.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["a", "b_pad", "b", "c", "d"]);
        let pad = &tree.children[1];
        assert!(pad.hidden);
        assert_eq!(pad.bit_range, 8..24);
        assert_eq!(pad.value, binparse::Value::Bytes(&[0u8, 0]));
    }}

    #[test]
    fn field_tree_len_bounded_includes_rest_and_error_status() {{
        let data = [7, 5, 0x01, 0x02, 0x03, 0xaa, 0xbb, 0x99];
        let (mut packet, _) = Bounded::parse(&data).unwrap();
        let tree = packet.field_tree();
        let value = &tree.children[2];
        assert_eq!(value.name, "value");
        assert_eq!(value.type_name, "Inner");
        assert_eq!(value.bit_range, 16..56);
        let names: Vec<_> = value.children.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["a", "b", "rest"]);
        assert_eq!(value.children[1].value, binparse::Value::UInt(0x0203));
        let rest = &value.children[2];
        assert_eq!(rest.value, binparse::Value::Bytes(&[0xaau8, 0xbb]));
        assert_eq!(rest.bit_range, 40..56);
        assert_eq!(rest.path, "Bounded.value.rest");

        let data = [7, 2, 0x01, 0x02, 0x99];
        let (mut packet, _) = Bounded::parse(&data).unwrap();
        let tree = packet.field_tree();
        let value = &tree.children[2];
        assert_eq!(value.value, binparse::Value::Opaque);
        assert_eq!(value.bit_range, 16..32);
        assert_eq!(
            value.status,
            binparse::Status::Error(binparse::ParseError::NotEnoughData {{
                expected: 3,
                got: 2,
            }})
        );
        assert_eq!(tree.children[3].value, binparse::Value::UInt(0x99));
    }}

    #[test]
    fn field_tree_conditional_present_emits_real_node() {{
        let data = [0x46, 0xaa, 0xbb, 0xcc, 0xdd, 0x11];
        let (mut packet, _) = Ipv4WithOptions::parse(&data).unwrap();
        let tree = packet.field_tree();
        let names: Vec<_> = tree.children.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["version", "ihl", "options", "proto"]);
        let options = &tree.children[2];
        assert_eq!(options.value, binparse::Value::Array);
        assert_eq!(options.type_name, "[u8]");
        assert_eq!(options.bit_range, 8..40);
        assert!(!options.hidden);
        let elems: Vec<_> = options.children.iter().map(|c| c.value.clone()).collect();
        assert_eq!(
            elems,
            [
                binparse::Value::UInt(0xaa),
                binparse::Value::UInt(0xbb),
                binparse::Value::UInt(0xcc),
                binparse::Value::UInt(0xdd),
            ]
        );
        assert_eq!(options.children[0].path, "Ipv4WithOptions.options.0");
        assert_eq!(tree.children[3].value, binparse::Value::UInt(0x11));
    }}

    #[test]
    fn field_tree_conditional_absent_emits_hidden_absent_node() {{
        let data = [0x45, 0x11];
        let (mut packet, _) = Ipv4WithOptions::parse(&data).unwrap();
        let tree = packet.field_tree();
        let names: Vec<_> = tree.children.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["version", "ihl", "options", "proto"]);
        let options = &tree.children[2];
        assert_eq!(options.value, binparse::Value::Absent);
        assert_eq!(options.status, binparse::Status::Ok);
        assert!(options.hidden);
        assert_eq!(options.bit_range, 8..8);
        assert!(options.children.is_empty());
        assert_eq!(tree.children[3].value, binparse::Value::UInt(0x11));
    }}

    #[test]
    fn field_tree_conditional_with_else_picks_branch() {{
        let then_data = [1, 7, 9];
        let (mut packet, _) = CondElse::parse(&then_data).unwrap();
        let tree = packet.field_tree();
        let names: Vec<_> = tree.children.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["kind", "small", "big", "tail"]);
        assert_eq!(tree.children[1].value, binparse::Value::UInt(7));
        assert!(!tree.children[1].hidden);
        assert_eq!(tree.children[2].value, binparse::Value::Absent);
        assert!(tree.children[2].hidden);
        assert_eq!(tree.children[3].value, binparse::Value::UInt(9));

        let else_data = [0, 0x12, 0x34, 9];
        let (mut packet, _) = CondElse::parse(&else_data).unwrap();
        let tree = packet.field_tree();
        assert_eq!(tree.children[1].value, binparse::Value::Absent);
        assert!(tree.children[1].hidden);
        assert_eq!(tree.children[2].value, binparse::Value::UInt(0x1234));
        assert!(!tree.children[2].hidden);
        assert_eq!(tree.children[3].value, binparse::Value::UInt(9));
    }}

    #[test]
    fn field_tree_nested_conditional_struct_ref() {{
        let too_many = [0u8; 9];
        let (mut packet, _) = Opts::parse(&too_many).unwrap();
        let tree = packet.field_tree();
        let opts = &tree.children[0];
        let first = &opts.children[0];
        assert_eq!(first.value, binparse::Value::Struct);
        let body = &first.children[1];
        assert_eq!(body.name, "body");
        assert_eq!(body.value, binparse::Value::Absent);
        assert!(body.hidden);
    }}

    #[test]
    fn field_tree_hooks_emit_value_and_error_nodes() {{
        let data = [3, 0, 2, b'h', b'i', 0];
        let (mut packet, _) = Hooked::parse(&data).unwrap();
        let tree = packet.field_tree();
        let names: Vec<_> = tree.children.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["prefix", "value", "name"]);
        assert_eq!(tree.children[1].value, binparse::Value::UInt(4));
        assert_eq!(tree.children[1].type_name, "u32");
        assert_eq!(tree.children[1].status, binparse::Status::Ok);
        assert_eq!(tree.children[2].value, binparse::Value::Opaque);
        assert_eq!(tree.children[2].type_name, "String");
        assert_eq!(tree.children[2].bit_range, 24..48);
        assert_eq!(tree.children[2].status, binparse::Status::Ok);
    }}

    #[test]
    fn field_tree_vla_hook_reports_uint_and_range() {{
        let data = [7, 0xE5, 0x8E, 0x26, 9];
        let (mut packet, _) = Varint::parse(&data).unwrap();
        let tree = packet.field_tree();
        let value = &tree.children[1];
        assert_eq!(value.name, "value");
        assert_eq!(value.type_name, "u64");
        assert_eq!(value.value, binparse::Value::UInt(624485));
        assert_eq!(value.bit_range, 8..32);
        assert_eq!(value.status, binparse::Status::Ok);
    }}

    #[test]
    fn field_tree_dns_hook_reports_consumed_range() {{
        let data = [
            0xAB, 0xCD,
            3, b'a', b'b', b'c', 2, b'd', b'e', 0,
            0x00, 0x01,
            0xC0, 0x02,
            0x00, 0x1C,
        ];
        let (mut packet, _) = DnsMsg::parse(&data).unwrap();
        let tree = packet.field_tree();
        let qname = &tree.children[1];
        assert_eq!(qname.type_name, "String");
        assert_eq!(qname.value, binparse::Value::Opaque);
        assert_eq!(qname.bit_range, 16..80);
        assert_eq!(qname.status, binparse::Status::Ok);
        let aname = &tree.children[3];
        assert_eq!(aname.bit_range, 96..112);
        assert_eq!(aname.status, binparse::Status::Ok);
    }}

    #[test]
    fn field_tree_union_dynamic_variant() {{
        let data = [1, 3, 0xaa, 0xbb, 0xcc, 0x99];
        let (mut packet, _) = Dispatch::parse(&data).unwrap();
        let tree = packet.field_tree();
        let body = &tree.children[1];
        assert_eq!(body.name, "body");
        assert_eq!(body.type_name, "union");
        assert_eq!(body.value, binparse::Value::UnionVariant("Msg"));
        assert_eq!(body.bit_range, 8..40);
        assert_eq!(body.status, binparse::Status::Ok);
        assert_eq!(body.children.len(), 1);
        let msg = &body.children[0];
        assert_eq!(msg.name, "Msg");
        assert_eq!(msg.path, "Dispatch.body.Msg");
        assert_eq!(msg.children[0].value, binparse::Value::UInt(3));
        let mut inner = &msg.children[1];
        assert_eq!(inner.value, binparse::Value::Array);
        let elems: Vec<_> = inner.children.iter().map(|c| c.value.clone()).collect();
        assert_eq!(
            elems,
            [
                binparse::Value::UInt(0xaa),
                binparse::Value::UInt(0xbb),
                binparse::Value::UInt(0xcc),
            ]
        );
    }}

    #[test]
    fn field_tree_union_error_arm_surfaces_failed_status() {{
        let data = [9, 0xaa];
        let (mut packet, _) = Dispatch::parse(&data).unwrap();
        let tree = packet.field_tree();
        let body = &tree.children[1];
        assert_eq!(body.value, binparse::Value::Opaque);
        assert_eq!(body.type_name, "union");
        assert_eq!(body.bit_range, 8..8);
        assert_eq!(body.status, binparse::Status::Failed("UNKNOWN_KIND"));
        assert!(body.children.is_empty());
    }}

    #[test]
    fn field_tree_union_tuple_dispatch() {{
        let echo = [8, 0, 0x12, 0x34, 0x00, 0x01];
        let (mut packet, _) = Icmp::parse(&echo).unwrap();
        let tree = packet.field_tree();
        let body = &tree.children[2];
        assert_eq!(body.value, binparse::Value::UnionVariant("Echo"));
        assert_eq!(body.bit_range, 16..48);
        let echo_node = &body.children[0];
        assert_eq!(echo_node.name, "Echo");
        assert_eq!(echo_node.children[0].value, binparse::Value::UInt(0x1234));
        assert_eq!(echo_node.children[1].value, binparse::Value::UInt(1));

        let raw = [42, 7, 0xff];
        let (mut packet, _) = Icmp::parse(&raw).unwrap();
        let tree = packet.field_tree();
        let body = &tree.children[2];
        assert_eq!(body.value, binparse::Value::UnionVariant("Raw"));
        assert_eq!(body.children[0].name, "Raw");
    }}

    #[test]
    fn field_tree_union_inside_concat() {{
        let data = [1, 2, 0x42, 0x12, 0x34, 2, 0xaa, 0xbb, 0x99];
        let (mut packet, _) = ConcatUnion::parse(&data).unwrap();
        let tree = packet.field_tree();
        let pair = &tree.children[2];
        assert_eq!(pair.type_name, "concat");
        assert_eq!(pair.children.len(), 3);
        assert_eq!(pair.children[0].value, binparse::Value::UInt(0x42));
        let word = &pair.children[1];
        assert_eq!(word.value, binparse::Value::UnionVariant("Word"));
        assert_eq!(word.children[0].name, "Word");
        assert_eq!(word.children[0].children[0].value, binparse::Value::UInt(0x1234));
        let bytes = &pair.children[2];
        assert_eq!(bytes.value, binparse::Value::UnionVariant("Bytes"));
        let bytes_inner = &bytes.children[0];
        assert_eq!(bytes_inner.name, "Bytes");
        assert_eq!(bytes_inner.children[0].value, binparse::Value::UInt(2));
    }}

    #[test]
    fn field_tree_until_greedy_and_struct_arrays() {{
        let data = [b'h', b'i', 0, 7];
        let (mut packet, _) = CStr::parse(&data).unwrap();
        let tree = packet.field_tree();
        let name = &tree.children[0];
        assert_eq!(name.bit_range, 0..24);
        assert_eq!(name.children.len(), 2);
        assert_eq!(name.children[1].value, binparse::Value::UInt(b'i' as u128));

        let data = [9, 0x12, 0x34, 0x56, 0x78];
        let (mut packet, _) = RestWide::parse(&data).unwrap();
        let tree = packet.field_tree();
        let words = &tree.children[1];
        assert_eq!(words.bit_range, 8..40);
        let elems: Vec<_> = words.children.iter().map(|c| c.value.clone()).collect();
        assert_eq!(elems, [binparse::Value::UInt(0x1234), binparse::Value::UInt(0x5678)]);

        let data = [1, 0x02, 0x03, 4, 0x05, 0x06];
        let (mut packet, _) = GreedyStructs::parse(&data).unwrap();
        let tree = packet.field_tree();
        let items = &tree.children[0];
        assert_eq!(items.children.len(), 2);
        assert_eq!(items.children[1].bit_range, 24..48);
        assert_eq!(items.children[1].children[1].value, binparse::Value::UInt(0x0506));
        assert_eq!(items.children[1].children[1].path, "GreedyStructs.items.1.b");
    }}

    #[test]
    fn field_tree_reports_element_errors_without_panicking() {{
        let too_many = [0u8; 9];
        let (mut packet, _) = Opts::parse(&too_many).unwrap();
        let tree = packet.field_tree();
        let opts = &tree.children[0];
        assert_eq!(opts.children.len(), 9);
        assert_eq!(opts.children[0].type_name, "Opt");
        assert_eq!(opts.children[0].value, binparse::Value::Struct);
        assert_eq!(opts.children[0].path, "Opts.opts.0");
        assert_eq!(
            opts.children[8].status,
            binparse::Status::Error(binparse::ParseError::MaxIterationsExceeded {{
                field: "Opts.opts",
                max: 8,
            }})
        );
    }}

    fn eth_ip_udp_frame() -> Vec<u8> {{
        let mut frame = vec![
            0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
            0x00, 0x0b, 0x82, 0x01, 0xfc, 0x42,
            0x08, 0x00,
        ];
        frame.extend([
            0x45, 0x00, 0x00, 0x20, 0x1c, 0x46, 0x40, 0x00, 0x40, 0x11, 0x00, 0x00,
            0xac, 0x10, 0x0a, 0x63, 0xac, 0x10, 0x0a, 0x0c,
        ]);
        frame.extend([
            0xc3, 0x50, 0x00, 0x35, 0x00, 0x0c, 0x1a, 0x2b, 0xde, 0xad, 0xbe, 0xef,
        ]);
        frame
    }}

    #[test]
    fn ethernet_handoff_exposes_ethertype_and_payload() {{
        let frame = eth_ip_udp_frame();
        let (mut eth, _) = Eth::parse(&frame).unwrap();
        let handoff = eth.handoff().expect("ethernet declares a payload");
        assert_eq!(handoff.keys, vec![0x0800]);
        assert_eq!(handoff.payload_byte_range, 14..frame.len());
        assert_eq!(handoff.payload, &frame[14..]);
        assert_parse_no_panic("Eth handoff", &frame, |data| {{
            if let Ok((mut eth, _)) = Eth::parse(data) {{
                let _ = eth.handoff();
            }}
        }});
    }}

    #[test]
    fn ipv4_handoff_payload_starts_after_dynamic_options() {{
        let mut packet = vec![
            0x46, 0x00, 0x00, 0x24, 0x1c, 0x46, 0x40, 0x00, 0x40, 0x11, 0x00, 0x00,
            0xac, 0x10, 0x0a, 0x63, 0xac, 0x10, 0x0a, 0x0c,
            0x94, 0x04, 0x00, 0x00,
        ];
        packet.extend([
            0xc3, 0x50, 0x00, 0x35, 0x00, 0x0c, 0x1a, 0x2b, 0xde, 0xad, 0xbe, 0xef,
        ]);
        let (mut ip, _) = Ip4::parse(&packet).unwrap();
        assert_eq!(ip.ihl(), 6);
        let handoff = ip.handoff().expect("ipv4 declares a payload");
        assert_eq!(handoff.keys, vec![17]);
        assert_eq!(handoff.payload_byte_range, 24..packet.len());
        assert_eq!(handoff.payload, &packet[24..]);
        assert_parse_no_panic("Ip4 handoff", &packet, |data| {{
            if let Ok((mut ip, _)) = Ip4::parse(data) {{
                let _ = ip.handoff();
            }}
        }});
    }}

    #[test]
    fn udp_handoff_exposes_two_discriminators() {{
        let datagram = vec![
            0xc3, 0x50, 0x00, 0x35, 0x00, 0x0c, 0x1a, 0x2b, 0xde, 0xad, 0xbe, 0xef,
        ];
        let (mut udp, _) = Udp4::parse(&datagram).unwrap();
        let handoff = udp.handoff().expect("udp declares a payload");
        assert_eq!(handoff.keys, vec![50000, 53]);
        assert_eq!(handoff.payload_byte_range, 8..12);
        assert_eq!(handoff.payload, &[0xde, 0xad, 0xbe, 0xef]);
    }}

    #[test]
    fn struct_without_payload_has_no_handoff() {{
        let data = [0x01, 0x02, 0x03];
        let (mut packet, _) = NoHandoff::parse(&data).unwrap();
        assert!(packet.handoff().is_none());
        let dissect: &mut dyn binparse::Dissect = &mut packet;
        assert!(dissect.handoff().is_none());
    }}

    fn parse_for_key(key: u128, payload: &[u8]) -> Option<binparse::Handoff<'_>> {{
        match key {{
            0x0800 => Ip4::parse(payload).ok().and_then(|(mut p, _)| p.handoff()),
            17 => Udp4::parse(payload).ok().and_then(|(mut p, _)| p.handoff()),
            _ => None,
        }}
    }}

    #[test]
    fn chain_eth_ipv4_udp_via_handoff_only() {{
        let frame = eth_ip_udp_frame();
        let (mut eth, _) = Eth::parse(&frame).unwrap();
        let first = eth.handoff().expect("ethernet payload");

        let mut keys = first.keys;
        let mut payload: &[u8] = first.payload;
        while let Some(key) = keys.first().copied() {{
            match parse_for_key(key, payload) {{
                Some(next) => {{
                    keys = next.keys;
                    payload = next.payload;
                }}
                None => break,
            }}
        }}

        assert_eq!(keys, vec![50000, 53]);
        assert_eq!(payload, &[0xde, 0xad, 0xbe, 0xef]);
    }}

    #[test]
    fn struct_level_len_exact_fit() {{
        let data = [0x00, 0x05, 0xaa, 0xbb, 0xcc];
        let (mut packet, rem) = BoundedFill::parse(&data).unwrap();
        assert!(rem.is_empty());
        assert_eq!(packet.total_len(), 5);
        let payload = packet
            .payload()
            .unwrap()
            .collect::<binparse::ParseResult<Vec<_>>>()
            .unwrap();
        assert_eq!(payload, vec![0xaa, 0xbb, 0xcc]);
        assert_parse_no_panic("BoundedFill", &data, |data| {{
            if let Ok((mut packet, _)) = BoundedFill::parse(data) {{
                let _ = packet.field_tree();
            }}
        }});
    }}

    #[test]
    fn struct_level_len_bound_greater_than_fields_leaves_trailing_and_advances_parent() {{
        let data = [0x00, 0x06, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff];
        let (mut packet, rem) = BoundedFill::parse(&data).unwrap();
        assert_eq!(packet.total_len(), 6);
        let payload = packet
            .payload()
            .unwrap()
            .collect::<binparse::ParseResult<Vec<_>>>()
            .unwrap();
        assert_eq!(payload, vec![0xaa, 0xbb, 0xcc, 0xdd]);
        assert_eq!(rem, &[0xee, 0xff]);
        let tree = packet.field_tree();
        assert_eq!(tree.bit_range, 0..48);
    }}

    #[test]
    fn struct_level_len_truncated_by_parent_errors() {{
        let data = [0x00, 0x08, 0xaa, 0xbb];
        assert!(matches!(
            BoundedFill::parse(&data),
            Err(binparse::ParseError::NotEnoughData {{ .. }})
        ));
        let tree = BoundedFill::dissect(&data);
        assert!(matches!(tree.status, binparse::Status::Error(_)));
        assert_parse_no_panic("BoundedFill trunc", &data, |data| {{
            let _ = BoundedFill::parse(data);
            let _ = BoundedFill::dissect(data);
        }});
    }}

    #[test]
    fn struct_level_len_trailing_inside_bound_exposed_and_advances() {{
        let data = [0x06, 0x12, 0x34, 0xaa, 0xbb, 0xcc, 0xff];
        let (mut packet, rem) = BoundedGap::parse(&data).unwrap();
        assert_eq!(packet.cap(), 6);
        assert_eq!(packet.value(), 0x1234);
        assert_eq!(rem, &[0xff]);
        let tree = packet.field_tree();
        assert_eq!(tree.bit_range, 0..48);
        let trailing = tree
            .children
            .iter()
            .find(|child| child.name == "trailing")
            .expect("trailing node inside bound");
        assert_eq!(trailing.bit_range, 24..48);
        assert_eq!(trailing.value, binparse::Value::Bytes(&[0xaa, 0xbb, 0xcc]));
    }}

    #[test]
    fn struct_level_len_u16_fill_divisible() {{
        let data = [0x00, 0x06, 0x12, 0x34, 0x56, 0x78];
        let (mut packet, rem) = BoundedU16Fill::parse(&data).unwrap();
        assert!(rem.is_empty());
        let words = packet
            .words()
            .unwrap()
            .collect::<binparse::ParseResult<Vec<_>>>()
            .unwrap();
        assert_eq!(words, vec![0x1234, 0x5678]);
    }}

    #[test]
    fn struct_level_len_u16_fill_indivisible_tail_errors() {{
        let data = [0x00, 0x07, 0x12, 0x34, 0x56, 0x78, 0x9a];
        assert!(matches!(
            BoundedU16Fill::parse(&data),
            Err(binparse::ParseError::NotEnoughData {{ .. }})
        ));
        assert_parse_no_panic("BoundedU16Fill", &data, |data| {{
            let _ = BoundedU16Fill::parse(data);
        }});
    }}

    #[test]
    fn struct_level_len_nested_offsets_advance_by_bound() {{
        let data = [0x03, 0xaa, 0xbb, 0x42];
        let (mut packet, rem) = BoundedNested::parse(&data).unwrap();
        assert!(rem.is_empty());
        let mut inner = packet.inner().unwrap();
        assert_eq!(inner.n(), 3);
        let body = inner
            .body()
            .unwrap()
            .collect::<binparse::ParseResult<Vec<_>>>()
            .unwrap();
        assert_eq!(body, vec![0xaa, 0xbb]);
        assert_eq!(packet.after(), 0x42);
        assert_parse_no_panic("BoundedNested", &data, |data| {{
            if let Ok((mut packet, _)) = BoundedNested::parse(data) {{
                let _ = packet.field_tree();
            }}
        }});
    }}

    #[test]
    fn struct_level_len_inside_union_variant() {{
        let data = [0x01, 0x02, 0xaa, 0x99];
        let (mut packet, rem) = StructLenUnion::parse(&data).unwrap();
        assert!(rem.is_empty());
        assert_eq!(packet.tag(), 1);
        match packet.body().unwrap() {{
            StructLenUnion_body::Sized(mut sized) => {{
                let mut inner = sized.inner().unwrap();
                assert_eq!(inner.n(), 2);
            }}
            _ => panic!("expected Sized variant"),
        }}
        assert_eq!(packet.trailer(), 0x99);
    }}
}}
"#
        ),
    )
    .expect("failed to write runtime lib.rs");

    test_dir
}

#[test]
fn generated_code_compiles_and_handles_runtime_baseline() {
    let dsl = r#"
struct Inner {
    a: u8,
    b: u16,
}

@endian(little)
struct Baseline {
    n: u8,
    word: u16,
    @endian(big) be: u32,
    flag_a: b<3>,
    flag_b: b<5>,
    fixed: [u8; 3],
    inner: Inner,
    dyns: [u16; n],
    pair: concat(u8, u16),
    payload: union(n) {
        1 => One { x: u8 },
        _ => Unknown { },
    },
}

struct Hooked {
    prefix: u8,
    @hook(double_it, u32)
    value: u16,
    @hook(parse_cstring, String)
    name: [u8],
}

struct StructArray {
    count: u8,
    items: [Inner; count],
}

struct CrossByte {
    high: b<5>,
    mid: b<6>,
    low: b<5>,
}

struct Huge {
    n: u64,
    xs: [u128; n],
}

struct SizeExpr {
    n: u64,
    xs: [u8; n * 2],
}

@endian(little)
struct Signed {
    a: i8,
    b: i16,
    @endian(big) c: i32,
    d: i64,
    e: i128,
    vals: [i16; 2],
    small: [i8; 2],
}

struct Ipv4Start {
    version: b<4>,
    ihl: b<4>,
}

struct TcpFlags {
    data_offset: b<4>,
    reserved: b<3>,
    ns: b<1>,
    cwr: b<1>,
    ece: b<1>,
    urg: b<1>,
    ack: b<1>,
    psh: b<1>,
    rst: b<1>,
    syn: b<1>,
    fin: b<1>,
}

@bit_order(lsb)
struct LsbFlags {
    low: b<3>,
    high: b<5>,
    @bit_order(msb) top: b<4>,
    @bit_order(msb) bottom: b<4>,
}

struct Validated {
    magic = x89504e47,
    @check(version == 4) version: b<4>,
    ihl: b<4>,
    @range(20, 60) total_len: u16,
    reserved = b00,
    @check(flags <= 3) flags: b<6>,
}

struct Ipv4WithOptions {
    version: b<4>,
    ihl: b<4>,
    if (ihl > 5) {
        options: [u8; (ihl - 5) * 4],
    }
    proto: u8,
}

struct TcpStart {
    data_offset: b<4>,
    reserved: b<4>,
    if (data_offset > 5) {
        options: [u8; (data_offset - 5) * 4],
    }
}

struct CondElse {
    kind: u8,
    if (kind == 1) {
        small: u8,
    } else {
        big: u16,
    }
    tail: u8,
}

struct Rest {
    n: u8,
    @greedy(unsafe_eof) tail: [u8],
}

struct RestWide {
    n: u8,
    @greedy(unsafe_eof) words: [u16],
}

struct CStr {
    @until(x00) name: [u8],
    after: u8,
}

struct GreedyStructs {
    @greedy(unsafe_eof) items: [Inner],
}

struct Capped {
    len: u8,
    @max_iter(4) vals: [u8; len],
}

struct Opt {
    kind: u8,
    if (kind > 0) {
        body: u8,
    }
}

struct Opts {
    @greedy(unsafe_eof) @max_iter(8) opts: [Opt],
}

struct Padded {
    a: u8,
    @pad(2) b: u8,
    @pad_to(4) c: u16,
    @align(2) d: u16,
}

struct DynPadded {
    n: u8,
    data: [u8; n],
    @pad_to(4) tail: u8,
}

struct DynAligned {
    n: u8,
    data: [u8; n],
    @align(2) word: u16,
}

struct SkipReserved {
    @skip reserved: b<3>,
    flags: b<5>,
    @skip skipped_len: u8,
    payload: [u8; skipped_len],
    pair: concat(b<4>, @skip b<4>),
}

error {
    UNKNOWN_KIND { kind: u8 },
}

struct Icmp {
    icmp_type: u8,
    code: u8,
    body: union(icmp_type, code) {
        (0, 0) | (8, 0) => Echo { id: u16, seq: u16 },
        (3, _) => DestUnreach { unused: u32 },
        (_, _) => Raw { },
    },
}

struct Dispatch {
    kind: u8,
    body: union(kind) {
        1 => Msg { len: u8, data: [u8; len] },
        2 => Checked { version = 4 },
        _ => @error(UNKNOWN_KIND { kind: kind }),
    },
}

struct ConcatUnion {
    a: u8,
    b: u8,
    pair: concat(
        u8,
        union(a) { 1 => Word { w: u16 }, _ => Empty { } },
        union(b) { 2 => Bytes { n: u8, data: [u8; n] }, _ => Skip { } }
    ),
    tail: u8,
}

struct Bounded {
    tag: u8,
    len: u8,
    @len(len) value: Inner,
    after: u8,
}

struct BoundedUnion {
    tag: u8,
    len: u8,
    @len(len) value: union(tag) {
        1 => Pair { inner: Inner },
        2 => Blob { @greedy(unsafe_eof) bytes: [u8] },
        _ => Unknown { },
    },
    after: u8,
}

struct BoundedGreedy {
    len: u8,
    @len(len) @greedy(unsafe_eof) body: [u8],
    after: u8,
}

struct BoundedUntil {
    len: u8,
    @len(len) @until(x00) body: [u8],
    after: u8,
}

struct Varint {
    tag: u8,
    @hook(read_leb128, u64) value: [u8],
    after: u8,
}

struct Lying {
    @hook(lying_hook, u8) v: [u8],
}

struct FailTransform {
    prefix: u8,
    @hook(failing_transform, u32) value: u16,
    suffix: u8,
}

struct LenVarint {
    len: u8,
    @len(len) @hook(read_leb128, u64) value: [u8],
    after: u8,
}

struct LenDns {
    id: u16,
    @len(8) @hook(parse_dns_name, String) qname: [u8],
    @hook(parse_dns_name, String) aname: [u8],
    atype: u16,
}

struct CondHook {
    kind: u8,
    if (kind == 1) {
        @hook(double_it, u32) doubled: u16,
        @hook(parse_cstring, String) name: [u8],
    }
    tail: u8,
}

struct DnsMsg {
    id: u16,
    @hook(parse_dns_name, String) qname: [u8],
    qtype: u16,
    @hook(parse_dns_name, String) aname: [u8],
    atype: u16,
}

struct Cached {
    count: u8,
    @hook(counting_cstring, String) @cache(len) body: [u8],
    a: u16,
    b: u16,
    c: u16,
}

struct CachedValue {
    count: u8,
    @hook(counting_value_cstring, String) @cache(value) body: [u8],
    a: u16,
}

struct CachedLenValue {
    count: u8,
    @hook(counting_len_value_cstring, String) @cache body: [u8],
    a: u16,
}

struct CachedFixedHook {
    prefix: u8,
    @hook(counting_double_it, u32) @cache(value) value: u16,
    suffix: u8,
}

struct CachedUnion {
    ty: u8,
    len: u8,
    @len(len) @cache body: union(ty) {
        1 => Msg {
            @hook(counting_union_cstring, String) @cache value: [u8],
        },
        _ => Raw { },
    },
    trailer: u8,
}

struct Eth {
    dst: [u8; 6],
    src: [u8; 6],
    @discriminator ethertype: u16,
    @greedy(unsafe_eof) @payload payload: [u8],
}

struct Ip4 {
    @check(version == 4) version: b<4>,
    @range(5, 15) ihl: b<4>,
    dscp: b<6>,
    ecn: b<2>,
    @range(20, 65535) total_len: u16,
    ident: u16,
    flags: b<3>,
    frag_hi: b<5>,
    frag_lo: u8,
    ttl: u8,
    @discriminator proto: u8,
    checksum: u16,
    src: [u8; 4],
    dst: [u8; 4],
    if (ihl > 5) {
        options: [u8; (ihl - 5) * 4],
    }
    @payload payload: [u8; total_len - (ihl * 4)],
}

struct Udp4 {
    @discriminator src_port: u16,
    @discriminator dst_port: u16,
    @range(8, 65535) length: u16,
    checksum: u16,
    @payload payload: [u8; length - 8],
}

struct NoHandoff {
    a: u8,
    b: u16,
}

@len(total_len)
struct BoundedFill {
    total_len: u16,
    payload: [u8],
}

@len(total_len)
struct BoundedU16Fill {
    total_len: u16,
    words: [u16],
}

@len(cap)
struct BoundedGap {
    cap: u8,
    value: u16,
}

@len(n)
struct BoundedInner {
    n: u8,
    @greedy(unsafe_eof) body: [u8],
}

struct BoundedNested {
    inner: BoundedInner,
    after: u8,
}

struct StructLenUnion {
    tag: u8,
    body: union(tag) {
        1 => Sized { inner: BoundedInner },
        _ => Raw { },
    },
    trailer: u8,
}
"#;

    let code = generated_code(dsl);
    let test_dir = write_runtime_crate(&code);
    let output = Command::new("cargo")
        .arg("test")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(test_dir.join("Cargo.toml"))
        .output()
        .expect("failed to run generated runtime tests");

    assert!(
        output.status.success(),
        "generated runtime tests failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
