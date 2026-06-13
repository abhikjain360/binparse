#![no_main]

use libfuzzer_sys::fuzz_target;

include!(concat!(env!("OUT_DIR"), "/generated.rs"));

fn double_it(value: u16) -> u32 {
    u32::from(value) * 2
}

fn parse_cstring(data: &[u8]) -> (String, usize) {
    binparse::hooks::cstring(data)
}

fuzz_target!(|data: &[u8]| {
    if let Ok((packet, _)) = Baseline::parse(data) {
        let _ = packet.n();
        let _ = packet.word();
        let _ = packet.be();
        let _ = packet.flag_a();
        let _ = packet.flag_b();
        if let Ok(fixed) = packet.fixed() {
            let _ = fixed.collect::<binparse::ParseResult<Vec<_>>>();
        }
        if let Ok(inner) = packet.inner() {
            let _ = inner.a();
            let _ = inner.b();
        }
        if let Ok(dyns) = packet.dyns() {
            let _ = dyns.collect::<binparse::ParseResult<Vec<_>>>();
        }
        let _ = packet.dyns_bit_range();
        let _ = packet.payload_bit_range();
        let _ = packet.pair();
        match packet.payload() {
            Ok(Baseline_payload::One(one)) => {
                let _ = one.x();
            }
            Ok(Baseline_payload::Unknown(_)) => {}
            Err(_) => {}
        }
    }

    if let Ok((packet, _)) = Hooked::parse(data) {
        let _ = packet.prefix();
        let _ = packet.value();
        let _ = packet.name();
        let _ = packet.name_bit_range();
    }

    if let Ok((packet, _)) = StructArray::parse(data)
        && let Ok(items) = packet.items()
    {
        let _ = packet.items_bit_range();
        for item in items.flatten() {
            let _ = item.a();
            let _ = item.b();
        }
    }

    if let Ok((packet, _)) = SizeExpr::parse(data) {
        let _ = packet.xs_bit_range();
        if let Ok(xs) = packet.xs() {
            let _ = xs.collect::<binparse::ParseResult<Vec<_>>>();
        }
    }

    if let Ok((packet, _)) = Mixed::parse(data) {
        let _ = packet.a();
        let _ = packet.b();
        let _ = packet.c();
        let _ = packet.version();
        let _ = packet.ihl();
        let _ = packet.low();
        let _ = packet.high();
        let _ = packet.vals_bit_range();
        if let Ok(vals) = packet.vals() {
            let _ = vals.collect::<binparse::ParseResult<Vec<_>>>();
        }
    }

    if let Ok((packet, _)) = Conditional::parse(data) {
        let _ = packet.version();
        let _ = packet.ihl();
        if let Some(Ok(options)) = packet.options() {
            let _ = options.collect::<binparse::ParseResult<Vec<_>>>();
        }
        let _ = packet.big();
        let _ = packet.tail();
        let _ = packet.options_bit_range();
        let _ = packet.tail_bit_range();
    }

    if let Ok((packet, _)) = Validated::parse(data) {
        let _ = packet.magic();
        let _ = packet.version();
        let _ = packet.ihl();
        let _ = packet.total_len();
        let _ = packet.reserved();
        let _ = packet.flags();
    }

    if let Ok((packet, _)) = Rest::parse(data) {
        let _ = packet.n();
        let _ = packet.words_bit_range();
        if let Ok(words) = packet.words() {
            let _ = words.collect::<binparse::ParseResult<Vec<_>>>();
        }
    }

    if let Ok((packet, _)) = CStr::parse(data) {
        let _ = packet.after();
        let _ = packet.name_bit_range();
        if let Ok(name) = packet.name() {
            let _ = name.collect::<binparse::ParseResult<Vec<_>>>();
        }
    }

    if let Ok((packet, _)) = Capped::parse(data) {
        let _ = packet.count();
        let _ = packet.vals_bit_range();
        if let Ok(vals) = packet.vals() {
            let _ = vals.collect::<binparse::ParseResult<Vec<_>>>();
        }
    }

    if let Ok((packet, _)) = Opts::parse(data) {
        let _ = packet.opts_bit_range();
        if let Ok(opts) = packet.opts() {
            for opt in opts.flatten() {
                let _ = opt.kind();
                let _ = opt.body();
            }
        }
    }

    if let Ok((packet, _)) = Padded::parse(data) {
        let _ = packet.flags();
        let _ = packet.n();
        let _ = packet.tail();
        let _ = packet.data_bit_range();
        let _ = packet.tail_bit_range();
        if let Ok(items) = packet.data() {
            let _ = items.collect::<binparse::ParseResult<Vec<_>>>();
        }
    }

    if let Ok((packet, _)) = Dispatch::parse(data) {
        let _ = packet.kind();
        let _ = packet.body_bit_range();
        match packet.body() {
            Ok(Dispatch_body::Msg(msg)) => {
                let _ = msg.msg_len();
                if let Ok(bytes) = msg.data() {
                    let _ = bytes.collect::<binparse::ParseResult<Vec<_>>>();
                }
            }
            Ok(Dispatch_body::Checked(checked)) => {
                let _ = checked.version();
            }
            Err(Error::UNKNOWN_KIND { kind }) => {
                let _ = kind;
            }
            Err(Error::Parse(_)) => {}
        }
    }

    if let Ok((packet, _)) = ConcatUnion::parse(data) {
        let _ = packet.tail();
        let _ = packet.pair_bit_range();
        let (first, second, third) = packet.pair();
        let _ = first;
        if let Ok(ConcatUnion_pair_1::Word(word)) = second {
            let _ = word.w();
        }
        if let Ok(ConcatUnion_pair_2::Bytes(bytes)) = third {
            let _ = bytes.count();
            if let Ok(items) = bytes.data() {
                let _ = items.collect::<binparse::ParseResult<Vec<_>>>();
            }
        }
    }

    if let Ok((packet, _)) = Bounded::parse(data) {
        let _ = packet.tag();
        let _ = packet.length();
        let _ = packet.value_bit_range();
        if let Ok(inner) = packet.value() {
            let _ = inner.a();
            let _ = inner.b();
        }
        let _ = packet.value_rest();
        let _ = packet.after();
    }
});
