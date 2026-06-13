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
fn double_it(value: u16) -> u32 {{
    u32::from(value) * 2
}}

fn parse_cstring(data: &[u8], ctx: binparse::HookContext<'_>) -> binparse::ParseResult<(String, usize)> {{
    binparse::hooks::cstring(data, ctx)
}}

fn read_leb128(data: &[u8], ctx: binparse::HookContext<'_>) -> binparse::ParseResult<(u64, usize)> {{
    binparse::hooks::leb128_unsigned(data, ctx)
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

    #[test]
    fn baseline_valid_packet_decodes() {{
        let data = [
            1, 0x34, 0x12, 0x01, 0x02, 0x03, 0x04, 0b1010_1101, 9, 8, 7, 0xaa, 0x01, 0x02,
            0x78, 0x56, 0x55, 0xcd, 0xab, 0xfe,
        ];
        let (packet, rem) = Baseline::parse(&data).unwrap();
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

        let inner = packet.inner().unwrap();
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
            Baseline_payload::One(one) => assert_eq!(one.x(), 0xfe),
            Baseline_payload::Unknown(_) => panic!("expected One payload"),
        }}
    }}

    #[test]
    fn offsets_report_absolute_bit_ranges() {{
        let data = [
            1, 0x34, 0x12, 0x01, 0x02, 0x03, 0x04, 0b1010_1101, 9, 8, 7, 0xaa, 0x01, 0x02,
            0x78, 0x56, 0x55, 0xcd, 0xab, 0xfe,
        ];
        let (packet, _) = Baseline::parse(&data).unwrap();
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

        let inner = packet.inner().unwrap();
        assert_eq!(inner.a_bit_range(), 0..8);
        assert_eq!(inner.b_bit_range(), 8..24);
        let inner_base = packet.inner_start_offset().bits();
        assert_eq!(inner_base + inner.b_bit_range().start, 96);
        assert_eq!(inner_base + inner.b_bit_range().end, 112);
    }}

    #[test]
    fn cross_byte_bitfield_offsets_and_values() {{
        let data = [0xad, 0xad];
        let (packet, rem) = CrossByte::parse(&data).unwrap();
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
            let _ = CrossByte::parse(data);
        }});
    }}

    #[test]
    fn size_expression_valid_packet_decodes() {{
        let data = [0, 0, 0, 0, 0, 0, 0, 2, 1, 2, 3, 4];
        let (packet, rem) = SizeExpr::parse(&data).unwrap();
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
            let _ = SizeExpr::parse(data);
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
            let _ = Huge::parse(data);
        }});
    }}

    #[test]
    fn baseline_parse_does_not_panic_on_truncation() {{
        let data = [
            1, 0x34, 0x12, 0x01, 0x02, 0x03, 0x04, 0b1010_1101, 9, 8, 7, 0xaa, 0x01, 0x02,
            0x78, 0x56, 0x55, 0xcd, 0xab, 0xfe,
        ];
        assert_parse_no_panic("Baseline", &data, |data| {{
            let _ = Baseline::parse(data);
        }});
    }}

    #[test]
    fn hooks_decode_and_do_not_panic_on_truncation() {{
        let data = [3, 0, 2, b'h', b'i', 0];
        let (packet, rem) = Hooked::parse(&data).unwrap();
        assert!(rem.is_empty());
        assert_eq!(packet.prefix(), 3);
        assert_eq!(packet.value(), 4);
        assert_eq!(packet.name().unwrap(), "hi");
        assert_eq!(packet.value_bit_range(), 8..24);
        assert_eq!(packet.name_bit_range(), 24..48);
        assert!(matches!(
            Hooked::parse(&[3, 0, 2, b'h', b'i']),
            Err(binparse::ParseError::NotEnoughData {{ .. }})
        ));
        assert_parse_no_panic("Hooked", &data, |data| {{
            let _ = Hooked::parse(data);
        }});
    }}

    #[test]
    fn leb128_hook_decodes_and_errors_propagate() {{
        let data = [7, 0xE5, 0x8E, 0x26, 9];
        let (packet, rem) = Varint::parse(&data).unwrap();
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
            let _ = Varint::parse(data);
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
            let _ = Lying::parse(data);
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
        let (packet, rem) = DnsMsg::parse(&data).unwrap();
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
            let _ = DnsMsg::parse(data);
        }});
    }}

    #[test]
    fn struct_array_decodes_and_does_not_panic_on_truncation() {{
        let data = [2, 1, 0x02, 0x03, 4, 0x05, 0x06];
        let (packet, rem) = StructArray::parse(&data).unwrap();
        assert!(rem.is_empty());
        let items = packet
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
            let _ = StructArray::parse(data);
        }});

        let short = [10, 1, 0x02];
        assert_parse_no_panic("StructArray short", &short, |data| {{
            let _ = StructArray::parse(data);
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
        let (packet, rem) = Signed::parse(&data).unwrap();
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
            let _ = Signed::parse(data);
        }});
    }}

    #[test]
    fn ipv4_version_and_ihl_decode_msb_first() {{
        let data = [0x45];
        let (packet, rem) = Ipv4Start::parse(&data).unwrap();
        assert!(rem.is_empty());
        assert_eq!(packet.version(), 4);
        assert_eq!(packet.ihl(), 5);
        assert_eq!(packet.version_bit_range(), 0..4);
        assert_eq!(packet.ihl_bit_range(), 4..8);
        assert_parse_no_panic("Ipv4Start", &data, |data| {{
            let _ = Ipv4Start::parse(data);
        }});
    }}

    #[test]
    fn tcp_flags_decode_without_hooks() {{
        let data = [0x50, 0b0001_1000];
        let (packet, rem) = TcpFlags::parse(&data).unwrap();
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
            let _ = TcpFlags::parse(data);
        }});
    }}

    #[test]
    fn validated_packet_decodes() {{
        let data = [0x89, 0x50, 0x4e, 0x47, 0x45, 0x00, 0x14, 0b00_000011];
        let (packet, rem) = Validated::parse(&data).unwrap();
        assert!(rem.is_empty());
        assert_eq!(packet.magic(), 0x89504e47);
        assert_eq!(packet.version(), 4);
        assert_eq!(packet.ihl(), 5);
        assert_eq!(packet.total_len(), 20);
        assert_eq!(packet.reserved(), 0);
        assert_eq!(packet.flags(), 3);
        assert_parse_no_panic("Validated", &data, |data| {{
            let _ = Validated::parse(data);
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
        let (packet, rem) = Ipv4WithOptions::parse(&data).unwrap();
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
            let _ = Ipv4WithOptions::parse(data);
        }});
    }}

    #[test]
    fn ipv4_options_absent_when_ihl_is_five() {{
        let data = [0x45, 0x11];
        let (packet, rem) = Ipv4WithOptions::parse(&data).unwrap();
        assert!(rem.is_empty());
        assert!(packet.options().is_none());
        assert_eq!(packet.proto(), 0x11);
        assert_eq!(packet.proto_bit_range(), 8..16);
        assert_parse_no_panic("Ipv4WithOptions absent", &data, |data| {{
            let _ = Ipv4WithOptions::parse(data);
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
        let (packet, rem) = TcpStart::parse(&data).unwrap();
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
        let (packet, rem) = TcpStart::parse(&no_options).unwrap();
        assert!(rem.is_empty());
        assert!(packet.options().is_none());
        assert_parse_no_panic("TcpStart", &data, |data| {{
            let _ = TcpStart::parse(data);
        }});
    }}

    #[test]
    fn conditional_else_branch_updates_offsets() {{
        let then_data = [1, 7, 9];
        let (packet, rem) = CondElse::parse(&then_data).unwrap();
        assert!(rem.is_empty());
        assert_eq!(packet.small(), Some(7));
        assert_eq!(packet.big(), None);
        assert_eq!(packet.tail(), 9);
        assert_eq!(packet.tail_bit_range(), 16..24);

        let else_data = [0, 0x12, 0x34, 9];
        let (packet, rem) = CondElse::parse(&else_data).unwrap();
        assert!(rem.is_empty());
        assert_eq!(packet.small(), None);
        assert_eq!(packet.big(), Some(0x1234));
        assert_eq!(packet.tail(), 9);
        assert_eq!(packet.tail_bit_range(), 24..32);

        assert_parse_no_panic("CondElse then", &then_data, |data| {{
            let _ = CondElse::parse(data);
        }});
        assert_parse_no_panic("CondElse else", &else_data, |data| {{
            let _ = CondElse::parse(data);
        }});
    }}

    #[test]
    fn greedy_rest_consumes_remaining_bytes() {{
        let data = [5, 1, 2, 3];
        let (packet, rem) = Rest::parse(&data).unwrap();
        assert!(rem.is_empty());
        let tail = packet
            .tail()
            .unwrap()
            .collect::<binparse::ParseResult<Vec<_>>>()
            .unwrap();
        assert_eq!(tail, vec![1, 2, 3]);
        assert_eq!(packet.tail_bit_range(), 8..32);

        let (packet, rem) = Rest::parse(&[7]).unwrap();
        assert!(rem.is_empty());
        let tail = packet
            .tail()
            .unwrap()
            .collect::<binparse::ParseResult<Vec<_>>>()
            .unwrap();
        assert!(tail.is_empty());

        assert_parse_no_panic("Rest", &data, |data| {{
            let _ = Rest::parse(data);
        }});
    }}

    #[test]
    fn greedy_rest_multibyte_requires_whole_elements() {{
        let data = [9, 0x12, 0x34, 0x56, 0x78];
        let (packet, rem) = RestWide::parse(&data).unwrap();
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
            let _ = RestWide::parse(data);
        }});
    }}

    #[test]
    fn until_array_stops_at_sentinel() {{
        let data = [b'h', b'i', 0, 7];
        let (packet, rem) = CStr::parse(&data).unwrap();
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

        let (packet, rem) = CStr::parse(&[0, 7]).unwrap();
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
            let _ = CStr::parse(data);
        }});
    }}

    #[test]
    fn greedy_struct_array_decodes_fixed_elements() {{
        let data = [1, 0x02, 0x03, 4, 0x05, 0x06];
        let (packet, rem) = GreedyStructs::parse(&data).unwrap();
        assert!(rem.is_empty());
        let items = packet
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
            let _ = GreedyStructs::parse(data);
        }});
    }}

    #[test]
    fn max_iter_bounds_sized_array() {{
        let data = [3, 1, 2, 3];
        let (packet, rem) = Capped::parse(&data).unwrap();
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
            let _ = Capped::parse(data);
        }});
    }}

    #[test]
    fn greedy_dynamic_struct_array_parses_until_exhausted() {{
        let data = [2, 9, 0];
        let (packet, rem) = Opts::parse(&data).unwrap();
        assert!(rem.is_empty());
        let opts = packet
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
        let (packet, _) = Opts::parse(&too_many).unwrap();
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

        let (packet, _) = Opts::parse(&[1]).unwrap();
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
            let _ = Opts::parse(data);
            if let Ok((packet, _)) = Opts::parse(data)
                && let Ok(opts) = packet.opts()
            {{
                for opt in opts.flatten() {{
                    let _ = opt.kind();
                    let _ = opt.body();
                }}
            }}
        }});
    }}

    #[test]
    fn padded_fields_decode_and_report_offsets() {{
        let data = [1, 0, 0, 2, 0x12, 0x34, 0x56, 0x78];
        let (packet, rem) = Padded::parse(&data).unwrap();
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
            let _ = Padded::parse(data);
        }});
    }}

    #[test]
    fn dynamic_pad_to_skips_to_boundary() {{
        let data = [1, 9, 0, 0, 7];
        let (packet, rem) = DynPadded::parse(&data).unwrap();
        assert!(rem.is_empty());
        assert_eq!(packet.tail(), 7);
        assert_eq!(packet.tail_bit_range(), 32..40);

        let aligned = [3, 9, 9, 9, 7];
        let (packet, rem) = DynPadded::parse(&aligned).unwrap();
        assert!(rem.is_empty());
        assert_eq!(packet.tail(), 7);
        assert_eq!(packet.tail_bit_range(), 32..40);

        assert_parse_no_panic("DynPadded", &data, |data| {{
            let _ = DynPadded::parse(data);
        }});
    }}

    #[test]
    fn dynamic_align_errors_on_misaligned_offset() {{
        let data = [1, 9, 0xab, 0xcd];
        let (packet, rem) = DynAligned::parse(&data).unwrap();
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
            let _ = DynAligned::parse(data);
        }});
        assert_parse_no_panic("DynAligned misaligned", &misaligned, |data| {{
            let _ = DynAligned::parse(data);
        }});
    }}

    #[test]
    fn skipped_fields_consume_bytes_and_stay_usable_in_expressions() {{
        let data = [0xad, 2, 0xaa, 0xbb, 0x5f];
        let (packet, rem) = SkipReserved::parse(&data).unwrap();
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
            let _ = SkipReserved::parse(data);
        }});
    }}

    #[test]
    fn lsb_bit_order_decodes_with_field_override() {{
        let data = [0b1010_1101, 0b0100_0011];
        let (packet, rem) = LsbFlags::parse(&data).unwrap();
        assert!(rem.is_empty());
        assert_eq!(packet.low(), 5);
        assert_eq!(packet.high(), 21);
        assert_eq!(packet.top(), 4);
        assert_eq!(packet.bottom(), 3);
        assert_parse_no_panic("LsbFlags", &data, |data| {{
            let _ = LsbFlags::parse(data);
        }});
    }}

    #[test]
    fn icmp_tuple_dispatch_decodes() {{
        let echo = [8, 0, 0x12, 0x34, 0x00, 0x01];
        let (packet, rem) = Icmp::parse(&echo).unwrap();
        assert!(rem.is_empty());
        match packet.body().unwrap() {{
            Icmp_body::Echo(echo) => {{
                assert_eq!(echo.id(), 0x1234);
                assert_eq!(echo.seq(), 1);
            }}
            _ => panic!("expected Echo body"),
        }}

        let unreach = [3, 1, 0xde, 0xad, 0xbe, 0xef];
        let (packet, _) = Icmp::parse(&unreach).unwrap();
        match packet.body().unwrap() {{
            Icmp_body::DestUnreach(unreach) => assert_eq!(unreach.unused(), 0xdead_beef),
            _ => panic!("expected DestUnreach body"),
        }}
        assert_eq!(packet.body_bit_range(), 16..48);

        let raw = [42, 7, 0xff];
        let (packet, rem) = Icmp::parse(&raw).unwrap();
        assert_eq!(rem, &[0xff]);
        match packet.body().unwrap() {{
            Icmp_body::Raw(_) => {{}}
            _ => panic!("expected Raw body"),
        }}

        assert_parse_no_panic("Icmp", &echo, |data| {{
            let _ = Icmp::parse(data);
        }});
    }}

    #[test]
    fn union_dynamic_variant_decodes() {{
        let data = [1, 3, 0xaa, 0xbb, 0xcc, 0x99];
        let (packet, rem) = Dispatch::parse(&data).unwrap();
        assert_eq!(rem, &[0x99]);
        match packet.body().unwrap() {{
            Dispatch_body::Msg(msg) => {{
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
            let _ = Dispatch::parse(data);
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
        let (packet, rem) = Dispatch::parse(&good).unwrap();
        assert!(rem.is_empty());
        match packet.body().unwrap() {{
            Dispatch_body::Checked(checked) => assert_eq!(checked.version(), 4),
            _ => panic!("expected Checked body"),
        }}
    }}

    #[test]
    fn union_error_variant_surfaces_declared_error() {{
        let data = [9, 0xaa];
        let (packet, rem) = Dispatch::parse(&data).unwrap();
        assert_eq!(rem, &[0xaa]);
        assert_eq!(packet.body_bit_range(), 8..8);
        match packet.body() {{
            Err(Error::UNKNOWN_KIND {{ kind }}) => assert_eq!(kind, 9),
            _ => panic!("expected UNKNOWN_KIND error"),
        }}
        assert_parse_no_panic("Dispatch", &data, |data| {{
            let _ = Dispatch::parse(data);
        }});
    }}

    #[test]
    fn unions_in_concat_decode_independently() {{
        let data = [1, 2, 0x42, 0x12, 0x34, 2, 0xaa, 0xbb, 0x99];
        let (packet, rem) = ConcatUnion::parse(&data).unwrap();
        assert!(rem.is_empty());
        let (first, second, third) = packet.pair();
        assert_eq!(first, 0x42);
        match second.unwrap() {{
            ConcatUnion_pair_1::Word(word) => assert_eq!(word.w(), 0x1234),
            _ => panic!("expected Word"),
        }}
        match third.unwrap() {{
            ConcatUnion_pair_2::Bytes(bytes) => {{
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
        let (packet, rem) = ConcatUnion::parse(&empty).unwrap();
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
            let _ = ConcatUnion::parse(data);
        }});
    }}

    #[test]
    fn len_bounded_struct_ref_decodes_within_bound() {{
        let data = [7, 5, 0x01, 0x02, 0x03, 0xaa, 0xbb, 0x99];
        let (packet, rem) = Bounded::parse(&data).unwrap();
        assert!(rem.is_empty());
        assert_eq!(packet.tag(), 7);
        assert_eq!(packet.len(), 5);
        let inner = packet.value().unwrap();
        assert_eq!(inner.a(), 0x01);
        assert_eq!(inner.b(), 0x0203);
        assert_eq!(packet.value_rest().unwrap(), &[0xaa, 0xbb]);
        assert_eq!(packet.after(), 0x99);
        assert_eq!(packet.value_bit_range(), 16..56);

        let exact = [7, 3, 0x01, 0x02, 0x03, 0x99];
        let (packet, rem) = Bounded::parse(&exact).unwrap();
        assert!(rem.is_empty());
        assert_eq!(packet.value().unwrap().b(), 0x0203);
        assert!(packet.value_rest().unwrap().is_empty());
        assert_eq!(packet.after(), 0x99);
    }}

    #[test]
    fn len_bounded_struct_ref_rejects_inner_overrun() {{
        let data = [7, 2, 0x01, 0x02, 0x99];
        let (packet, rem) = Bounded::parse(&data).unwrap();
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
            let _ = Bounded::parse(data);
        }});
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

struct Varint {
    tag: u8,
    @hook(read_leb128, u64) value: [u8],
    after: u8,
}

struct Lying {
    @hook(lying_hook, u8) v: [u8],
}

struct DnsMsg {
    id: u16,
    @hook(parse_dns_name, String) qname: [u8],
    qtype: u16,
    @hook(parse_dns_name, String) aname: [u8],
    atype: u16,
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
