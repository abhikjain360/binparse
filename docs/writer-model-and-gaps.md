# Write-Path Codegen: Unifying Model & Completeness Spec

Status: **design** (not yet implemented). Derived from the write-path design
discussion. This is the reference for closing the writer-vs-reader coverage gap
("serialize anything you can dissect") and for emitting *valid* wire packets
(checksums/MACs), not just structurally-correct bytes.

The current writer is best-effort: `classify` returns `None` and ships
reader-only output whenever it meets a shape it cannot serialize. This doc names
the single model those shapes fall under, the few genuinely-new capabilities
needed, the design policies, and the concrete gap list against `writer.rs`.

---

## 1. Core abstraction: derived fields with declared dependencies

Every field is either:

- **plain** — value supplied directly by the caller (a primitive, a byte slice,
  a fixed array). Written as-is.
- **derived** — value is a function of *other* fields: `field = f(deps...)`. The
  dependencies may sit **earlier or later** in the message; codegen schedules.

Length, element-count, offset/pointer, checksum/CRC/MAC, flags-derived-from-
presence, and content transforms (compress/encrypt) are **all instances of the
same derived-field mechanism**. There is no separate "checksum subsystem" or
"length subsystem" — there is one dependency engine with instances.

This is the abstraction the whole write path should be built around. "Length
prefix" was too narrow a framing; it's one derived field among many.

---

## 2. Three orthogonal axes

Every derived field is classified on three axes. The axes are independent; their
combination determines how (and when) codegen emits the write.

### 2.1 Width source — how the field's serialized byte-width is known

