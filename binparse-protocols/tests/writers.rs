//! Round-trip tests for the shipped zero-copy WRITER API. For each protocol we
//! build a real packet through the generated writer, parse it back through the
//! reader, and assert field equality (pinning wire bytes where derived/constant
//! fields make them load-bearing). Each test is gated by its protocol feature so
//! the suite works under any subset (run with `--features all`).

use binparse::ParseResult;

#[cfg(feature = "ethernet")]
#[test]
fn ethernet_mode3_round_trips_and_pins_wire_bytes() {
    use binparse_protocols::ethernet::{
        EthernetII, EthernetIIContent, EthernetIILens, EthernetIIWriter,
    };

    let content = EthernetIIContent {
        dst: [0x00, 0x1b, 0x21, 0x3c, 0x9d, 0xf8],
        src: [0x00, 0x19, 0x06, 0xea, 0xb8, 0xc1],
        ethertype: 0x0800,
        payload: &[0xde, 0xad, 0xbe, 0xef],
    };
    let bytes = EthernetIIWriter::to_vec(&content);
    let expected: Vec<u8> = vec![
        0x00, 0x1b, 0x21, 0x3c, 0x9d, 0xf8, 0x00, 0x19, 0x06, 0xea, 0xb8, 0xc1, 0x08, 0x00, 0xde,
        0xad, 0xbe, 0xef,
    ];
    assert_eq!(bytes, expected);

    let lens = EthernetIILens {
        payload: content.payload.len(),
    };
    assert_eq!(EthernetIIWriter::encoded_len(&lens), 18);

    let (mut eth, rem) = EthernetII::parse(&bytes).unwrap();
    assert!(rem.is_empty());
    assert_eq!(
        eth.dst()
            .unwrap()
            .collect::<ParseResult<Vec<u8>>>()
            .unwrap(),
        vec![0x00, 0x1b, 0x21, 0x3c, 0x9d, 0xf8]
    );
    assert_eq!(
        eth.src()
            .unwrap()
            .collect::<ParseResult<Vec<u8>>>()
            .unwrap(),
        vec![0x00, 0x19, 0x06, 0xea, 0xb8, 0xc1]
    );
    assert_eq!(eth.ethertype(), 0x0800);
    assert_eq!(
        eth.payload()
            .unwrap()
            .collect::<ParseResult<Vec<u8>>>()
            .unwrap(),
        vec![0xde, 0xad, 0xbe, 0xef]
    );
}

#[cfg(feature = "ethernet")]
#[test]
fn ethernet_mode1_setters_and_mut_slices() {
    use binparse_protocols::ethernet::{EthernetIILens, EthernetIIWriter};

    let lens = EthernetIILens { payload: 4 };
    let mut buf = vec![0u8; EthernetIIWriter::encoded_len(&lens)];
    let mut w = EthernetIIWriter::new(&mut buf, lens).unwrap();
    w.dst_mut()
        .copy_from_slice(&[0x00, 0x1b, 0x21, 0x3c, 0x9d, 0xf8]);
    w.src_mut()
        .copy_from_slice(&[0x00, 0x19, 0x06, 0xea, 0xb8, 0xc1]);
    w.set_ethertype(0x0800);
    w.payload_mut().copy_from_slice(&[0xde, 0xad, 0xbe, 0xef]);

    let expected: Vec<u8> = vec![
        0x00, 0x1b, 0x21, 0x3c, 0x9d, 0xf8, 0x00, 0x19, 0x06, 0xea, 0xb8, 0xc1, 0x08, 0x00, 0xde,
        0xad, 0xbe, 0xef,
    ];
    assert_eq!(buf, expected);

    assert!(matches!(
        EthernetIIWriter::new(&mut [0u8; 10], EthernetIILens { payload: 4 }),
        Err(binparse::WriteError::NotEnoughSpace { .. })
    ));
}

#[cfg(feature = "vlan")]
#[test]
fn vlan_round_trips_and_derives_constant_tpid() {
    use binparse_protocols::vlan::{Vlan, VlanContent, VlanWriter};

    let content = VlanContent {
        dst: [1, 2, 3, 4, 5, 6],
        src: [10, 11, 12, 13, 14, 15],
        pcp: 5,
        dei: 1,
        vid_hi: 0xa,
        vid_lo: 0xbc,
        ethertype: 0x0800,
        payload: &[0xde, 0xad, 0xbe, 0xef],
    };
    let bytes = VlanWriter::to_vec(&content);
    // tpid is a derived constant (no setter): the writer must emit 0x8100 itself.
    assert_eq!(&bytes[12..14], &[0x81, 0x00]);

    let (mut vlan, rem) = Vlan::parse(&bytes).unwrap();
    assert!(rem.is_empty());
    assert_eq!(
        vlan.dst()
            .unwrap()
            .collect::<ParseResult<Vec<u8>>>()
            .unwrap(),
        vec![1, 2, 3, 4, 5, 6]
    );
    assert_eq!(
        vlan.src()
            .unwrap()
            .collect::<ParseResult<Vec<u8>>>()
            .unwrap(),
        vec![10, 11, 12, 13, 14, 15]
    );
    assert_eq!(vlan.tpid(), 0x8100);
    assert_eq!(vlan.pcp(), 5);
    assert_eq!(vlan.dei(), 1);
    assert_eq!(vlan.vid_hi(), 0xa);
    assert_eq!(vlan.vid_lo(), 0xbc);
    assert_eq!(vlan.ethertype(), 0x0800);
    assert_eq!(
        vlan.payload()
            .unwrap()
            .collect::<ParseResult<Vec<u8>>>()
            .unwrap(),
        vec![0xde, 0xad, 0xbe, 0xef]
    );
}

