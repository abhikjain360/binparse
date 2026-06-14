//! MQTT 3.1.1 + v5: binparse vs the rumqtt codecs.
//!
//! Three rumqtt codecs are compared: the shipping standalone `mqttbytes 0.6`
//! (what rumqttc depends on today), the in-progress next-line client codec
//! bundled in `rumqttc-v4-next`/`rumqttc-v5-next`, and the broker-side codec in
//! `rumqttd 0.20` (`protocol::v4::V4`/`v5::V5` via the `Protocol::read_mut`
//! trait method). All three decode a whole packet out of a `BytesMut` into owned
//! structs (allocating topic/payload), whereas binparse borrows the input and
//! reads fields lazily — so this is decode-into-owned vs zero-copy, not a
//! like-for-like memory model. The rumqtt buffers are rebuilt per iteration via
//! `iter_batched` setup (untimed) because `read()` drains them; binparse borrows
//! a `'static` slice and needs no setup.

use std::hint::black_box;

use bytes::BytesMut;
use criterion::{BatchSize, Criterion, criterion_group, criterion_main};

use binparse_bench::*;

const MAX: usize = 256 * 1024;

fn sum_bp(it: impl Iterator<Item = binparse::ParseResult<u8>>) -> u64 {
    it.map(|b| b.unwrap() as u64).fold(0, u64::wrapping_add)
}

fn buf(bytes: &[u8]) -> BytesMut {
    let mut b = BytesMut::with_capacity(bytes.len());
    b.extend_from_slice(bytes);
    b
}

fn v3_connect(c: &mut Criterion) {
    use binparse_protocols::mqtt_v3::{MqttPacket, MqttPacket_body};
    let mut g = c.benchmark_group("mqtt_v3_connect");
    g.bench_function("binparse", |b| {
        b.iter(|| {
            let (pkt, _) = MqttPacket::parse(black_box(MQTT_V3_CONNECT)).unwrap();
            let pt = pkt.packet_type() as u64;
            match pkt.body().unwrap() {
                MqttPacket_body::Connect(c) => {
                    black_box(pt ^ c.keep_alive() as u64 ^ sum_bp(c.proto_name().unwrap()))
                }
                _ => unreachable!(),
            }
        })
    });
    g.bench_function("mqttbytes_0.6", |b| {
        b.iter_batched(
            || buf(MQTT_V3_CONNECT),
            |mut buf| match mqttbytes::v4::read(&mut buf, MAX).unwrap() {
                mqttbytes::v4::Packet::Connect(c) => {
                    black_box(c.keep_alive as u64 ^ c.client_id.len() as u64)
                }
                _ => unreachable!(),
            },
            BatchSize::SmallInput,
        )
    });
    g.bench_function("rumqttc_v4_next", |b| {
        use rumqttc_v4_next::mqttbytes::v4;
        b.iter_batched(
            || buf(MQTT_V3_CONNECT),
            |mut buf| match v4::Packet::read(&mut buf, MAX).unwrap() {
                v4::Packet::Connect(c) => black_box(c.keep_alive as u64 ^ c.client_id.len() as u64),
                _ => unreachable!(),
            },
            BatchSize::SmallInput,
        )
    });
    g.bench_function("rumqttd_0.20", |b| {
        use rumqttd::protocol::{Packet, Protocol, v4::V4};
        b.iter_batched(
            || buf(MQTT_V3_CONNECT),
            |mut buf| match V4.read_mut(&mut buf, MAX).unwrap() {
                Packet::Connect(c, ..) => black_box(c.keep_alive as u64 ^ c.client_id.len() as u64),
                _ => unreachable!(),
            },
            BatchSize::SmallInput,
        )
    });
    g.finish();
}

