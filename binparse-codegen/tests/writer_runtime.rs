use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn generated_code(dsl: &str) -> String {
    let ast = binparse_dsl_parse::parse_str(dsl).expect("failed to parse DSL");
    binparse_codegen::CodeGen::generate_writers(&ast).expect("failed to generate code")
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("binparse-codegen should have a workspace parent")
        .to_path_buf()
}

fn write_runtime_crate(name: &str, code: &str, test_body: &str) -> PathBuf {
    let root = workspace_root();
    let test_dir = root
        .join("target")
        .join("writer-runtime-tests")
        .join(format!("runtime-{}-{}", name, std::process::id()));

    let _ = fs::remove_dir_all(&test_dir);
    fs::create_dir_all(test_dir.join("src")).expect("failed to create runtime test crate");

    let binparse_path = root.join("binparse");
    fs::write(
        test_dir.join("Cargo.toml"),
        format!(
            r#"[package]
name = "writer-runtime-test"
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
{code}

#[cfg(test)]
mod tests {{
    use super::*;

    #[test]
    fn round_trip() {{
        {test_body}
    }}
}}
"#
        ),
    )
    .expect("failed to write runtime lib.rs");

    test_dir
}

fn run_round_trip(name: &str, dsl: &str, test_body: &str) {
    let code = generated_code(dsl);
    let test_dir = write_runtime_crate(name, &code, test_body);
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

#[test]
fn generated_writer_round_trips() {
    let dsl = r#"
@endian(big)
struct P {
    a: u8,
    b: u16,
    c: u32,
}
"#;
    let test_body = r#"
        assert_eq!(PWriter::SIZE, 7);
        assert!(PWriter::new(&mut [0u8; 3]).is_err());

        let content = PContent { a: 0x11, b: 0x2233, c: 0x44556677 };
        let bytes = PWriter::to_vec(&content);
        let (p, _) = P::parse(&bytes).unwrap();
        assert_eq!(p.a(), 0x11);
        assert_eq!(p.b(), 0x2233);
        assert_eq!(p.c(), 0x44556677);
    "#;
    run_round_trip("primitive", dsl, test_body);
}

#[test]
fn generated_writer_bitfields_round_trip() {
    let dsl = r#"
@endian(big)
struct Ip {
    version: b<4>,
    ihl: b<4>,
    ttl: u8,
    total: u16,
}
"#;
    let test_body = r#"
        assert_eq!(IpWriter::SIZE, 4);

        let content = IpContent { version: 4, ihl: 5, ttl: 64, total: 0x1234 };
        let bytes = IpWriter::to_vec(&content);
        let (ip, _) = Ip::parse(&bytes).unwrap();
        assert_eq!(ip.version(), 4);
        assert_eq!(ip.ihl(), 5);
        assert_eq!(ip.ttl(), 64);
        assert_eq!(ip.total(), 0x1234);
    "#;
    run_round_trip("bitfield-mixed", dsl, test_body);
}

#[test]
fn generated_writer_bitfields_straddle_msb_round_trips() {
    let dsl = r#"
struct S {
    a: b<3>,
    b: b<7>,
    c: b<6>,
}
"#;
    let test_body = r#"
        assert_eq!(SWriter::SIZE, 2);

        let content = SContent { a: 5, b: 0x4b, c: 0x29 };
        let bytes = SWriter::to_vec(&content);
        let (s, _) = S::parse(&bytes).unwrap();
        assert_eq!(s.a(), 5);
        assert_eq!(s.b(), 0x4b);
        assert_eq!(s.c(), 0x29);
    "#;
    run_round_trip("bitfield-straddle-msb", dsl, test_body);
}

#[test]
fn generated_writer_bitfields_straddle_lsb_round_trips() {
    let dsl = r#"
@bit_order(lsb)
struct S {
    a: b<3>,
    b: b<7>,
    c: b<6>,
}
"#;
    let test_body = r#"
        assert_eq!(SWriter::SIZE, 2);

        let content = SContent { a: 5, b: 0x4b, c: 0x29 };
        let bytes = SWriter::to_vec(&content);
        let (s, _) = S::parse(&bytes).unwrap();
        assert_eq!(s.a(), 5);
        assert_eq!(s.b(), 0x4b);
        assert_eq!(s.c(), 0x29);
    "#;
    run_round_trip("bitfield-straddle-lsb", dsl, test_body);
}

#[test]
fn generated_writer_byte_array_to_vec_round_trips() {
    let dsl = r#"
struct Eth {
    dst: [u8; 6],
    src: [u8; 6],
    ethertype: u16,
}
"#;
    let test_body = r#"
        assert_eq!(EthWriter::SIZE, 14);

        let content = EthContent {
            dst: [1, 2, 3, 4, 5, 6],
            src: [10, 11, 12, 13, 14, 15],
            ethertype: 0x0800,
        };
        let bytes = EthWriter::to_vec(&content);
        let (p, _) = Eth::parse(&bytes).unwrap();
        assert_eq!(p.ethertype(), 0x0800);
        assert_eq!(
            p.dst().unwrap().collect::<Result<Vec<u8>, _>>().unwrap(),
            vec![1, 2, 3, 4, 5, 6]
        );
        assert_eq!(
            p.src().unwrap().collect::<Result<Vec<u8>, _>>().unwrap(),
            vec![10, 11, 12, 13, 14, 15]
        );
    "#;
    run_round_trip("byte-array-to-vec", dsl, test_body);
}

#[test]
fn generated_writer_byte_array_mut_round_trips() {
    let dsl = r#"
struct Eth {
    dst: [u8; 6],
    src: [u8; 6],
    ethertype: u16,
}
"#;
    let test_body = r#"
        let mut buf = [0u8; 14];
        let mut w = EthWriter::new(&mut buf).unwrap();
        w.dst_mut().copy_from_slice(&[1, 2, 3, 4, 5, 6]);
        w.src_mut().copy_from_slice(&[10, 11, 12, 13, 14, 15]);
        w.set_ethertype(0x0800);

        let (p, _) = Eth::parse(&buf).unwrap();
        assert_eq!(p.ethertype(), 0x0800);
        assert_eq!(
            p.dst().unwrap().collect::<Result<Vec<u8>, _>>().unwrap(),
            vec![1, 2, 3, 4, 5, 6]
        );
        assert_eq!(
            p.src().unwrap().collect::<Result<Vec<u8>, _>>().unwrap(),
            vec![10, 11, 12, 13, 14, 15]
        );
    "#;
    run_round_trip("byte-array-mut", dsl, test_body);
}

#[test]
fn generated_writer_struct_ref_to_vec_round_trips() {
    let dsl = r#"
@endian(big) struct Inner { a: u8, b: u16 }
@endian(big) struct Outer { tag: u8, inner: Inner, trailer: u8 }
"#;
    let test_body = r#"
        assert_eq!(OuterWriter::SIZE, 5);

        let content = OuterContent {
            tag: 0xAA,
            inner: InnerContent { a: 1, b: 0x0203 },
            trailer: 0xBB,
        };
        let bytes = OuterWriter::to_vec(&content);
        let (outer, _) = Outer::parse(&bytes).unwrap();
        assert_eq!(outer.tag(), 0xAA);
        assert_eq!(outer.trailer(), 0xBB);
        assert_eq!(outer.inner().unwrap().a(), 1);
        assert_eq!(outer.inner().unwrap().b(), 0x0203);
    "#;
    run_round_trip("struct-ref-to-vec", dsl, test_body);
}

#[test]
fn generated_writer_struct_ref_mut_round_trips() {
    let dsl = r#"
@endian(big) struct Inner { a: u8, b: u16 }
@endian(big) struct Outer { tag: u8, inner: Inner, trailer: u8 }
"#;
    let test_body = r#"
        let mut buf = [0u8; 5];
        let mut w = OuterWriter::new(&mut buf).unwrap();
        w.set_tag(0xAA);
        {
            let mut inner = w.inner_mut();
            inner.set_a(1);
            inner.set_b(0x0203);
        }
        w.set_trailer(0xBB);

        let (outer, _) = Outer::parse(&buf).unwrap();
        assert_eq!(outer.tag(), 0xAA);
        assert_eq!(outer.trailer(), 0xBB);
        assert_eq!(outer.inner().unwrap().a(), 1);
        assert_eq!(outer.inner().unwrap().b(), 0x0203);
    "#;
    run_round_trip("struct-ref-mut", dsl, test_body);
}

#[test]
fn generated_writer_bitfield_then_byte_array_round_trips() {
    let dsl = r#"
@endian(big)
struct M {
    a: b<4>,
    b: b<4>,
    data: [u8; 3],
    tail: u16,
}
"#;
    let test_body = r#"
        assert_eq!(MWriter::SIZE, 6);

        let content = MContent { a: 0xa, b: 0x5, data: [0x11, 0x22, 0x33], tail: 0x4455 };
        let bytes = MWriter::to_vec(&content);
        let (m, _) = M::parse(&bytes).unwrap();
        assert_eq!(m.a(), 0xa);
        assert_eq!(m.b(), 0x5);
        assert_eq!(
            m.data().unwrap().collect::<Result<Vec<u8>, _>>().unwrap(),
            vec![0x11, 0x22, 0x33]
        );
        assert_eq!(m.tail(), 0x4455);
    "#;
    run_round_trip("bitfield-then-byte-array", dsl, test_body);
}

#[test]
fn generated_writer_dynamic_tail_u8_round_trips() {
    let dsl = r#"
struct Frame {
    kind: u8,
    len: u8,
    payload: [u8; len],
}
"#;
    let test_body = r#"
        let content = FrameContent { kind: 0x02, payload: b"hello" };
        let bytes = FrameWriter::to_vec(&content);
        assert_eq!(bytes.len(), 7);
        let (frame, _) = Frame::parse(&bytes).unwrap();
        assert_eq!(frame.kind(), 0x02);
        assert_eq!(frame.len(), 5);
        assert_eq!(
            frame.payload().unwrap().collect::<Result<Vec<u8>, _>>().unwrap(),
            b"hello".to_vec()
        );

        let lens = FrameLens { payload: 5 };
        let mut buf = vec![0u8; FrameWriter::encoded_len(&lens)];
        let mut w = FrameWriter::new(&mut buf, lens).unwrap();
        w.set_kind(0x02);
        w.payload_mut().copy_from_slice(b"hello");
        let (frame, _) = Frame::parse(&buf).unwrap();
        assert_eq!(frame.kind(), 0x02);
        assert_eq!(frame.len(), 5);
        assert_eq!(
            frame.payload().unwrap().collect::<Result<Vec<u8>, _>>().unwrap(),
            b"hello".to_vec()
        );

        assert!(matches!(
            FrameWriter::new(&mut [0u8; 3], FrameLens { payload: 5 }),
            Err(binparse::WriteError::NotEnoughSpace { .. })
        ));
        assert!(matches!(
            FrameWriter::new(&mut [0u8; 500], FrameLens { payload: 300 }),
            Err(binparse::WriteError::ValueTooLarge { .. })
        ));
    "#;
    run_round_trip("dynamic-tail-u8", dsl, test_body);
}

#[test]
fn generated_writer_varint_dynamic_tail_one_byte_round_trips() {
    let dsl = r#"
struct VarFrame {
    tag: u8,
    @hook(binparse.hooks.leb128_unsigned, u64) @write_hook(binparse.hooks.write_leb128_unsigned, binparse.hooks.leb128_unsigned_len) len: [u8],
    body: [u8; len],
}
"#;
    let test_body = r#"
        let content = VarFrameContent { tag: 0x07, body: b"hello" };
        let bytes = VarFrameWriter::to_vec(&content);
        assert_eq!(bytes, vec![0x07, 0x05, b'h', b'e', b'l', b'l', b'o']);

        let (frame, _) = VarFrame::parse(&bytes).unwrap();
        assert_eq!(frame.tag(), 0x07);
        assert_eq!(frame.len().unwrap(), 5);
        assert_eq!(
            frame.body().unwrap().collect::<Result<Vec<u8>, _>>().unwrap(),
            b"hello".to_vec()
        );
    "#;
    run_round_trip("varint-tail-one-byte", dsl, test_body);
}

#[test]
fn generated_writer_varint_dynamic_tail_two_byte_round_trips() {
    let dsl = r#"
struct VarFrame {
    tag: u8,
    @hook(binparse.hooks.leb128_unsigned, u64) @write_hook(binparse.hooks.write_leb128_unsigned, binparse.hooks.leb128_unsigned_len) len: [u8],
    body: [u8; len],
}
"#;
    let test_body = r#"
        let payload = vec![0xCDu8; 300];
        let content = VarFrameContent { tag: 0x01, body: &payload };
        let lens = VarFrameLens { body: 300 };
        assert_eq!(VarFrameWriter::encoded_len(&lens), 303);

        let bytes = VarFrameWriter::to_vec(&content);
        assert_eq!(bytes.len(), 303);
        assert_eq!(&bytes[1..3], &[0xAC, 0x02]);

        let (frame, _) = VarFrame::parse(&bytes).unwrap();
        assert_eq!(frame.tag(), 0x01);
        assert_eq!(frame.len().unwrap(), 300);
        assert_eq!(
            frame.body().unwrap().collect::<Result<Vec<u8>, _>>().unwrap(),
            payload
        );
    "#;
    run_round_trip("varint-tail-two-byte", dsl, test_body);
}

#[test]
fn generated_writer_varint_tail_without_write_hook_errors() {
    let dsl = r#"
struct VarFrame {
    tag: u8,
    @hook(binparse.hooks.leb128_unsigned, u64) len: [u8],
    body: [u8; len],
}
"#;
    let ast = binparse_dsl_parse::parse_str(dsl).expect("failed to parse DSL");
    assert!(binparse_codegen::CodeGen::generate_writers(&ast).is_err());
}

#[test]
fn generated_writer_union_connect_variant_round_trips() {
    let dsl = r#"
@endian(big)
struct Packet {
    kind: u8,
    body: union(kind) {
        1 => Connect { keep_alive: u16 },
        2 => Connack { ack: u8, code: u8 },
        _ => Unknown { },
    },
}
"#;
    let test_body = r#"
        let content = PacketContent {
            body: PacketBodyContent::Connect(ConnectContent { keep_alive: 60 }),
        };
        let bytes = PacketWriter::to_vec(&content);
        assert_eq!(bytes, vec![0x01, 0x00, 0x3C]);

        let (packet, _) = Packet::parse(&bytes).unwrap();
        assert_eq!(packet.kind(), 1);
        match packet.body().unwrap() {
            Packet_body::Connect(connect) => assert_eq!(connect.keep_alive(), 60),
            _ => panic!("expected Connect variant"),
        }
    "#;
    run_round_trip("union-connect", dsl, test_body);
}

#[test]
fn generated_writer_union_connack_variant_round_trips() {
    let dsl = r#"
@endian(big)
struct Packet {
    kind: u8,
    body: union(kind) {
        1 => Connect { keep_alive: u16 },
        2 => Connack { ack: u8, code: u8 },
        _ => Unknown { },
    },
}
"#;
    let test_body = r#"
        let content = PacketContent {
            body: PacketBodyContent::Connack(ConnackContent { ack: 0xAB, code: 0x05 }),
        };
        let bytes = PacketWriter::to_vec(&content);
        assert_eq!(bytes, vec![0x02, 0xAB, 0x05]);

        let (packet, _) = Packet::parse(&bytes).unwrap();
        assert_eq!(packet.kind(), 2);
        match packet.body().unwrap() {
            Packet_body::Connack(connack) => {
                assert_eq!(connack.ack(), 0xAB);
                assert_eq!(connack.code(), 0x05);
            }
            _ => panic!("expected Connack variant"),
        }
    "#;
    run_round_trip("union-connack", dsl, test_body);
}

#[test]
fn generated_writer_union_bitfield_discriminant_round_trips() {
    let dsl = r#"
@endian(big)
struct P2 {
    tag: b<4>,
    flags: b<4>,
    body: union(tag) {
        1 => A { v: u16 },
        2 => B { v: u32 },
        _ => Unknown { },
    },
}
"#;
    let test_body = r#"
        let content = P2Content {
            flags: 0x7,
            body: P2BodyContent::A(AContent { v: 0x1234 }),
        };
        let bytes = P2Writer::to_vec(&content);
        assert_eq!(bytes, vec![0x17, 0x12, 0x34]);

        let (p2, _) = P2::parse(&bytes).unwrap();
        assert_eq!(p2.tag(), 1);
        assert_eq!(p2.flags(), 0x7);
        match p2.body().unwrap() {
            P2_body::A(a) => assert_eq!(a.v(), 0x1234),
            _ => panic!("expected A variant"),
        }

        let content = P2Content {
            flags: 0x2,
            body: P2BodyContent::B(BContent { v: 0xDEADBEEF }),
        };
        let bytes = P2Writer::to_vec(&content);
        assert_eq!(bytes, vec![0x22, 0xDE, 0xAD, 0xBE, 0xEF]);

        let (p2, _) = P2::parse(&bytes).unwrap();
        assert_eq!(p2.tag(), 2);
        assert_eq!(p2.flags(), 0x2);
        match p2.body().unwrap() {
            P2_body::B(b) => assert_eq!(b.v(), 0xDEADBEEF),
            _ => panic!("expected B variant"),
        }
    "#;
    run_round_trip("union-bitfield-disc", dsl, test_body);
}

#[test]
fn generated_writer_ethernet_greedy_payload_round_trips() {
    let dsl = r#"
struct EthernetII {
    dst: [u8; 6],
    src: [u8; 6],
    @discriminator ethertype: u16,
    @greedy(unsafe_eof) @payload payload: [u8],
}
"#;
    let test_body = r#"
        let content = EthernetIIContent {
            dst: [1, 2, 3, 4, 5, 6],
            src: [10, 11, 12, 13, 14, 15],
            ethertype: 0x0800,
            payload: b"hello world",
        };
        let lens = EthernetIILens { payload: content.payload.len() };
        assert_eq!(EthernetIIWriter::encoded_len(&lens), 14 + content.payload.len());

        let bytes = EthernetIIWriter::to_vec(&content);
        let (eth, _) = EthernetII::parse(&bytes).unwrap();
        assert_eq!(
            eth.dst().unwrap().collect::<Result<Vec<u8>, _>>().unwrap(),
            vec![1, 2, 3, 4, 5, 6]
        );
        assert_eq!(
            eth.src().unwrap().collect::<Result<Vec<u8>, _>>().unwrap(),
            vec![10, 11, 12, 13, 14, 15]
        );
        assert_eq!(eth.ethertype(), 0x0800);
        assert_eq!(
            eth.payload().unwrap().collect::<Result<Vec<u8>, _>>().unwrap(),
            b"hello world".to_vec()
        );
    "#;
    run_round_trip("ethernet-greedy-payload", dsl, test_body);
}

#[test]
fn generated_writer_vlan_constant_and_payload_round_trips() {
    let dsl = r#"
struct Vlan {
    dst: [u8; 6],
    src: [u8; 6],
    tpid = x8100,
    pcp: b<3>,
    dei: b<1>,
    vid_hi: b<4>,
    vid_lo: u8,
    ethertype: u16,
    @greedy(unsafe_eof) payload: [u8],
}
"#;
    let test_body = r#"
        let content = VlanContent {
            dst: [1, 2, 3, 4, 5, 6],
            src: [10, 11, 12, 13, 14, 15],
            pcp: 5,
            dei: 1,
            vid_hi: 0xa,
            vid_lo: 0xbc,
            ethertype: 0x0800,
            payload: b"payload-bytes",
        };
        let bytes = VlanWriter::to_vec(&content);
        assert_eq!(&bytes[12..14], &[0x81, 0x00]);

        let (vlan, _) = Vlan::parse(&bytes).unwrap();
        assert_eq!(
            vlan.dst().unwrap().collect::<Result<Vec<u8>, _>>().unwrap(),
            vec![1, 2, 3, 4, 5, 6]
        );
        assert_eq!(vlan.tpid(), 0x8100);
        assert_eq!(vlan.pcp(), 5);
        assert_eq!(vlan.dei(), 1);
        assert_eq!(vlan.vid_hi(), 0xa);
        assert_eq!(vlan.vid_lo(), 0xbc);
        assert_eq!(vlan.ethertype(), 0x0800);
        assert_eq!(
            vlan.payload().unwrap().collect::<Result<Vec<u8>, _>>().unwrap(),
            b"payload-bytes".to_vec()
        );
    "#;
    run_round_trip("vlan-constant-payload", dsl, test_body);
}

#[test]
fn generated_writer_dynamic_tail_u16_big_endian_round_trips() {
    let dsl = r#"
@endian(big)
struct Msg {
    len: u16,
    body: [u8; len],
}
"#;
    let test_body = r#"
        let payload = vec![0xABu8; 300];
        let content = MsgContent { body: &payload };
        let bytes = MsgWriter::to_vec(&content);
        assert_eq!(bytes.len(), 302);
        let (msg, _) = Msg::parse(&bytes).unwrap();
        assert_eq!(msg.len(), 300);
        assert_eq!(
            msg.body().unwrap().collect::<Result<Vec<u8>, _>>().unwrap(),
            payload
        );
    "#;
    run_round_trip("dynamic-tail-u16-be", dsl, test_body);
}

#[test]
fn generated_writer_dynamic_region_with_fixed_trailer_round_trips() {
    let dsl = r#"
@endian(big)
struct Frame {
    kind: u8,
    len: u8,
    payload: [u8; len],
    crc: u16,
    tail: u8,
}
"#;
    let test_body = r#"
        let content = FrameContent { kind: 0x09, payload: b"hello", crc: 0xBEEF, tail: 0x7F };
        assert_eq!(FrameWriter::encoded_len(&FrameLens { payload: 5 }), 10);
        let bytes = FrameWriter::to_vec(&content);
        assert_eq!(
            bytes,
            vec![0x09, 0x05, b'h', b'e', b'l', b'l', b'o', 0xBE, 0xEF, 0x7F]
        );

        let (frame, rem) = Frame::parse(&bytes).unwrap();
        assert!(rem.is_empty());
        assert_eq!(frame.kind(), 0x09);
        assert_eq!(frame.len(), 5);
        assert_eq!(
            frame.payload().unwrap().collect::<Result<Vec<u8>, _>>().unwrap(),
            b"hello".to_vec()
        );
        assert_eq!(frame.crc(), 0xBEEF);
        assert_eq!(frame.tail(), 0x7F);

        let lens = FrameLens { payload: 5 };
        let mut buf = vec![0u8; FrameWriter::encoded_len(&lens)];
        let mut w = FrameWriter::new(&mut buf, lens).unwrap();
        w.set_kind(0x09);
        w.payload_mut().copy_from_slice(b"world");
        w.set_crc(0xBEEF);
        w.set_tail(0x7F);
        let (frame, _) = Frame::parse(&buf).unwrap();
        assert_eq!(frame.kind(), 0x09);
        assert_eq!(frame.len(), 5);
        assert_eq!(
            frame.payload().unwrap().collect::<Result<Vec<u8>, _>>().unwrap(),
            b"world".to_vec()
        );
        assert_eq!(frame.crc(), 0xBEEF);
        assert_eq!(frame.tail(), 0x7F);

        let empty = FrameContent { kind: 1, payload: b"", crc: 0x0102, tail: 0x03 };
        let bytes = FrameWriter::to_vec(&empty);
        assert_eq!(bytes, vec![0x01, 0x00, 0x01, 0x02, 0x03]);
        let (frame, _) = Frame::parse(&bytes).unwrap();
        assert_eq!(frame.len(), 0);
        assert_eq!(frame.crc(), 0x0102);
        assert_eq!(frame.tail(), 0x03);

        assert!(matches!(
            FrameWriter::new(&mut [0u8; 3], FrameLens { payload: 5 }),
            Err(binparse::WriteError::NotEnoughSpace { .. })
        ));
        assert!(matches!(
            FrameWriter::new(&mut [0u8; 600], FrameLens { payload: 300 }),
            Err(binparse::WriteError::ValueTooLarge { .. })
        ));
    "#;
    run_round_trip("dynamic-region-fixed-trailer", dsl, test_body);
}

#[test]
fn generated_writer_dynamic_region_u16_with_array_trailer_round_trips() {
    let dsl = r#"
@endian(big)
struct Msg {
    ver: u8,
    rlen: u16,
    region: [u8; rlen],
    footer: [u8; 4],
    checksum: u16,
}
"#;
    let test_body = r#"
        let region = vec![0xAAu8; 300];
        let content = MsgContent {
            ver: 0x01,
            region: &region,
            footer: [0xDE, 0xAD, 0xBE, 0xEF],
            checksum: 0x1234,
        };
        let lens = MsgLens { region: 300 };
        assert_eq!(MsgWriter::encoded_len(&lens), 3 + 300 + 6);

        let bytes = MsgWriter::to_vec(&content);
        assert_eq!(bytes.len(), 309);
        assert_eq!(&bytes[1..3], &[0x01, 0x2c]);
        assert_eq!(&bytes[303..307], &[0xDE, 0xAD, 0xBE, 0xEF]);
        assert_eq!(&bytes[307..309], &[0x12, 0x34]);

        let (msg, rem) = Msg::parse(&bytes).unwrap();
        assert!(rem.is_empty());
        assert_eq!(msg.ver(), 1);
        assert_eq!(msg.rlen(), 300);
        assert_eq!(
            msg.region().unwrap().collect::<Result<Vec<u8>, _>>().unwrap(),
            region
        );
        assert_eq!(
            msg.footer().unwrap().collect::<Result<Vec<u8>, _>>().unwrap(),
            vec![0xDE, 0xAD, 0xBE, 0xEF]
        );
        assert_eq!(msg.checksum(), 0x1234);

        let mut big = vec![0u8; 80000];
        assert!(matches!(
            MsgWriter::new(&mut big, MsgLens { region: 70000 }),
            Err(binparse::WriteError::ValueTooLarge { .. })
        ));
    "#;
    run_round_trip("dynamic-region-array-trailer", dsl, test_body);
}

#[test]
fn generated_writer_affine_len_minus_const_round_trips() {
    let dsl = r#"
@endian(big)
struct Udp {
    src: u16,
    dst: u16,
    length: u16,
    checksum: u16,
    payload: [u8; length - 8],
}
"#;
    let test_body = r#"
        let content = UdpContent {
            src: 0x1234,
            dst: 0x5678,
            checksum: 0xBEEF,
            payload: b"hello",
        };
        let lens = UdpLens { payload: 5 };
        assert_eq!(UdpWriter::encoded_len(&lens), 13);

        let bytes = UdpWriter::to_vec(&content);
        assert_eq!(bytes.len(), 13);
        assert_eq!(
            bytes,
            vec![0x12, 0x34, 0x56, 0x78, 0x00, 0x0D, 0xBE, 0xEF, b'h', b'e', b'l', b'l', b'o']
        );

        let (udp, rem) = Udp::parse(&bytes).unwrap();
        assert!(rem.is_empty());
        assert_eq!(udp.src(), 0x1234);
        assert_eq!(udp.dst(), 0x5678);
        assert_eq!(udp.checksum(), 0xBEEF);
        // derived length field inverts the size expr: length == payload.len() + 8
        assert_eq!(udp.length(), 13);
        assert_eq!(
            udp.payload().unwrap().collect::<Result<Vec<u8>, _>>().unwrap(),
            b"hello".to_vec()
        );

        // empty payload: length == 8 (the minimum, == k)
        let empty = UdpContent { src: 1, dst: 2, checksum: 3, payload: b"" };
        let bytes = UdpWriter::to_vec(&empty);
        assert_eq!(bytes.len(), 8);
        let (udp, _) = Udp::parse(&bytes).unwrap();
        assert_eq!(udp.length(), 8);
        assert_eq!(
            udp.payload().unwrap().collect::<Result<Vec<u8>, _>>().unwrap(),
            Vec::<u8>::new()
        );

        // Mode 1 random access keeps the same derived length.
        let lens = UdpLens { payload: 5 };
        let mut buf = vec![0u8; UdpWriter::encoded_len(&lens)];
        let mut w = UdpWriter::new(&mut buf, lens).unwrap();
        w.set_src(0x1234);
        w.set_dst(0x5678);
        w.set_checksum(0xBEEF);
        w.payload_mut().copy_from_slice(b"world");
        let (udp, _) = Udp::parse(&buf).unwrap();
        assert_eq!(udp.length(), 13);
        assert_eq!(
            udp.payload().unwrap().collect::<Result<Vec<u8>, _>>().unwrap(),
            b"world".to_vec()
        );

        // ValueTooLarge edge: the WRITTEN length (payload + 8) overflows u16.
        let mut big = vec![0u8; 70000];
        assert!(matches!(
            UdpWriter::new(&mut big, UdpLens { payload: 65534 }),
            Err(binparse::WriteError::ValueTooLarge { .. })
        ));
        // payload that fits exactly: 65535 - 8 = 65527 is the largest legal payload.
        let lens = UdpLens { payload: 65528 };
        let mut buf = vec![0u8; UdpWriter::encoded_len(&lens)];
        assert!(matches!(
            UdpWriter::new(&mut buf, lens),
            Err(binparse::WriteError::ValueTooLarge { .. })
        ));
    "#;
    run_round_trip("affine-len-minus-const", dsl, test_body);
}

#[test]
fn generated_writer_affine_len_minus_const_with_trailer_round_trips() {
    let dsl = r#"
@endian(big)
struct Frame {
    kind: u8,
    total: u16,
    payload: [u8; total - 5],
    crc: u16,
}
"#;
    let test_body = r#"
        let content = FrameContent {
            kind: 0x09,
            payload: b"abc",
            crc: 0xDEAD,
        };
        let lens = FrameLens { payload: 3 };
        // 1 (kind) + 2 (total) + 3 (payload) + 2 (crc) = 8
        assert_eq!(FrameWriter::encoded_len(&lens), 8);

        let bytes = FrameWriter::to_vec(&content);
        assert_eq!(
            bytes,
            vec![0x09, 0x00, 0x08, b'a', b'b', b'c', 0xDE, 0xAD]
        );

        let (frame, rem) = Frame::parse(&bytes).unwrap();
        assert!(rem.is_empty());
        assert_eq!(frame.kind(), 0x09);
        // derived: total == payload.len() + 5
        assert_eq!(frame.total(), 8);
        assert_eq!(
            frame.payload().unwrap().collect::<Result<Vec<u8>, _>>().unwrap(),
            b"abc".to_vec()
        );
        assert_eq!(frame.crc(), 0xDEAD);
    "#;
    run_round_trip("affine-len-minus-const-trailer", dsl, test_body);
}

#[test]
fn generated_writer_affine_len_plus_const_round_trips() {
    let dsl = r#"
struct Frame {
    kind: u8,
    len: u8,
    payload: [u8; len + 2],
}
"#;
    let test_body = r#"
        let content = FrameContent { kind: 0x02, payload: b"hello" };
        let lens = FrameLens { payload: 5 };
        assert_eq!(FrameWriter::encoded_len(&lens), 7);

        let bytes = FrameWriter::to_vec(&content);
        // derived: len == payload.len() - 2 == 3
        assert_eq!(bytes, vec![0x02, 0x03, b'h', b'e', b'l', b'l', b'o']);

        let (frame, _) = Frame::parse(&bytes).unwrap();
        assert_eq!(frame.kind(), 0x02);
        assert_eq!(frame.len(), 3);
        assert_eq!(
            frame.payload().unwrap().collect::<Result<Vec<u8>, _>>().unwrap(),
            b"hello".to_vec()
        );

        // underflow edge: a payload smaller than k saturates the derived len to 0,
        // which the reader then sees as a 2-byte region (len + 2).
        let small = FrameContent { kind: 0x01, payload: b"xy" };
        let bytes = FrameWriter::to_vec(&small);
        assert_eq!(bytes, vec![0x01, 0x00, b'x', b'y']);
        let (frame, _) = Frame::parse(&bytes).unwrap();
        assert_eq!(frame.len(), 0);
        assert_eq!(
            frame.payload().unwrap().collect::<Result<Vec<u8>, _>>().unwrap(),
            b"xy".to_vec()
        );
    "#;
    run_round_trip("affine-len-plus-const", dsl, test_body);
}

#[test]
fn generated_writer_len_region_struct_ref_round_trips() {
    let dsl = r#"
@endian(big) struct Inner { a: u8, b: u16 }
@endian(big) struct Tlv { tag: u8, length: u8, @len(length) value: Inner }
"#;
    let test_body = r#"
        let content = TlvContent {
            tag: 0x42,
            value: InnerContent { a: 0x11, b: 0x2233 },
        };
        let lens = TlvLens { value: InnerWriter::SIZE };
        // 1 (tag) + 1 (length) + 3 (Inner::SIZE) = 5
        assert_eq!(TlvWriter::encoded_len(&lens), 5);

        let bytes = TlvWriter::to_vec(&content);
        // length is DERIVED = Inner encoded_len = 3
        assert_eq!(bytes, vec![0x42, 0x03, 0x11, 0x22, 0x33]);

        let (tlv, rem) = Tlv::parse(&bytes).unwrap();
        assert!(rem.is_empty());
        assert_eq!(tlv.tag(), 0x42);
        // derived length field == inner encoded_len
        assert_eq!(tlv.length() as usize, InnerWriter::SIZE);
        let inner = tlv.value().unwrap();
        assert_eq!(inner.a(), 0x11);
        assert_eq!(inner.b(), 0x2233);

        // Mode 1: the region accessor returns the child writer, derived length still set.
        let lens = TlvLens { value: InnerWriter::SIZE };
        let mut buf = vec![0u8; TlvWriter::encoded_len(&lens)];
        let mut w = TlvWriter::new(&mut buf, lens).unwrap();
        w.set_tag(0x99);
        {
            let mut inner = w.value_mut();
            inner.set_a(0x44);
            inner.set_b(0x5566);
        }
        let (tlv, _) = Tlv::parse(&buf).unwrap();
        assert_eq!(tlv.tag(), 0x99);
        assert_eq!(tlv.length() as usize, InnerWriter::SIZE);
        let inner = tlv.value().unwrap();
        assert_eq!(inner.a(), 0x44);
        assert_eq!(inner.b(), 0x5566);

        assert!(matches!(
            TlvWriter::new(&mut [0u8; 2], TlvLens { value: InnerWriter::SIZE }),
            Err(binparse::WriteError::NotEnoughSpace { .. })
        ));
    "#;
    run_round_trip("len-region-struct-ref", dsl, test_body);
}

#[test]
fn generated_writer_len_region_struct_ref_affine_round_trips() {
    // @len(length - 2): the region occupies (length - 2) bytes, so the derived
    // length = encoded_len + 2.
    let dsl = r#"
@endian(big) struct Inner { a: u8, b: u16 }
@endian(big) struct Tlv { tag: u8, length: u8, @len(length - 2) value: Inner }
"#;
    let test_body = r#"
        let content = TlvContent {
            tag: 0x42,
            value: InnerContent { a: 0x11, b: 0x2233 },
        };
        let bytes = TlvWriter::to_vec(&content);
        // length is DERIVED: Inner encoded_len (3) + 2 = 5
        assert_eq!(bytes, vec![0x42, 0x05, 0x11, 0x22, 0x33]);

        let (tlv, rem) = Tlv::parse(&bytes).unwrap();
        assert!(rem.is_empty());
        assert_eq!(tlv.length() as usize, InnerWriter::SIZE + 2);
        let inner = tlv.value().unwrap();
        assert_eq!(inner.a(), 0x11);
        assert_eq!(inner.b(), 0x2233);
    "#;
    run_round_trip("len-region-struct-ref-affine", dsl, test_body);
}

#[test]
fn generated_writer_len_region_struct_ref_with_trailer_round_trips() {
    let dsl = r#"
@endian(big) struct Inner { a: u8, b: u16 }
@endian(big) struct Tlv { tag: u8, length: u8, @len(length) value: Inner, crc: u16 }
"#;
    let test_body = r#"
        let content = TlvContent {
            tag: 0x42,
            value: InnerContent { a: 0x11, b: 0x2233 },
            crc: 0xBEEF,
        };
        let lens = TlvLens { value: InnerWriter::SIZE };
        // 1 + 1 + 3 + 2 = 7
        assert_eq!(TlvWriter::encoded_len(&lens), 7);

        let bytes = TlvWriter::to_vec(&content);
        assert_eq!(bytes, vec![0x42, 0x03, 0x11, 0x22, 0x33, 0xBE, 0xEF]);

        let (tlv, rem) = Tlv::parse(&bytes).unwrap();
        assert!(rem.is_empty());
        assert_eq!(tlv.length() as usize, InnerWriter::SIZE);
        let inner = tlv.value().unwrap();
        assert_eq!(inner.a(), 0x11);
        assert_eq!(inner.b(), 0x2233);
        // trailer sits AFTER the bounded region.
        assert_eq!(tlv.crc(), 0xBEEF);
    "#;
    run_round_trip("len-region-struct-ref-trailer", dsl, test_body);
}

#[test]
fn generated_writer_len_region_union_round_trips() {
    let dsl = r#"
@endian(big) struct Packet {
    kind: u8,
    length: u8,
    @len(length) body: union(kind) {
        1 => Connect { keep_alive: u16 },
        2 => Connack { ack: u8, code: u8 },
        _ => Unknown { },
    },
}
"#;
    let test_body = r#"
        // Connect: 2-byte body, derived length == 2.
        let content = PacketContent {
            body: PacketBodyContent::Connect(ConnectContent { keep_alive: 0x1234 }),
        };
        assert_eq!(PacketWriter::encoded_len(&content), 4);
        let bytes = PacketWriter::to_vec(&content);
        // kind (derived) = 1, length (derived) = 2, then body.
        assert_eq!(bytes, vec![0x01, 0x02, 0x12, 0x34]);
        let (packet, rem) = Packet::parse(&bytes).unwrap();
        assert!(rem.is_empty());
        assert_eq!(packet.kind(), 1);
        assert_eq!(packet.length(), 2);
        match packet.body().unwrap() {
            Packet_body::Connect(c) => assert_eq!(c.keep_alive(), 0x1234),
            _ => panic!("expected Connect"),
        }

        // Connack: 2-byte body too, but a different discriminator.
        let content = PacketContent {
            body: PacketBodyContent::Connack(ConnackContent { ack: 0xAB, code: 0xCD }),
        };
        let bytes = PacketWriter::to_vec(&content);
        assert_eq!(bytes, vec![0x02, 0x02, 0xAB, 0xCD]);
        let (packet, _) = Packet::parse(&bytes).unwrap();
        assert_eq!(packet.kind(), 2);
        // derived length == inner encoded_len
        assert_eq!(packet.length(), 2);
        match packet.body().unwrap() {
            Packet_body::Connack(c) => {
                assert_eq!(c.ack(), 0xAB);
                assert_eq!(c.code(), 0xCD);
            }
            _ => panic!("expected Connack"),
        }

        assert!(matches!(
            PacketWriter::write_into(
                &mut [0u8; 2],
                &PacketContent {
                    body: PacketBodyContent::Connect(ConnectContent { keep_alive: 1 }),
                },
            ),
            Err(binparse::WriteError::NotEnoughSpace { .. })
        ));
    "#;
    run_round_trip("len-region-union", dsl, test_body);
}

#[test]
fn generated_writer_len_region_value_too_large_errors() {
    // @len(length + 1): derived length = encoded_len - 1, into a u8 field. With a
    // large fixed child, the derived value can exceed u8::MAX.
    let dsl = r#"
@endian(big) struct Inner { data: [u8; 300] }
@endian(big) struct Tlv { tag: u8, length: u8, @len(length) value: Inner }
"#;
    let test_body = r#"
        // Inner::SIZE == 300 > u8::MAX, so the derived length field overflows.
        assert!(matches!(
            TlvWriter::new(&mut [0u8; 400], TlvLens { value: InnerWriter::SIZE }),
            Err(binparse::WriteError::ValueTooLarge { .. })
        ));
        let content = TlvContent { tag: 0x01, value: InnerContent { data: [0u8; 300] } };
        let mut buf = vec![0u8; TlvWriter::encoded_len(&TlvLens { value: InnerWriter::SIZE })];
        assert!(matches!(
            TlvWriter::write_into(&mut buf, &content),
            Err(binparse::WriteError::ValueTooLarge { .. })
        ));
    "#;
    run_round_trip("len-region-value-too-large", dsl, test_body);
}
