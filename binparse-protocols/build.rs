use std::{env, fs, path::PathBuf};

const PROTOCOLS: &[&str] = &[
    "ethernet", "arp", "vlan", "ip", "icmp", "icmpv6", "udp", "tcp", "dns", "tls", "dhcp", "sctp",
    "bgp", "mqtt_v3", "mqtt_v5",
];

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let specs_dir = manifest_dir.join("specs");
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));

    println!("cargo:rerun-if-changed=build.rs");

    let mut generated = String::new();
    for proto in PROTOCOLS {
        let spec_path = specs_dir.join(format!("{proto}.bp"));
        println!("cargo:rerun-if-changed={}", spec_path.display());

        let feature_env = format!("CARGO_FEATURE_{}", proto.to_uppercase());
        if env::var_os(&feature_env).is_none() {
            continue;
        }

        let dsl = fs::read_to_string(&spec_path)
            .unwrap_or_else(|e| panic!("failed to read spec {}: {e}", spec_path.display()));
        let ast = binparse_dsl_parse::parse_str(&dsl)
            .unwrap_or_else(|e| panic!("failed to parse spec {proto}:\n{e}"));
        let code = binparse_codegen::CodeGen::generate_writers(&ast)
            .unwrap_or_else(|e| panic!("failed to generate code for spec {proto}: {e}"));

        generated.push_str(&format!(
            "pub mod {proto} {{\n#![allow(clippy::all, unused)]\n{code}\n}}\n"
        ));
    }

    fs::write(out_dir.join("protocols.rs"), generated).expect("failed to write generated protocols");
}
