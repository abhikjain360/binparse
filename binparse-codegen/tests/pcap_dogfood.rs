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

fn corpus_dir() -> PathBuf {
    workspace_root()
        .join("third_party")
        .join("wireshark")
        .join("test")
        .join("captures")
}

fn write_runtime_crate(code: &str, corpus: &str) -> PathBuf {
    let root = workspace_root();
    let test_dir = root
        .join("target")
        .join("generated-runtime-tests")
        .join(format!("pcap-dogfood-{}", std::process::id()));

    let _ = fs::remove_dir_all(&test_dir);
    fs::create_dir_all(test_dir.join("src")).expect("failed to create runtime test crate");

    let binparse_path = root.join("binparse");
    fs::write(
        test_dir.join("Cargo.toml"),
        format!(
            r#"[package]
name = "pcap-dogfood-test"
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
    use std::fs;
    use std::path::Path;

    const CORPUS: &str = "{corpus}";

    const PCAPNG_MAGIC: [u8; 4] = [0x0a, 0x0d, 0x0d, 0x0a];

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

    #[derive(Clone, Copy, PartialEq)]
    enum Endian {{
        Big,
        Little,
    }}

    fn classify_magic(data: &[u8]) -> Option<Endian> {{
        let head: [u8; 4] = data.get(0..4)?.try_into().ok()?;
        let magic = u32::from_be_bytes(head);
        match magic {{
            0xa1b2c3d4 | 0xa1b23c4d => Some(Endian::Big),
            0xd4c3b2a1 | 0x4d3cb2a1 => Some(Endian::Little),
            _ => None,
        }}
    }}

    fn build_pcap(endian: Endian, network: u32, records: &[(u32, u32, &[u8])]) -> Vec<u8> {{
        let mut out = Vec::new();
        let w32 = |out: &mut Vec<u8>, v: u32| match endian {{
            Endian::Big => out.extend_from_slice(&v.to_be_bytes()),
            Endian::Little => out.extend_from_slice(&v.to_le_bytes()),
        }};
        let w16 = |out: &mut Vec<u8>, v: u16| match endian {{
            Endian::Big => out.extend_from_slice(&v.to_be_bytes()),
            Endian::Little => out.extend_from_slice(&v.to_le_bytes()),
        }};
        match endian {{
            Endian::Big => out.extend_from_slice(&0xa1b2c3d4u32.to_be_bytes()),
            Endian::Little => out.extend_from_slice(&0xa1b2c3d4u32.to_le_bytes()),
        }}
        w16(&mut out, 2);
        w16(&mut out, 4);
        w32(&mut out, 0);
        w32(&mut out, 0);
        w32(&mut out, 65535);
        w32(&mut out, network);
        for (ts_sec, ts_usec, payload) in records {{
            w32(&mut out, *ts_sec);
            w32(&mut out, *ts_usec);
            w32(&mut out, payload.len() as u32);
            w32(&mut out, payload.len() as u32);
            out.extend_from_slice(payload);
        }}
        out
    }}

    struct Record {{
        ts_sec: u32,
        ts_usec: u32,
        incl_len: u32,
        orig_len: u32,
        data: Vec<u8>,
    }}

    fn body_records(body: &Pcap_body<'_>) -> binparse::ParseResult<Vec<Record>> {{
        match body {{
            Pcap_body::Be(be) => be
                .records()?
                .map(|rec| {{
                    let rec = rec?;
                    let _ = rec.field_tree();
                    let data = rec.data()?.collect::<binparse::ParseResult<Vec<u8>>>()?;
                    Ok(Record {{
                        ts_sec: rec.ts_sec(),
                        ts_usec: rec.ts_usec(),
                        incl_len: rec.incl_len(),
                        orig_len: rec.orig_len(),
                        data,
                    }})
                }})
                .collect(),
            Pcap_body::Le(le) => le
                .records()?
                .map(|rec| {{
                    let rec = rec?;
                    let _ = rec.field_tree();
                    let data = rec.data()?.collect::<binparse::ParseResult<Vec<u8>>>()?;
                    Ok(Record {{
                        ts_sec: rec.ts_sec(),
                        ts_usec: rec.ts_usec(),
                        incl_len: rec.incl_len(),
                        orig_len: rec.orig_len(),
                        data,
                    }})
                }})
                .collect(),
        }}
    }}

    fn body_network(body: &Pcap_body<'_>) -> u32 {{
        match body {{
            Pcap_body::Be(be) => be.network(),
            Pcap_body::Le(le) => le.network(),
        }}
    }}

    fn body_version_major(body: &Pcap_body<'_>) -> u16 {{
        match body {{
            Pcap_body::Be(be) => be.version_major(),
            Pcap_body::Le(le) => le.version_major(),
        }}
    }}

    #[test]
    fn valid_two_record_file_decodes() {{
        let p0: &[u8] = &[0xde, 0xad, 0xbe, 0xef];
        let p1: &[u8] = &[1, 2, 3, 4, 5, 6, 7];
        for endian in [Endian::Little, Endian::Big] {{
            let bytes =
                build_pcap(endian, 1, &[(0x1111_2222, 0x33, p0), (0x4444_5555, 0x66, p1)]);
            assert!(classify_magic(&bytes) == Some(endian));
            let (pcap, rem) = Pcap::parse(&bytes).unwrap();
            assert!(rem.is_empty());
            let body = pcap.body().unwrap();
            assert_eq!(body_version_major(&body), 2);
            assert_eq!(body_network(&body), 1);

            let records = body_records(&body).unwrap();
            assert_eq!(records.len(), 2);
            assert_eq!(records[0].ts_sec, 0x1111_2222);
            assert_eq!(records[0].ts_usec, 0x33);
            assert_eq!(records[0].incl_len, p0.len() as u32);
            assert_eq!(records[0].orig_len, p0.len() as u32);
            assert_eq!(records[0].data, p0);
            assert_eq!(records[1].ts_sec, 0x4444_5555);
            assert_eq!(records[1].ts_usec, 0x66);
            assert_eq!(records[1].incl_len, p1.len() as u32);
            assert_eq!(records[1].data, p1);
        }}
    }}

    #[test]
    fn truncated_record_errors() {{
        let p0: &[u8] = &[0xaa; 10];
        let bytes = build_pcap(Endian::Little, 1, &[(0, 0, p0)]);
        let truncated = &bytes[..bytes.len() - 3];
        match Pcap::parse(truncated) {{
            Ok((pcap, _)) => {{
                let body = pcap.body().unwrap();
                match body_records(&body) {{
                    Ok(records) => assert!(
                        records.iter().all(|r| r.data.len() == r.incl_len as usize),
                        "truncated record must not yield a short payload silently"
                    ),
                    Err(err) => assert!(matches!(
                        err,
                        binparse::ParseError::NotEnoughData {{ .. }}
                    )),
                }}
            }}
            Err(err) => {{
                assert!(matches!(err, binparse::ParseError::NotEnoughData {{ .. }}));
            }}
        }}
    }}

    #[test]
    fn bad_magic_is_rejected() {{
        let mut bytes = build_pcap(Endian::Little, 1, &[(0, 0, &[1, 2, 3])]);
        bytes[0] = 0x00;
        bytes[1] = 0x11;
        bytes[2] = 0x22;
        bytes[3] = 0x33;
        assert!(classify_magic(&bytes).is_none());
        let (pcap, _) = Pcap::parse(&bytes).unwrap();
        assert!(matches!(pcap.body(), Err(Error::BadMagic {{ .. }})));
    }}

    #[test]
    fn field_tree_smoke() {{
        let p0: &[u8] = &[0xca, 0xfe];
        let bytes = build_pcap(Endian::Big, 1, &[(0x99, 0x88, p0)]);
        let (pcap, _) = Pcap::parse(&bytes).unwrap();
        let mut tree = pcap.field_tree();
        tree.set_paths("");
        assert_eq!(tree.name, "Pcap");
        let names: Vec<&str> = tree.children.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"magic"));
        assert!(names.contains(&"body"));
        let magic = tree
            .children
            .iter()
            .find(|c| c.name == "magic")
            .expect("magic node");
        assert_eq!(magic.bit_range, 0..32);
        assert_eq!(magic.value, binparse::Value::UInt(0xa1b2c3d4));
    }}

    #[test]
    fn truncated_prefix_never_panics() {{
        let bytes = build_pcap(Endian::Little, 1, &[(0x1111, 0x22, &[0xde, 0xad, 0xbe, 0xef])]);
        assert_parse_no_panic("Pcap", &bytes, |data| {{
            if let Ok((pcap, _)) = Pcap::parse(data)
                && let Ok(body) = pcap.body()
            {{
                let _ = body_records(&body);
                let _ = pcap.field_tree();
            }}
        }});
    }}

    #[derive(Default)]
    struct SoakStats {{
        files_parsed: usize,
        files_failed: usize,
        files_skipped: usize,
        packets: usize,
        ethernet_ok: usize,
        ipv4_ok: usize,
        udp_ok: usize,
        tcp_ok: usize,
    }}

    fn dissect_ethernet(stats: &mut SoakStats, frame: &[u8]) {{
        let Ok((eth, _)) = EthernetHdr::parse(frame) else {{
            return;
        }};
        stats.ethernet_ok += 1;
        let _ = eth.field_tree();
        if frame.len() < 14 {{
            return;
        }}
        if eth.ethertype() == 0x0800 {{
            dissect_ipv4(stats, &frame[14..]);
        }}
    }}

    fn dissect_ipv4(stats: &mut SoakStats, packet: &[u8]) {{
        let Ok((ip, _)) = Ipv4Hdr::parse(packet) else {{
            return;
        }};
        if ip.version() != 4 {{
            return;
        }}
        stats.ipv4_ok += 1;
        let _ = ip.field_tree();
        let ihl = ip.ihl() as usize;
        if ihl < 5 {{
            return;
        }}
        let header_len = ihl * 4;
        let total_len = ip.total_len() as usize;
        if header_len > packet.len() || total_len < header_len || total_len > packet.len() {{
            return;
        }}
        let payload = &packet[header_len..total_len];
        match ip.proto() {{
            17 => {{
                if let Ok((udp, _)) = UdpHdr::parse(payload) {{
                    stats.udp_ok += 1;
                    let _ = udp.field_tree();
                }}
            }}
            6 => {{
                if let Ok((tcp, _)) = TcpHdr::parse(payload) {{
                    stats.tcp_ok += 1;
                    let _ = tcp.field_tree();
                }}
            }}
            _ => {{}}
        }}
    }}

    fn soak_one_file(stats: &mut SoakStats, bytes: &[u8]) {{
        let pcap = match Pcap::parse(bytes) {{
            Ok((pcap, _)) => pcap,
            Err(_) => {{
                stats.files_failed += 1;
                return;
            }}
        }};
        let _ = pcap.field_tree();
        let body = match pcap.body() {{
            Ok(body) => body,
            Err(_) => {{
                stats.files_failed += 1;
                return;
            }}
        }};
        let network = body_network(&body);
        let records = match body_records(&body) {{
            Ok(records) => records,
            Err(_) => {{
                stats.files_failed += 1;
                return;
            }}
        }};
        stats.files_parsed += 1;
        for record in records {{
            stats.packets += 1;
            let _ = (record.ts_sec, record.ts_usec, record.orig_len, record.incl_len);
            if network == 1 {{
                dissect_ethernet(stats, &record.data);
            }}
        }}
    }}

    #[test]
    fn soak_prefix_never_panics() {{
        let bytes = build_pcap(Endian::Big, 1, &[(0x1, 0x2, &[0xde, 0xad, 0xbe, 0xef])]);
        for len in 0..=bytes.len() {{
            let mut stats = SoakStats::default();
            soak_one_file(&mut stats, &bytes[..len]);
        }}
    }}

    #[test]
    fn soak_over_wireshark_corpus() {{
        let dir = Path::new(CORPUS);
        if !dir.is_dir() {{
            eprintln!("soak: corpus dir {{CORPUS}} missing, skipping");
            return;
        }}
        let entries = match fs::read_dir(dir) {{
            Ok(e) => e,
            Err(e) => {{
                eprintln!("soak: cannot read {{CORPUS}}: {{e}}, skipping");
                return;
            }}
        }};

        let mut stats = SoakStats::default();
        for entry in entries.flatten() {{
            let path = entry.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !name.ends_with(".pcap") {{
                continue;
            }}
            let bytes = match fs::read(&path) {{
                Ok(b) => b,
                Err(_) => continue,
            }};
            if bytes.get(0..4) == Some(&PCAPNG_MAGIC) {{
                stats.files_skipped += 1;
                continue;
            }}
            if classify_magic(&bytes).is_none() {{
                stats.files_skipped += 1;
                continue;
            }}
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {{
                soak_one_file(&mut stats, &bytes);
            }}));
            assert!(result.is_ok(), "panic while soaking {{name}}");
        }}

        eprintln!(
            "soak summary: parsed={{}} failed={{}} skipped={{}} packets={{}} eth_ok={{}} ipv4_ok={{}} udp_ok={{}} tcp_ok={{}}",
            stats.files_parsed,
            stats.files_failed,
            stats.files_skipped,
            stats.packets,
            stats.ethernet_ok,
            stats.ipv4_ok,
            stats.udp_ok,
            stats.tcp_ok,
        );

        assert!(
            stats.files_parsed > 0,
            "expected at least one classic pcap file in corpus"
        );
    }}
}}
"#
        ),
    )
    .expect("failed to write runtime lib.rs");

    test_dir
}