fn v3_publish(c: &mut Criterion) {
    use binparse_protocols::mqtt_v3::{MqttPacket, MqttPacket_body};
    let mut g = c.benchmark_group("mqtt_v3_publish");
    g.bench_function("binparse", |b| {
        b.iter(|| {
            let (pkt, _) = MqttPacket::parse(black_box(MQTT_V3_PUBLISH)).unwrap();
            match pkt.body().unwrap() {
                MqttPacket_body::Publish(p) => black_box(
                    p.topic_len() as u64 ^ sum_bp(p.topic().unwrap()) ^ sum_bp(p.payload().unwrap()),
                ),
                _ => unreachable!(),
            }
        })
    });
    g.bench_function("mqttbytes_0.6", |b| {
        b.iter_batched(
            || buf(MQTT_V3_PUBLISH),
            |mut buf| match mqttbytes::v4::read(&mut buf, MAX).unwrap() {
                mqttbytes::v4::Packet::Publish(p) => {
                    black_box(p.topic.len() as u64 ^ p.payload.len() as u64 ^ p.qos as u64)
                }
                _ => unreachable!(),
            },
            BatchSize::SmallInput,
        )
    });
    g.bench_function("rumqttc_v4_next", |b| {
        use rumqttc_v4_next::mqttbytes::v4;
        b.iter_batched(
            || buf(MQTT_V3_PUBLISH),
            |mut buf| match v4::Packet::read(&mut buf, MAX).unwrap() {
                v4::Packet::Publish(p) => {
                    black_box(p.topic.len() as u64 ^ p.payload.len() as u64 ^ p.qos as u64)
                }
                _ => unreachable!(),
            },
            BatchSize::SmallInput,
        )
    });
    g.bench_function("rumqttd_0.20", |b| {
        use rumqttd::protocol::{Packet, Protocol, v4::V4};
        b.iter_batched(
            || buf(MQTT_V3_PUBLISH),
            |mut buf| match V4.read_mut(&mut buf, MAX).unwrap() {
                Packet::Publish(p, _) => {
                    black_box(p.topic.len() as u64 ^ p.payload.len() as u64)
                }
                _ => unreachable!(),
            },
            BatchSize::SmallInput,
        )
    });
    g.finish();
}

fn v5_connack(c: &mut Criterion) {
    use binparse_protocols::mqtt_v5::{MqttPacket, MqttPacket_body};
    let mut g = c.benchmark_group("mqtt_v5_connack");
    g.bench_function("binparse", |b| {
        b.iter(|| {
            let (pkt, _) = MqttPacket::parse(black_box(MQTT_V5_CONNACK)).unwrap();
            match pkt.body().unwrap() {
                MqttPacket_body::Connack(c) => black_box(
                    c.reason_code() as u64 ^ c.prop_len().unwrap() ^ sum_bp(c.properties().unwrap()),
                ),
                _ => unreachable!(),
            }
        })
    });
    g.bench_function("mqttbytes_0.6", |b| {
        b.iter_batched(
            || buf(MQTT_V5_CONNACK),
            |mut buf| match mqttbytes::v5::read(&mut buf, MAX).unwrap() {
                mqttbytes::v5::Packet::ConnAck(c) => {
                    black_box(c.session_present as u64 ^ c.code as u64 ^ c.properties.is_some() as u64)
                }
                _ => unreachable!(),
            },
            BatchSize::SmallInput,
        )
    });
    g.bench_function("rumqttc_v5_next", |b| {
        use rumqttc_v5_next::mqttbytes::v5;
        b.iter_batched(
            || buf(MQTT_V5_CONNACK),
            |mut buf| match v5::Packet::read(&mut buf, None).unwrap() {
                v5::Packet::ConnAck(c) => {
                    black_box(c.session_present as u64 ^ c.code as u64 ^ c.properties.is_some() as u64)
                }
                _ => unreachable!(),
            },
            BatchSize::SmallInput,
        )
    });
    // No rumqttd arm here: its `protocol::v5` codec is the broker's read path,
    // which only decodes client->server packets. CONNACK is server->client, so
    // `V5::read_mut` reaches `unreachable!()` on it. rumqttd participates in the
    // v3 CONNECT/PUBLISH groups (both client->server) instead.
    g.finish();
}

criterion_group!(benches, v3_connect, v3_publish, v5_connack);
criterion_main!(benches);
