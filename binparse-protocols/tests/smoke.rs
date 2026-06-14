//! One real-packet smoke test per protocol. Each is gated by its feature so the
//! suite works under any feature subset (run with `--features all`).

#[cfg(feature = "ethernet")]
#[test]
fn ethernet_parses_arp_frame() {
    use binparse::ParseResult;
    use binparse_protocols::ethernet::EthernetII;
    let mut frame = vec![0xff, 0xff, 0xff, 0xff, 0xff, 0xff];
    frame.extend([0x00, 0x0b, 0x82, 0x01, 0xfc, 0x42]);
    frame.extend([0x08, 0x06]);
    frame.extend([0xde, 0xad, 0xbe, 0xef]);
    let (mut eth, rem) = EthernetII::parse(&frame).unwrap();
    assert!(rem.is_empty());
    assert_eq!(eth.ethertype(), 0x0806);
    let payload = eth
        .payload()
        .unwrap()
        .collect::<ParseResult<Vec<_>>>()
        .unwrap();
    assert_eq!(payload, vec![0xde, 0xad, 0xbe, 0xef]);
}

#[cfg(feature = "vlan")]
#[test]
fn vlan_parses() {
    use binparse_protocols::vlan::Vlan;
    let mut frame = vec![0x00, 0x1b, 0x21, 0x3c, 0x9d, 0xf8];
    frame.extend([0x00, 0x19, 0x06, 0xea, 0xb8, 0xc1]);
    frame.extend([0x81, 0x00, 0x00, 0x64, 0x08, 0x00]);
    frame.extend([0xde, 0xad, 0xbe, 0xef]);
    let (mut vlan, rem) = Vlan::parse(&frame).unwrap();
    assert!(rem.is_empty());
    assert_eq!(vlan.ethertype(), 0x0800);
}

#[cfg(feature = "arp")]
#[test]
fn arp_parses() {
    use binparse_protocols::arp::Arp;
    let packet = vec![
        0x00, 0x01, 0x08, 0x00, 0x06, 0x04, 0x00, 0x01, 0x00, 0x0b, 0x82, 0x01, 0xfc, 0x42, 0xc0,
        0xa8, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xc0, 0xa8, 0x00, 0x02,
    ];
    let (mut arp, rem) = Arp::parse(&packet).unwrap();
    assert!(rem.is_empty());
    assert_eq!(arp.oper(), 1);
    assert_eq!(arp.ptype(), 0x0800);
}

#[cfg(feature = "ip")]
#[test]
fn ipv4_parses() {
    use binparse_protocols::ip::Ipv4;
    let packet = vec![
        0x45, 0x00, 0x00, 0x1c, 0x1c, 0x46, 0x40, 0x00, 0x40, 0x11, 0x00, 0x00, 0xac, 0x10, 0x0a,
        0x63, 0xac, 0x10, 0x0a, 0x0c, 0xde, 0xad, 0xbe, 0xef, 0x00, 0x00, 0x00, 0x00,
    ];
    let (mut ip, rem) = Ipv4::parse(&packet).unwrap();
    assert!(rem.is_empty());
    assert_eq!(ip.version(), 4);
    assert_eq!(ip.ihl(), 5);
    assert_eq!(ip.proto(), 17);
}

#[cfg(feature = "ip")]
#[test]
fn ipv6_parses() {
    use binparse_protocols::ip::Ipv6;
    let mut packet = vec![0x60, 0x00, 0x00, 0x00, 0x00, 0x04, 0x11, 0x40];
    packet.extend([
        0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x01,
    ]);
    packet.extend([
        0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x02,
    ]);
    packet.extend([0xde, 0xad, 0xbe, 0xef]);
    let (mut ip, rem) = Ipv6::parse(&packet).unwrap();
    assert!(rem.is_empty());
    assert_eq!(ip.version(), 6);
    assert_eq!(ip.next_header(), 17);
}

#[cfg(feature = "icmp")]
#[test]
fn icmpv4_echo_parses() {
    use binparse_protocols::icmp::{Icmpv4, Icmpv4_body};
    let mut packet = vec![0x08, 0x00, 0xf7, 0x4b, 0x00, 0x01, 0x00, 0x09];
    packet.extend([b'a', b'b', b'c', b'd']);
    let (mut icmp, rem) = Icmpv4::parse(&packet).unwrap();
    assert!(rem.is_empty());
    assert_eq!(icmp.icmp_type(), 8);
    assert!(matches!(icmp.body().unwrap(), Icmpv4_body::Echo(_)));
}