#[test]
fn pcap_dogfood_compiles_and_soaks_corpus() {
    let dsl = r#"
error {
    BadMagic { magic: u32 },
}

struct PcapRecBe {
    ts_sec: u32,
    ts_usec: u32,
    incl_len: u32,
    orig_len: u32,
    data: [u8; incl_len],
}

@endian(little)
struct PcapRecLe {
    ts_sec: u32,
    ts_usec: u32,
    incl_len: u32,
    orig_len: u32,
    data: [u8; incl_len],
}

struct Pcap {
    magic: u32,
    body: union(magic) {
        2712847316 | 2712812621 => Be {
            @range(2, 2) version_major: u16,
            version_minor: u16,
            thiszone: i32,
            sigfigs: u32,
            snaplen: u32,
            network: u32,
            @greedy(unsafe_eof) @max_iter(200000) records: [PcapRecBe],
        },
        3569595041 | 1295823521 => @endian(little) Le {
            @range(2, 2) version_major: u16,
            version_minor: u16,
            thiszone: i32,
            sigfigs: u32,
            snaplen: u32,
            network: u32,
            @greedy(unsafe_eof) @max_iter(200000) records: [PcapRecLe],
        },
        _ => @error(BadMagic { magic: magic }),
    },
}

struct EthernetHdr {
    eth_dst: [u8; 6],
    eth_src: [u8; 6],
    ethertype: u16,
    @greedy(unsafe_eof) payload: [u8],
}

struct Ipv4Hdr {
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
    proto: u8,
    checksum: u16,
    ip_src: [u8; 4],
    ip_dst: [u8; 4],
}

struct UdpHdr {
    src_port: u16,
    dst_port: u16,
    length: u16,
    checksum: u16,
}

struct TcpHdr {
    src_port: u16,
    dst_port: u16,
    seq: u32,
    ack_no: u32,
    @range(5, 15) data_offset: b<4>,
    reserved: b<3>,
    ns: b<1>,
    flags: u8,
    window: u16,
    checksum: u16,
    urgent_ptr: u16,
}
"#;

    let code = generated_code(dsl);
    let corpus = corpus_dir();
    let corpus_str = corpus.to_string_lossy().replace('\\', "\\\\");
    let test_dir = write_runtime_crate(&code, &corpus_str);
    let output = Command::new("cargo")
        .arg("test")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(test_dir.join("Cargo.toml"))
        .arg("--")
        .arg("--nocapture")
        .output()
        .expect("failed to run pcap dogfood runtime tests");

    assert!(
        output.status.success(),
        "pcap dogfood runtime tests failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