| source | cost of the sizing pass | example |
|---|---|---|
| fixed type | constant | `u16`, `[u8; 4]` |
| explicit width fn | arithmetic | `leb128_unsigned_len(n)` |
| scratch-encode of a **scalar** | ~free (encode a number into `[0u8;16]`) | varint length, no declared width fn |
| `"default"` = lens length / length-preserving | arithmetic | XOR/obfuscation tail (encoded size == content size) |
| **structural composition** (Σ children's `encoded_len`) | arithmetic, recurses | nested length-prefixed bodies |
| scratch-encode of a **content-dependent blob** | the expensive 2-pass | gzip body, opaque compression |

Only the **last row** is expensive. Everything else — including deeply nested
length-prefixed varint bodies — sizes by cheap arithmetic.

### 2.2 Dependency value-kind — what the field reads from its deps

- **structural** — size, element count, offset/position, whole-message total.
  Computable from the layout *without writing any content bytes*.
- **content** — the actual bytes of a range (checksum, CRC, MAC).
- **cross-layer** — fields in the *enclosing* struct (TCP/UDP pseudo-header,
  AEAD additional-authenticated-data). Not reachable by an in-message edge.

### 2.3 Resolution time — *forced* by the derived field's own width

This is the load-bearing rule, and it is not a design choice — it is forced:

- A **variable-width** derived field (a varint length) **must** resolve at
  **size-time** and may depend **only on structural** properties.
  *Proof:* to place the bytes after it you need its width → its value; if its
  value needed later *bytes*, those bytes sit after it and cannot be placed until
  its width is known → circular. Hence variable-width ⇒ deps ⊆ structural (which
  are computable without writing).
- A **fixed-width** derived field (a `u16` checksum) has a known width, so
  everything after it places regardless of its value. Write the whole buffer with
  its slot reserved (zeroed), then compute its value from the **final bytes**,
  then backpatch. Hence fixed-width ⇒ may depend on **content**, resolved at
  **byte-time**, in a final pass.

Corollary: lengths (often variable-width) resolve early; checksums (fixed-width)
resolve late. A varint length that depended on later *bytes* is unsatisfiable.

---

## 3. `encoded_len` is recursive

The sizing pass is a pure arithmetic walk over §2.1 width sources. A struct's
`encoded_len` is the sum of its fields' `encoded_len`s; for a variable field it
calls the field's width source, **not** its encoder. It composes through nesting
and through a varint prefix (compute body size by composition → varint width by
scratch/width-fn → reserve exactly). This yields a single exact allocation.

The only thing that escapes the cheap sizing pass is a field whose size requires
**encoding content-dependent data** (real compression). That, and only that, is
the expensive case.

---

## 4. Write modes / public APIs

1. **`Content` struct → `to_vec` / `write_into`** — single pass, protocol order.
   Type system guarantees completeness (struct literal) and variant validity
   (enum). The always-safe path. *Today: shipped for the supported layouts.*
2. **`Lens` + random-access setters** (`new(buf, lens)` + `set_x`/`x_mut`) —
   available only when all widths are known up front (offsets pinnable). *Today:
   shipped, but NOT type-safe — forgotten setter → silent zero; wrong-length
   `copy_from_slice` → panic; only `NotEnoughSpace` is a clean `Result`.*
3. **Forward / streaming typestate builder** (no lens, head→tail, cursor-tracked)
   — *Not built.* The substrate for content-dependent and non-relocatable
   encodings (write in place, in order, each field sees `ctx.written`). Made safe
   by typestate: each `write_x` consumes `self` → returns the next state; only the
   terminal state exposes `finish()`. Kills the §4.2 footguns; enforces order by
   construction. Cannot express back-dependencies (it reaches a prefix before the
   later data exists) → forward-only protocols; the rest use §5.

---

## 5. Dependency scheduling (the back-dependency engine)

Build a DAG of derived fields (edge: derived → its deps). Topologically sort it.

- **Cycle → hard error** (e.g. two checksums each covering the other; a length
  that includes its own variable width is the benign fixpoint sub-case, see §11).
- **Variable-width derived** → resolved at **size-time**, written in place during
  the forward pass.
- **Fixed-width derived** → **reserve** the slot (zero-init), assemble, then a
  **final backpatch pass** computes it from final bytes in topo order.
- **Self-in-range** (checksum over its own slot) → natural: the slot is zero
  during computation because buffers are zero-initialized.
- The fused unit for "variable prefix in front of a later region" evaluates as:
  size the region (composition) → varint width falls out → reserve → write region
  → backpatch prefix. No shift, no heap (see §8).

A checksum is exactly the **fixed-width, byte-time, content-range** instance of
this engine. It needs three things the single-length-prefix case never exercised:
range-as-dependency (hook gets the finalized buffer slice), the topo-sorted DAG,
and cycle detection. Nothing more.

---

## 6. Non-relocatable encodings

An **absolute-reference encoding** embeds offsets measured from the start of the
message, pointing back into the same buffer. Canonical case: DNS name
compression — a 2-byte pointer `0xC0 | (off >> 8), off & 0xFF` where `off` is the
absolute message offset of an earlier copy of the suffix (hence `write_backref_blob`
takes `ctx.offset` + `ctx.written`).

Consequence: the encoded bytes are valid **only at the exact message offset they
were computed for**. They cannot be scratch-encoded-and-moved, nor shifted after
the fact. So they must be written **in place, in order** — i.e. via the §4.3
forward builder. This is a sharper boundary than "has a width fn."

**Policy:** a variable-width (varint) length prefix in front of a non-relocatable
region is **disallowed** (compile error), not handled cleverly — you cannot know
the prefix width without encoding the region, and you cannot relocate the region
after. Use a fixed-width prefix or restructure the spec.

---

## 7. Cross-layer context (pseudo-headers / AAD)

TCP/UDP checksums are computed over the payload **plus** a pseudo-header
synthesized from the *enclosing* IP layer (src/dst address, protocol, transport
length). AEAD tags authenticate AAD drawn from headers. These inputs live in a
**different struct**, so the in-message dependency edge cannot reach them.

Requirement: the writer must be able to receive **enclosing-layer inputs** — the
write-side analogue of the read-side `HookContext.enclosing`. Intra-struct
derived fields (IP header checksum, Ethernet FCS, any CRC over "my own bytes")
need none of this; only cross-layer derivations do.

---

## 8. No-allocation policy

- **`write_into(&mut [u8])` is always heap-free.** No internal temp buffers,
  ever. Variable prefixes use reserve-in-place + backpatch (§5); the opaque-size
  compression corner uses a fixed-width prefix or a 2-pass encode (CPU, not heap).
- **`to_vec` allocates the output `Vec` once** whenever the width is knowable
  (which is why knowable widths are preferred — they also eliminate the
  grow-and-retry reallocs).
- Grow-and-retry (`cap *= 2`, then `truncate`) is permitted **only** for the
  opaque content-dependent-size tail (no width source at all).

---

## 9. Affine length/offset relations (required generalization)

Derived lengths and offsets must support **`a·x + b`**, not just `x ± b`. The
current `affine_size_shape` handles only `path` and `path ± int_literal`.
Networking needs scaling:

- IPv4 `IHL = header_bytes / 4`
- TCP data offset `= header_bytes / 4`
- IPv6 extension-header `len = bytes / 8 − 1`

So `len_value_expr` / `affine_size_shape` must carry a multiplier and divisor
(with a divisibility precondition checked at write time → `ValueTooLarge`-style
error if the region size isn't a clean multiple).

---

## 10. Coverage check: protocol shapes → mechanism

| shape | example | handled by |
|---|---|---|
| fixed header | Ethernet, ARP | plain fields |
| length-prefixed / TLV | DHCP options, TLS records | §3 + §5 (size-time) |
| varint length | MQTT remaining_length, protobuf | §2.1 scratch/width-fn |
| definite-length nesting | ASN.1 DER, protobuf | §3 recursive |
| indefinite / sentinel-terminated | ASN.1 indefinite, C strings, DNS label run | §4.3 forward + terminator |
| discriminated union | MQTT body, TCP options by kind | union + disc (codegen gap §12) |
| counted array | DNS `ancount`, repeated TLVs | §2.2 structural **count** |
| offset / pointer field | ELF/PE/ZIP, DNS compression target | §2.2 structural **offset** |
| end-relative offset | ZIP central directory | §2.2 structural **total** |
| checksum / CRC / FCS (intra-struct) | IPv4 hdr cksum, Ethernet FCS | §5 fixed-width/byte-time/range |
| checksum w/ pseudo-header | TCP/UDP | §5 + §7 cross-layer |
| AEAD tag + AAD | TLS 1.3, ESP | §5 (MAC) + §7 (AAD) |
| content transform | gzip body, encryption | §2.1 (predictable size → width fn; opaque → 2-pass/tail) |
| non-relocatable compression | DNS names | §6 forward, in place |
| relocatable compression | HPACK (index-based) | §2.1 scratch / 2-pass |
| conditional / optional | IPv4 options, TCP options | `Option` in `Content`, conditional `encoded_len` |
| padding / alignment | IP options to 4B, ATM 53B cell | §2.2 structural size |
| non-byte length units | IHL, data-offset, IPv6 ext-len | §9 affine scaling |
| self-including length | SCTP chunk length | §9 (`+k`) / §11 fixpoint |
| length over nested variable items | TLS extensions block | §3 + §5 |
| length + checksum over same range | IPv4 total-length + checksum | §5 DAG topo-sort |

No single-message field shape examined escapes the model.

---

## 11. Edges & open questions

- **Self-including-length fixpoint.** A varint length that counts its own encoded
  width: `L = bodylen + width(L)`. Solvable by iterating `width` to a fixpoint
  (it's monotonic, ≤2 steps in practice). Decide: support, or require fixed-width
  for self-including lengths.
- **Transform-field size predictability.** AEAD (`ct = pt + tag`) and block
  ciphers (pad to block) have closed-form size → width fn; truly opaque transforms
  fall to the 2-pass/tail path. Encode the size formula as a width fn where known.
- **Typestate shape.** Concrete state-type naming, how to expose `x_mut()`-style
  borrows inside a consuming builder, and how partial random-access composes with
  the forward builder (likely: not at all — they're distinct modes).
- **Dependency annotation syntax.** How a spec declares "this field is derived
  from {range | fields | enclosing.X} via `fn`," and the size-time vs byte-time
  intent (or infer it purely from the field's width per §2.3).
- **`@cache(value)` interaction.** Read-side memoization is orthogonal to all of
  the above; no write-path impact expected, but verify accessor-signature changes
  (`&mut self`, reference-returning getters) don't leak into writer assumptions.

---

## 12. Current codegen gaps (what `classify` returns `None` on today)

Mapped to the model. Each is an implementation gap, not a model gap.

1. **Union body behind a varint-hook length** — `classify` bails when a `union`
   follows a pending hook-len (the MQTT `remaining_length` + `body` shape). Needs
   the §5 fused unit with §2.1 scratch/width-fn sizing. *(MQTT, fixed variants.)*
2. **Dynamic-size union variants** — `push_union_variant` only accepts variants
   whose fields are all fixed (`classify_fixed_items`). MQTT `Connect`/`Publish`
   (`[u8; len]` + greedy `payload`) are rejected, so the whole union is. Needs
   variants emitted as their own §3-composed sub-writers. *(MQTT CONNECT/PUBLISH.)*
3. **Content-range derived fields (checksums/CRC/MAC)** — no §5 byte-time
   backpatch pass exists; no annotation. *(IPv4/TCP/UDP/ICMP, Ethernet FCS.)*
4. **Cross-layer context** — no write-side `enclosing` channel. *(TCP/UDP cksum.)*
5. **Affine scaling** — `affine_size_shape` is `x ± k` only. *(IHL, data-offset.)*
6. **Forward typestate builder** — not built; no compile-time-safe incremental
   mode; no in-place substrate for non-relocatable encodings beyond the existing
   tail-only no-width content hook.
7. **Compressed-name write behind any prefix** — only the self-delimiting tail
   form (`write_backref_blob`) exists. *(DNS qname in positions with a prefix.)*
8. **Multiple dynamic regions in one struct; nested dynamic struct-refs; arrays
   of dynamic-size structs** — only single-trailing-dynamic layouts today.
9. **`@len` on concat; conditionals combined with dynamic tails** — previously
   noted deferrals.

The actionable implementation list is "every `return None` in `writer.rs`,"
categorized above.

---

## 13. Explicit non-goals (not field shapes — don't contort the model for these)

- **Stream multiplexing / interleaving** (HTTP/2 frames, QUIC streams, RTP). A
  message serializer emits one message; interleaving is a layer above.
- **Fragmentation / reassembly.** Splitting a payload across packets is caller
  logic; the per-fragment *fields* are plain.
- **Bit-packing across byte boundaries wider than 8 bits.** Existing reader/writer
  constraint (bitfield width < 8); a 13-bit field is modeled as a wider primitive
  with masking, not a native bitfield.

---

## 14. Summary

The write path reduces to **one** abstraction — *derived fields with declared
dependencies* — classified on three axes (**width source** × **dependency
value-kind** × **resolution time**, the last forced by width). `encoded_len` is
recursive and cheap except for genuine compression. Three capabilities remain to
build: the **forward typestate builder** (§4.3), the **derived-field DAG with
byte-time backpatch** (§5, of which checksums are the fixed-width instance), and
**cross-layer context** (§7) — plus the **affine-scaling** generalization (§9)
and the no-alloc, no-relocate policies (§6, §8). With those, the model covers
every single-message wire-protocol field shape we could enumerate (§10); the only
exclusions are non-field, cross-message concerns (§13).
