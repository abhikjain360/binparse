# Writer Work — Next-Session Kickoff / Handoff

**Read `docs/writer-model-and-gaps.md` first** (the design model + the actionable
gap list in §12). This doc is the *operational* complement: current state, the
immediate plan, the GATE, the file map, and the in-flight caveats. Written
2026-06-14.

---

## TL;DR — what to do

The user wants **write-path benchmarks**: binparse vs mqttbytes / rumqttc /
rumqttd (MQTT) and vs simple-dns (DNS). **Neither MQTT nor DNS has a writer yet**
(confirmed: `generate_writers` emits 0 writer-structs for `mqtt_v3`, `mqtt_v5`,
`dns`). So the writers must be built first. Decided order: **MQTT writer → DNS
writer → write benches**. Build the MQTT pieces as *general* capabilities per the
model doc, not MQTT-specific hacks.

---

## Current state (2026-06-14)

- Branch **`main`** (all writer work consolidated here; force-pushed; history
  was scrubbed of a private work-machine hostname/paths — never reintroduce any
  work-machine names or paths in code, docs, or commits).
- **Reader**: mature — 15 protocols, `dissect()`, `@cache(len)`, borrowed hooks.
- **Writer**: best-effort; ships for the simpler protocols (Ethernet, Vlan,
  Ipv6, Udp, + option/chunk sub-structs = 11 writer-structs). Returns `None`
  (reader-only) on everything else. **MQTT / DNS / checksums are unbuilt.**
- The full write-path **design was worked out and is captured in
  `docs/writer-model-and-gaps.md`** — read it; it subsumes all the gaps under one
  model (derived fields × width-source × dependency-direction × resolution-time).
- Read-path MQTT bench already shipped (commit `5e82d88`: rumqttd broker-codec
  arm added alongside mqttbytes/rumqttc).

## In-flight — DO NOT collide

- **Another agent is adding `@cache(value)` codegen** (the attr is parsed but had
  no codegen). It changes read accessors to `&mut self` + reference-returning
  getters — visible in `binparse-bench/benches/mqtt.rs` (e.g. `let mut pkt`,
  `*c.prop_len()`). Don't revert those. Expect churn in read-side codegen and
  possibly `writer.rs`; **re-grep for function names, line numbers below will
  drift.**
- **Uncommitted in the tree, leave alone**: `dns-profile/` and `Cargo.*` (a bench
  agent's), plus the two new `docs/writer-*.md` (mine — commit them when the tree
  is clean). The `@cache` agent's edits are also uncommitted.

---

## Plan

### Step 1 — MQTT gap 1: varint-length union with FIXED variants
Unblocks `MqttPacketWriter` for CONNACK / PUBACK / PINGREQ / PINGRESP /
DISCONNECT / UNSUBACK (the fixed-size variants).

- **Bail to fix**: `writer.rs::classify`, the `Union` arm — returns `None` when
  `pending_hook_len.is_some()` (a union following a varint hook-len). Add a new
  `Layout::LenUnionHook` + classify path here.
- **Combine two existing emitters**:
  - `emit_len_union` — union body, but it backpatches a *fixed-width* len at a
    *fixed* offset.
  - `emit_dynamic_tail_hook` — varint write-then-measure, body offset =
    `prefix + len_width` (runtime), for a *byte-array* body.
  - New `emit_len_union_hook`: union body **+** varint prefix. `region_offset =
    prefix + len_width` (runtime); `encoded_len = prefix + width(region_len) +
    region_len`; `region_len = variant SIZE`.
- **Spec edit**: `remaining_length` in `mqtt_v3.bp` + `mqtt_v5.bp` declares only
  the read `@hook`. Add the write side. Two options (see model §2.1, §8):
  - explicit: `@write_hook(binparse.hooks.write_leb128_unsigned, binparse.hooks.leb128_unsigned_len)`
  - **preferred (more ergonomic):** single-arg `@write_hook(binparse.hooks.write_leb128_unsigned)`
    and have codegen **scratch-encode** (`[0u8;16]` on the stack) to learn the
    width. Pick one; the leb128 fns already exist in `binparse/src/hooks.rs`.
- **Make `build_union_layout` lenient**: today `push_union_variant` `?`-fails on a
  non-fixed non-wildcard variant, so one dynamic variant kills the whole union.
  For the LenUnionHook path, **skip** dynamic variants (don't push them) instead
  of failing, so the fixed variants still emit. (The writer's body enum need not
  cover every reader variant.)
- **Test**: round-trip in `binparse-protocols/tests/writers.rs` — build a v3
  CONNACK (or PUBACK) via the writer → parse via the reader → assert fields, and
  pin the wire bytes including the varint `remaining_length`.

### Step 2 — MQTT gap 2: dynamic union variants (CONNECT / PUBLISH)
The hard one; unblocks the connect/publish write benches.

- MQTT `Connect { proto_name: [u8; proto_name_len], …, @greedy payload }` and
  `Publish { topic: [u8; topic_len], @greedy payload }` are variants that are
  themselves dynamic structs (a derived-len sized array **+** a greedy tail).
- Approach (model §3): emit each variant as its **own `encoded_len`-composed
  sub-writer**; the union computes each variant's *runtime* size (not a const
  `SIZE`) and places the varint + body. Reuse the Forward/derived-len machinery so
  `proto_name_len`/`topic_len` are derived from the slice lengths (ergonomic:
  caller passes `topic: &[u8]`, never a length).