#[cfg(feature = "ip")]
#[test]
fn ipv6_round_trips_and_derives_payload_len() {
    use binparse_protocols::ip::{Ipv6, Ipv6Content, Ipv6Lens, Ipv6Writer};

    let payload: Vec<u8> = vec![
        0xc3, 0x50, 0x00, 0x35, 0x00, 0x0c, 0x1a, 0x2b, 0xde, 0xad, 0xbe, 0xef,
    ];
    let content = Ipv6Content {
        version: 6,
        tc_hi: 0,
        tc_lo: 0,
        flow_hi: 0,
        flow_lo: 0,
        next_header: 17,
        hop_limit: 64,
        src: [
            0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x01,
        ],
        dst: [
            0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x02,
        ],
        payload: &payload,
    };
    let lens = Ipv6Lens {
        payload: payload.len(),
    };
    assert_eq!(Ipv6Writer::encoded_len(&lens), 40 + payload.len());

    let bytes = Ipv6Writer::to_vec(&content);
    assert_eq!(bytes[0] >> 4, 6);
    // payload_len is derived from the payload region (no setter): 12 == 0x000c.
    assert_eq!(&bytes[4..6], &[0x00, 0x0c]);

    let (mut ip, rem) = Ipv6::parse(&bytes).unwrap();
    assert!(rem.is_empty());
    assert_eq!(ip.version(), 6);
    assert_eq!(ip.payload_len(), 12);
    assert_eq!(ip.next_header(), 17);
    assert_eq!(ip.hop_limit(), 64);
    assert_eq!(
        ip.src().unwrap().collect::<ParseResult<Vec<u8>>>().unwrap(),
        vec![
            0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x01
        ]
    );
    assert_eq!(
        ip.dst().unwrap().collect::<ParseResult<Vec<u8>>>().unwrap()[15],
        0x02
    );
    assert_eq!(
        ip.payload()
            .unwrap()
            .collect::<ParseResult<Vec<u8>>>()
            .unwrap(),
        payload
    );
}

#[cfg(feature = "udp")]
#[test]
fn udp_round_trips_affine_length_region() {
    use binparse_protocols::udp::{Udp, UdpContent, UdpLens, UdpWriter};

    let payload: Vec<u8> = vec![0xde, 0xad, 0xbe, 0xef];
    let content = UdpContent {
        src_port: 50000,
        dst_port: 53,
        checksum: 0x1a2b,
        payload: &payload,
    };
    // length is derived as payload + 8 (the affine `[u8; length - 8]` region).
    let lens = UdpLens {
        payload: payload.len(),
    };
    assert_eq!(UdpWriter::encoded_len(&lens), 8 + payload.len());

    let bytes = UdpWriter::to_vec(&content);
    let expected: Vec<u8> = vec![
        0xc3, 0x50, 0x00, 0x35, 0x00, 0x0c, 0x1a, 0x2b, 0xde, 0xad, 0xbe, 0xef,
    ];
    assert_eq!(bytes, expected);

    let (mut udp, rem) = Udp::parse(&bytes).unwrap();
    assert!(rem.is_empty());
    assert_eq!(udp.src_port(), 50000);
    assert_eq!(udp.dst_port(), 53);
    assert_eq!(udp.length(), 12);
    assert_eq!(udp.checksum(), 0x1a2b);
    assert_eq!(
        udp.payload()
            .unwrap()
            .collect::<ParseResult<Vec<u8>>>()
            .unwrap(),
        payload
    );
}

#[cfg(feature = "udp")]
#[test]
fn udp_mode2_writer_over_edits_fixed_field_in_place() {
    use binparse_protocols::udp::{Udp, UdpWriter};

    // A valid 12-byte UDP datagram (src=50000, dst=53, len=12, 4-byte payload).
    let mut buf: Vec<u8> = vec![
        0xc3, 0x50, 0x00, 0x35, 0x00, 0x0c, 0x1a, 0x2b, 0xde, 0xad, 0xbe, 0xef,
    ];
    {
        let mut w = UdpWriter::writer_over(&mut buf).unwrap();
        w.set_dst_port(80);
    }
    // The edited fixed field took effect; derived length and payload untouched.
    let (mut udp, rem) = Udp::parse(&buf).unwrap();
    assert!(rem.is_empty());
    assert_eq!(udp.src_port(), 50000);
    assert_eq!(udp.dst_port(), 80);
    assert_eq!(udp.length(), 12);
    assert_eq!(
        udp.payload()
            .unwrap()
            .collect::<ParseResult<Vec<u8>>>()
            .unwrap(),
        vec![0xde, 0xad, 0xbe, 0xef]
    );
}