#[cfg(feature = "icmpv6")]
#[test]
fn icmpv6_echo_parses() {
    use binparse_protocols::icmpv6::{Icmpv6, Icmpv6_body};
    let mut packet = vec![0x80, 0x00, 0x12, 0x34, 0x00, 0x01, 0x00, 0x09];
    packet.extend([0xde, 0xad, 0xbe, 0xef]);
    let (mut icmp, rem) = Icmpv6::parse(&packet).unwrap();
    assert!(rem.is_empty());
    assert_eq!(icmp.icmp_type(), 128);
    match icmp.body().unwrap() {
        Icmpv6_body::Echo(mut echo) => {
            assert_eq!(echo.id(), 1);
            assert_eq!(echo.seq(), 9);
        }
        _ => panic!("expected Echo body"),
    }
}

#[cfg(feature = "udp")]
#[test]
fn udp_parses() {
    use binparse_protocols::udp::Udp;
    let packet = vec![
        0xc3, 0x50, 0x00, 0x35, 0x00, 0x0c, 0x1a, 0x2b, 0xde, 0xad, 0xbe, 0xef,
    ];
    let (mut udp, rem) = Udp::parse(&packet).unwrap();
    assert!(rem.is_empty());
    assert_eq!(udp.src_port(), 50000);
    assert_eq!(udp.dst_port(), 53);
    assert_eq!(udp.length(), 12);
}

#[cfg(feature = "tcp")]
#[test]
fn tcp_parses() {
    use binparse_protocols::tcp::Tcp;
    let packet = vec![
        0xc2, 0x09, 0x00, 0x50, 0x5e, 0x4a, 0x1b, 0x3d, 0x11, 0x22, 0x33, 0x44, 0x50, 0x10, 0xfa,
        0xf0, 0x12, 0x34, 0x00, 0x00, b'G', b'E', b'T', b' ',
    ];
    let (mut tcp, rem) = Tcp::parse(&packet).unwrap();
    assert!(rem.is_empty());
    assert_eq!(tcp.dst_port(), 80);
    assert_eq!(tcp.data_offset(), 5);
    assert_eq!(tcp.ack(), 1);
}

#[cfg(feature = "dns")]
#[test]
fn dns_parses_with_compression() {
    use binparse_protocols::dns::Dns;
    let packet = vec![
        0x12, 0x34, 0x81, 0x80, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 7, b'e', b'x',
        b'a', b'm', b'p', b'l', b'e', 3, b'c', b'o', b'm', 0, 0x00, 0x01, 0x00, 0x01, 0xc0, 0x0c,
        0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x0e, 0x10, 0x00, 0x04, 0x5d, 0xb8, 0xd8, 0x22,
    ];
    let (mut dns, rem) = Dns::parse(&packet).unwrap();
    assert!(rem.is_empty());
    let qlabels: Vec<&[u8]> = dns.qname().unwrap().labels().collect();
    assert_eq!(qlabels, vec![b"example".as_slice(), b"com".as_slice()]);
    // `aname` is a compression pointer back to `qname`; the lazy iterator
    // follows it and yields the same labels with no allocation.
    let alabels: Vec<&[u8]> = dns.aname().unwrap().labels().collect();
    assert_eq!(alabels, vec![b"example".as_slice(), b"com".as_slice()]);
}

#[cfg(feature = "tls")]
#[test]
fn tls_record_parses() {
    use binparse_protocols::tls::TlsRecord;
    let packet = vec![0x16, 0x03, 0x01, 0x00, 0x05, 0x01, 0x00, 0x00, 0x01, 0x00];
    let (mut rec, rem) = TlsRecord::parse(&packet).unwrap();
    assert!(rem.is_empty());
    assert_eq!(rec.content_type(), 22);
    assert_eq!(rec.length(), 5);
}

#[cfg(feature = "dhcp")]
#[test]
fn dhcp_discover_parses() {
    use binparse_protocols::dhcp::Dhcp;
    let mut packet = vec![0x01, 0x01, 0x06, 0x00];
    packet.extend([0x39, 0x03, 0xf3, 0x26]);
    packet.extend([0x00, 0x00]);
    packet.extend([0x00, 0x00]);
    packet.extend([0u8; 16]);
    packet.extend([0u8; 16]);
    packet.extend([0u8; 64]);
    packet.extend([0u8; 128]);
    packet.extend([0x63, 0x82, 0x53, 0x63]);
    packet.extend([53, 1, 1, 255]);
    let (mut dhcp, rem) = Dhcp::parse(&packet).unwrap();
    assert!(rem.is_empty());
    assert_eq!(dhcp.op(), 1);
    assert_eq!(dhcp.xid(), 0x3903f326);
}