### Step 3 — DNS writer
- Compressed `qname`: the **no-width content-hook tail** form already exists
  (`emit_content_hook_no_width` + `write_backref_blob`, model §6). Wire the DNS
  spec's `@write_hook` + the classify path so `DnsWriter` emits. Mind the
  non-relocatable constraint (model §6): names are self-delimiting tails, which is
  why this works; do **not** put a varint length in front of a compressed region.

### Step 4 — Write-path benches → report numbers
- New benches (or extend `binparse-bench/benches/mqtt.rs` / `dns.rs`).
- **Confirmed competitor write APIs:**
  - rumqttd: `use rumqttd::protocol::{Protocol, v4::V4}; V4.write(packet, &mut buf) -> Result<usize>`
    (and `v5::V5`). Note: rumqttd write is broker→client, so it *can* write
    CONNACK (mirror of why it can't *read* it) — opposite direction from the read
    bench. Construct the `Packet`, write into a fresh `BytesMut`.
  - simple-dns: `Packet::build_bytes_vec_compressed()` (use the **compressed**
    variant to match binparse's compressed write) — also `build_bytes_vec()`.
  - mqttbytes / rumqttc-v4/v5-next: per-packet `.write(&mut BytesMut)` —
    confirm exact method when writing the bench.
  - binparse: `XWriter::to_vec(&XContent)` (alloc-free core is `write_into(&mut
    [u8])`; for the bench, `to_vec` is the fair "produce bytes" call).
- Mirror the existing read-bench groups (mqtt_v3_connect / v3_publish /
  v5_connack; dns full + partial). Report medians.

---

## GATE — run after EVERY writer change (non-negotiable)

A writer regression breaks the **protocol crate too**, because `build.rs` runs
`generate_writers` for all enabled protocols. So:

1. `cargo run -p binparse-codegen --example test`        (codegen smoke)
2. `cargo test -p binparse-codegen`                      (writer_runtime.rs, writer_snapshots.rs)
3. `cargo build -p binparse-protocols --features all`    (build.rs generate_writers, all 15)
4. `cargo test  -p binparse-protocols --features all`    (smoke.rs + writers.rs round-trips)
5. `cargo clippy --all-targets`

Writer work is **sequential** (everything touches `writer.rs`) — one change at a
time; supervisor verifies + commits each. (A past §4.5 regression slipped exactly
because a protocol-suite run was skipped.)

---

## File map — `binparse-codegen/src/writer.rs` (~4243 lines; grep names, lines drift)

Architecture (reader): `lib.rs → struct_.rs → field.rs → type_/mod.rs → type_/*`.
The **writer is a separate pass in `writer.rs`**. `use binparse_dsl as ast;`.

| fn / item | approx line | role |
|---|---|---|
| `classify` | 231 | dispatcher; bail points are `return None`/`Ok(None)`. Union arm ~452 = MQTT bail |
| `Layout` enum | 162 | add `LenUnionHook` here |
| `generate` | 177 | match Layout → emitter; add arm |
| `classify_len_union` / `emit_len_union` | 1542 / 3359 | fixed-width-len union template |
| `classify_dynamic_tail_hook` / `emit_dynamic_tail_hook` | 1742 / 2870 | varint-len byte-array tail template |
| `build_union_layout` / `push_union_variant` | 1351 / 1493 | variant classification (fixed-only today; make lenient) |
| `emit_content_hook_no_width` | 3097 | no-width tail = DNS backref pattern |
| `affine_size_shape` / `len_value_expr` | 1644 / 1676 | `x ± k` only; needs affine *scaling* (model §9) for IHL etc. — NOT needed for MQTT/DNS |
| `parse_write_hook` / `parse_write_hook_encode_only` | 1771 / 1822 | `@write_hook` arg parsing (2-arg vs 1-arg encode-only) |

Hooks lib: `binparse/src/hooks.rs` — `write_leb128_unsigned`, `leb128_unsigned_len`
(MQTT varint), `write_backref_blob` (DNS compression), `WriteHookContext { offset,
written }`.

---

## Design open questions (only block checksum/typestate work — NOT MQTT/DNS)

The model doc §11 flags two unresolved designs: the **dependency-annotation
syntax** (how a spec declares `field = f(range | fields | enclosing.X)`) and the
**typestate forward-builder shape** (model §4.3). These are needed for checksums /
MACs / the safe incremental write mode / cross-layer pseudo-headers — **the
networking "valid packet" frontier**. MQTT (structural, `Content`-mode) and DNS
(existing tail hook) need *neither*, so Steps 1–4 can proceed immediately; design
these before touching checksum/forward-builder codegen.