#[cfg(feature = "tcp")]
#[test]
fn tcp_option_union_round_trips_fixed_variants() {
    use binparse_protocols::tcp::{
        EolContent, NopContent, TcpOption, TcpOption_body, TcpOptionBodyContent, TcpOptionContent,
        TcpOptionWriter,
    };

    let nop = TcpOptionContent {
        body: TcpOptionBodyContent::Nop(NopContent {}),
    };
    let bytes = TcpOptionWriter::to_vec(&nop);
    assert_eq!(bytes, vec![0x01]);
    let (mut opt, rem) = TcpOption::parse(&bytes).unwrap();
    assert!(rem.is_empty());
    assert_eq!(opt.kind(), 1);
    assert!(matches!(opt.body().unwrap(), TcpOption_body::Nop(_)));

    let eol = TcpOptionContent {
        body: TcpOptionBodyContent::Eol(EolContent {}),
    };
    let bytes = TcpOptionWriter::to_vec(&eol);
    assert_eq!(bytes, vec![0x00]);
    let (mut opt, rem) = TcpOption::parse(&bytes).unwrap();
    assert!(rem.is_empty());
    assert_eq!(opt.kind(), 0);
    assert!(matches!(opt.body().unwrap(), TcpOption_body::Eol(_)));
}

#[cfg(feature = "dhcp")]
#[test]
fn dhcp_option_union_round_trips_fixed_variants() {
    use binparse_protocols::dhcp::{
        DhcpOption, DhcpOption_body, DhcpOptionBodyContent, DhcpOptionContent, DhcpOptionWriter,
        EndContent, PadContent,
    };

    let pad = DhcpOptionContent {
        body: DhcpOptionBodyContent::Pad(PadContent {}),
    };
    let bytes = DhcpOptionWriter::to_vec(&pad);
    assert_eq!(bytes, vec![0x00]);
    let (mut opt, rem) = DhcpOption::parse(&bytes).unwrap();
    assert!(rem.is_empty());
    assert_eq!(opt.code(), 0);
    assert!(matches!(opt.body().unwrap(), DhcpOption_body::Pad(_)));

    let end = DhcpOptionContent {
        body: DhcpOptionBodyContent::End(EndContent {}),
    };
    let bytes = DhcpOptionWriter::to_vec(&end);
    assert_eq!(bytes, vec![0xff]);
    let (mut opt, rem) = DhcpOption::parse(&bytes).unwrap();
    assert!(rem.is_empty());
    assert_eq!(opt.code(), 255);
    assert!(matches!(opt.body().unwrap(), DhcpOption_body::End(_)));
}

#[cfg(feature = "mqtt_v3")]
#[test]
fn mqtt_v3_connack_round_trips_varint_len_union() {
    use binparse_protocols::mqtt_v3::{
        ConnackContent, MqttPacket, MqttPacketBodyContent, MqttPacketContent, MqttPacketWriter,
        MqttPacket_body,
    };

    let content = MqttPacketContent {
        flags: 0,
        body: MqttPacketBodyContent::Connack(ConnackContent {
            ack_flags: 0,
            return_code: 0,
        }),
    };
    let bytes = MqttPacketWriter::to_vec(&content);
    assert_eq!(bytes, vec![0x20, 0x02, 0x00, 0x00]);

    let (mut mqtt, rem) = MqttPacket::parse(&bytes).unwrap();
    assert!(rem.is_empty());
    assert_eq!(mqtt.packet_type(), 2);
    assert_eq!(*mqtt.remaining_length().unwrap(), 2);
    match mqtt.body().unwrap() {
        MqttPacket_body::Connack(c) => {
            assert_eq!(c.ack_flags(), 0);
            assert_eq!(c.return_code(), 0);
        }
        _ => panic!("expected Connack"),
    }
}

#[cfg(feature = "mqtt_v3")]
#[test]
fn mqtt_v3_pingreq_round_trips_empty_variant_zero_varint() {
    use binparse_protocols::mqtt_v3::{
        MqttPacket, MqttPacketBodyContent, MqttPacketContent, MqttPacketWriter, MqttPacket_body,
        PingReqContent,
    };

    let content = MqttPacketContent {
        flags: 0,
        body: MqttPacketBodyContent::PingReq(PingReqContent {}),
    };
    let bytes = MqttPacketWriter::to_vec(&content);
    assert_eq!(bytes, vec![0xc0, 0x00]);

    let (mut mqtt, rem) = MqttPacket::parse(&bytes).unwrap();
    assert!(rem.is_empty());
    assert_eq!(mqtt.packet_type(), 12);
    assert_eq!(*mqtt.remaining_length().unwrap(), 0);
    assert!(matches!(mqtt.body().unwrap(), MqttPacket_body::PingReq(_)));
}