#[cfg(feature = "sctp")]
#[test]
fn sctp_single_chunk_parses() {
    use binparse_protocols::sctp::Sctp;
    let packet = vec![
        0x00, 0x50, 0x00, 0x50, 0x12, 0x34, 0x56, 0x78, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00,
        0x08, 0xaa, 0xbb, 0xcc, 0xdd,
    ];
    let (mut sctp, rem) = Sctp::parse(&packet).unwrap();
    assert!(rem.is_empty());
    assert_eq!(sctp.src_port(), 80);
    assert_eq!(sctp.dst_port(), 80);
}

#[cfg(feature = "bgp")]
#[test]
fn bgp_keepalive_parses() {
    use binparse_protocols::bgp::{Bgp, Bgp_body};
    let mut packet = vec![0xff; 16];
    packet.extend([0x00, 0x13]);
    packet.push(4);
    let (mut bgp, rem) = Bgp::parse(&packet).unwrap();
    assert!(rem.is_empty());
    assert_eq!(bgp.length(), 19);
    assert_eq!(bgp.msg_type(), 4);
    assert!(matches!(bgp.body().unwrap(), Bgp_body::Keepalive(_)));
}

#[cfg(feature = "mqtt_v3")]
#[test]
fn mqtt_v3_connect_parses() {
    use binparse::ParseResult;
    use binparse_protocols::mqtt_v3::{MqttPacket, MqttPacket_body};
    let packet = vec![
        0x10, 0x0a, 0x00, 0x04, b'M', b'Q', b'T', b'T', 0x04, 0x02, 0x00, 0x3c,
    ];
    let (mut mqtt, rem) = MqttPacket::parse(&packet).unwrap();
    assert!(rem.is_empty());
    assert_eq!(mqtt.packet_type(), 1);
    assert_eq!(*mqtt.remaining_length().unwrap(), 10);
    match mqtt.body().unwrap() {
        MqttPacket_body::Connect(c) => {
            assert_eq!(c.keep_alive(), 60);
            let name = c
                .proto_name()
                .unwrap()
                .collect::<ParseResult<Vec<_>>>()
                .unwrap();
            assert_eq!(name, b"MQTT");
        }
        _ => panic!("expected Connect"),
    }
}

#[cfg(feature = "mqtt_v3")]
#[test]
fn mqtt_v3_pingreq_parses() {
    use binparse_protocols::mqtt_v3::{MqttPacket, MqttPacket_body};
    let packet = vec![0xc0, 0x00];
    let (mut mqtt, rem) = MqttPacket::parse(&packet).unwrap();
    assert!(rem.is_empty());
    assert_eq!(mqtt.packet_type(), 12);
    assert_eq!(*mqtt.remaining_length().unwrap(), 0);
    assert!(matches!(mqtt.body().unwrap(), MqttPacket_body::PingReq(_)));
}

#[cfg(feature = "mqtt_v5")]
#[test]
fn mqtt_v5_connack_properties_sized_by_hook_varint() {
    use binparse::ParseResult;
    use binparse_protocols::mqtt_v5::{MqttPacket, MqttPacket_body};
    let packet = vec![0x20, 0x06, 0x00, 0x00, 0x03, 0x21, 0x00, 0x14];
    let (mut mqtt, rem) = MqttPacket::parse(&packet).unwrap();
    assert!(rem.is_empty());
    assert_eq!(mqtt.packet_type(), 2);
    assert_eq!(*mqtt.remaining_length().unwrap(), 6);
    match mqtt.body().unwrap() {
        MqttPacket_body::Connack(c) => {
            assert_eq!(c.reason_code(), 0);
            assert_eq!(*c.prop_len().unwrap(), 3);
            let props = c
                .properties()
                .unwrap()
                .collect::<ParseResult<Vec<_>>>()
                .unwrap();
            assert_eq!(props, vec![0x21, 0x00, 0x14]);
        }
        _ => panic!("expected Connack"),
    }
}
